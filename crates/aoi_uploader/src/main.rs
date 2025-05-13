#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(non_snake_case)]

use anyhow::{bail, Result};
use chrono::{DateTime, Local};
use log::{debug, error, info, warn};
use std::{
    collections::HashSet, fs, path::PathBuf, sync::mpsc::{self, SyncSender}, time::Duration
};
use tiberius::{Client, Query};
use tokio::{net::TcpStream, time::sleep};
use tokio_util::compat::TokioAsyncWriteCompatExt;
use tray_item::{IconSource, TrayItem};

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }

    env_logger::init();
    info!("Starting uploader");

    let (tx, rx) = mpsc::sync_channel(1);
    let sql_tx = tx.clone();

    // SQL uploader thread
    tokio::spawn(async move {
        
        let config = ICT_config::Config::read(ICT_config::CONFIG);
        if config.is_err() {
            error!("Failed to load configuration! Terminating.");
            sql_tx.send(Message::FatalError).unwrap();
            return;
        }
        let config = config.unwrap();

        if config.get_aoi_dir().is_empty() {
            error!("Configuration is missing AOI dir field!");
            sql_tx.send(Message::FatalError).unwrap();
            return;  
        }

        let log_dir = PathBuf::from(config.get_aoi_dir());
        let tminus = Duration::from_secs(config.get_aoi_tminus());

        let mut client = 
        loop {
            if let Ok(client) =  create_connection(&config).await {
                break client;
            }

            sql_tx.send(Message::SetIcon(IconCollor::Red)).unwrap();
            error!("Failed to connect to the SQL server, retrying in 60s.");
            sleep(Duration::from_secs(60)).await;
        }
        ;        


        sql_tx.send(Message::SetIcon(IconCollor::Green)).unwrap();

        // Stores the path of the previous loop
        // it is used to determine if a log was processed last time, or not
        let mut log_buffer: HashSet<PathBuf> = HashSet::new();

        // Uploader main loop
        loop {

            // 0 - check connection, reconnect if needed
            loop {
                match client.execute("SELECT 1", &[]).await {
                    Ok(_) => {
                        break;
                    }
                    Err(_) => {
                        warn!("Connection to DB lost, reconnecting!");
                        client = 
                        loop {
                            if let Ok(client) =  create_connection(&config).await {
                                break client;
                            }

                            sql_tx.send(Message::SetIcon(IconCollor::Red)).unwrap();
                            error!("Failed to connect to the SQL server, retrying in 60s.");
                            sleep(Duration::from_secs(60)).await;
                        }
                        ;  
                    }
                }
            }


            debug!("AOI auto update started");
            let start_time = chrono::Local::now();
            let mut new_logs = 0;
            let mut skipped_logs = 0;

            // 1 - get date_time of the last update
            if let Ok(last_date) = ICT_config::get_last_date() {
                let last_date = last_date - tminus; 

                // 2 - get possible directories
                //let target_dirs = get_subdirs_for_aoi(&log_dir, &last_date);
                let target_dirs = vec![log_dir.clone()];

                // 3 - get logs
                if let Ok(logs) = get_logs(target_dirs, last_date) {
                    let new_log_buffer: HashSet<PathBuf> = HashSet::from_iter(logs.iter().cloned());

                    // 4 - process_logs

                    let mut processed_logs = Vec::new();
                    for log in logs {
                        if log_buffer.contains(&log) {
                            skipped_logs += 1;
                            debug!("Buffer already contains log. Skipping!")
                        } else {
                            if let Ok(plog) = AOI_log_file::Panel::load_xml(&log) {
                                    processed_logs.push(plog);
                            } else {
                                error!("Failed to process log: {:?}", log);
                            }
                        }
                    }

                    let mut all_ok = true;
                    // uploading in chunks
                    for chunk in processed_logs.chunks(config.get_aoi_chunks()) {
                        // 5 - craft the SQL query

                        let mut qtext = String::from(
                            "INSERT INTO [dbo].[SMT_AOI_RESULTS] 
                            ([Serial_NMBR], [Date_Time], [Station], [Program], [Variant], [Operator], [Result], [Data])
                            VALUES",
                        );

                        for panel in chunk {

                            let time;
                            let operator;
                            if let Some(rep) = &panel.repair {
                                time = rep.date_time;
                                operator = rep.operator.as_str();
                            } else {
                                operator = "";
                                time = panel.inspection_date_time.unwrap();
                            }

                            

                            for board in &panel.boards {

                                let data = serde_json::to_string(&board.windows).unwrap();
                    
                                qtext += &format!(
                                    "('{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}'),",
                                    board.barcode,
                                    time,
                                    panel.station,
                                    panel.inspection_plan,
                                    panel.variant,
                                    operator,
                                    if board.result { "Pass" } else { "Fail" },
                                    data
                                );
                            }
                        }
                        qtext.pop(); // removes last ','

                        // 6 - execute query
                        debug!("Upload: {}", qtext);
                        let query = Query::new(qtext);
                        let result = query.execute(&mut client).await;

                        debug!("Result: {:?}", result);

                        

                        if let Err(e) = result {
                            all_ok = false;
                            error!("Upload failed: {e}");
                        } else {
                            debug!("Upload succesfull!");
                            let res = result.unwrap();
                            new_logs += res.total();
                        }
                    }

                    // 7 - update last_date or report the error
                    if all_ok {
                        sql_tx.send(Message::SetIcon(IconCollor::Green)).unwrap();
                        if let Err(e) = ICT_config::set_last_date(start_time) {
                            error!("Failed to update last_time! {}", e);
                        };
                        log_buffer = new_log_buffer;

                    } else {
                        sql_tx.send(Message::SetIcon(IconCollor::Red)).unwrap();
                        error!("Upload failed - not setting new last_date");
                    }
                } else {
                    error!("Failed to gather logs!");
                }
            } else {
                error!("Failed to read last_date!");
            }

            if new_logs > 0 {
                let delta_t = chrono::Local::now() - start_time;
                info!("Uploaded {new_logs} new results in {}s (skipped: {skipped_logs})", delta_t.num_seconds());
            }
            


            // wait and repeat
            sleep(Duration::from_secs(config.get_aoi_deltat())).await;
        }
    });

    let (mut tray, _) = init_tray(tx.clone());
    let mut last_color = String::new();

    // Tray event loop
    loop {
        match rx.recv() {
            Ok(Message::Quit) => {
                info!("Stoping due user request");
                break;
            }
            Ok(Message::FatalError) => {
                error!("Fatal error encountered, shuting down!");
                break;
            }
            Ok(Message::SetIcon(icon)) => {
                debug!("Icon change requested: {:?}", icon);

                let target_col = match icon {
                    IconCollor::Green => "green-icon",
                    IconCollor::Yellow => "yellow-icon",
                    IconCollor::Red => "red-icon",
                    IconCollor::Grey => "grey-icon",
                    IconCollor::Purple => "purple-icon",
                };

                if target_col == last_color {
                    continue;
                }
                if tray.set_icon(IconSource::Resource(target_col)).is_ok() {
                    debug!("Icon set to: {target_col}");
                    last_color = target_col.to_owned();
                } else {
                    warn!("Failed to change icon to: {target_col}");
                }
            }
            _ => {}
        }
    }

    Ok(())
}

async fn connect(
    tib_config: tiberius::Config,
) -> anyhow::Result<tiberius::Client<tokio_util::compat::Compat<TcpStream>>> {
    let tcp = TcpStream::connect(tib_config.get_addr()).await?;
    tcp.set_nodelay(true)?;
    let client = Client::connect(tib_config, tcp.compat_write()).await?;

    Ok(client)
}

async fn create_connection(config: &ICT_config::Config) -> Result<Client<tokio_util::compat::Compat<TcpStream>>> {
        // Tiberius configuartion:

        let sql_server = config.get_server().to_owned();
        let sql_user = config.get_username().to_owned();
        let sql_pass = config.get_password().to_owned();
    
        let mut tib_config = tiberius::Config::new();
        tib_config.host(sql_server);
        tib_config.authentication(tiberius::AuthMethod::sql_server(sql_user, sql_pass));
        tib_config.trust_cert(); // Most likely not needed.
    
        let mut client_tmp = connect(tib_config.clone()).await;
        let mut tries = 0;
        while client_tmp.is_err() && tries < 3 {
            client_tmp = connect(tib_config.clone()).await;
            tries += 1;
        }
    
        if client_tmp.is_err() {
            bail!("Connection to DB failed!")
        }
        let mut client = client_tmp?;
    
        // USE [DB]
        let qtext = format!("USE [{}]", config.get_database());
        debug!("USE DB: {}", qtext);
        let query = Query::new(qtext);
        query.execute(&mut client).await?;

        Ok(client)
}

fn get_logs(target_dirs: Vec<PathBuf>, last_date: DateTime<Local>) -> Result<Vec<PathBuf>> {
    let mut ret = Vec::new();

    for dir in target_dirs {
        for file in fs::read_dir(dir)? {
            let file = file?;
            let path = file.path();

            if path.is_file() && path.extension().is_some_and(|f| f == "xml" || f =="XML") {
                if let Ok(x) = path.metadata() {
                    let ct: chrono::DateTime<chrono::Local> = x.modified().unwrap().into();
                    if ct >= last_date {

                        // filtering temporary files from AOI / AXI
                        if let Some(filestem) = path.file_stem() {
                            let filestem = filestem.to_string_lossy();
                            if !(filestem.ends_with("_AOI") || filestem.ends_with("_AXI")) {
                                ret.push(path);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(ret)
}

/*
fn get_subdirs_for_aoi(log_dir: &Path, start: &chrono::DateTime<chrono::Local>) -> Vec<PathBuf> {
    let mut ret = Vec::new();

    let mut start_date = start.date_naive();
    let end_date = chrono::Local::now().date_naive();

    while start_date <= end_date {
        debug!("\tdate: {}", start_date);

        let sub_dir = start_date.format("%Y_%m_%d");

        debug!("\tsubdir: {}", sub_dir);

        let new_path = log_dir.join(sub_dir.to_string());
        debug!("\tfull path: {:?}", new_path);

        if new_path.exists() {
            debug!("\t\tsubdir exists");
            ret.push(new_path);
        }

        start_date = start_date.succ_opt().unwrap();
    }

    ret
}
*/
#[derive(Debug)]
pub enum IconCollor {
    Green,
    Yellow,
    Red,
    Grey,
    Purple,
}
pub enum Message {
    Quit,
    FatalError,
    SetIcon(IconCollor),
}

pub fn init_tray(tx: SyncSender<Message>) -> (TrayItem, Vec<u32>) {
    let mut ret = Vec::new();

    let mut tray = TrayItem::new("AOI Uploader", IconSource::Resource("red-icon")).unwrap();

    ret.push(
        // 0
        tray.inner_mut().add_label_with_id("AOI Uploader").unwrap(),
    );

    tray.inner_mut().add_separator().unwrap();

    let quit_tx = tx.clone();
    tray.add_menu_item("Quit", move || {
        quit_tx.send(Message::Quit).unwrap();
    })
    .unwrap();

    (tray, ret)
}
