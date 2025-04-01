#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(non_snake_case)]

use anyhow::{bail, Result};
use chrono::{DateTime, Local};
use log::{debug, error, info, warn};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc::{self, SyncSender},
    time::Duration,
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

        // Loading configuration, and creating a connection to the SQL server
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

        let mut client = loop {
            if let Ok(client) = create_connection(&config).await {
                break client;
            }

            sql_tx.send(Message::SetIcon(IconCollor::Red)).unwrap();
            error!("Failed to connect to the SQL server, retrying in 60s.");
            sleep(Duration::from_secs(60)).await;
        };

        sql_tx.send(Message::SetIcon(IconCollor::Green)).unwrap();

        // Main loop
        loop {
            // 0 - check connection, reconnect if needed
            loop {
                match client.execute("SELECT 1", &[]).await {
                    Ok(_) => {
                        break;
                    }
                    Err(_) => {
                        warn!("Connection to DB lost, reconnecting!");
                        client = loop {
                            if let Ok(client) = create_connection(&config).await {
                                break client;
                            }

                            sql_tx.send(Message::SetIcon(IconCollor::Red)).unwrap();
                            error!("Failed to connect to the SQL server, retrying in 60s.");
                            sleep(Duration::from_secs(60)).await;
                        };
                    }
                }
            }

            debug!("CCL5 auto update started");

            // 1 - get logs and pdfs from target dir
            let processed_files = get_logs(&log_dir);
            if let Ok((logs, pdfs)) = processed_files {

                // 2 - process_logs
                let mut processed_logs = Vec::new();
                for log in &logs {
                    if let Ok(plog) = CCL5_log_file::Board::load(log) {
                        processed_logs.push(plog);
                    } else {
                        error!("Failed to process log: {:?}", log);
                    }
                }

                // 3 - uploading in chunks
                for chunk in processed_logs.chunks(config.get_aoi_chunks()) {

                    // 4 - craft the SQL query
                    let mut qtext = String::from(
                        "INSERT INTO [dbo].[AOI_RESULTS] 
                        ([Barcode], [Lead_DMC], [ShortDMC], [Board], [Side], [Line] [Operator], [Result], [Palette_size], [FileDate], [RowUpdated], [Logfile])
                        VALUES",
                    );

                    for board in chunk {
                        qtext += &format!(
                                "('{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}'),",
                                board.serial,
                                board.serial,
                                board.short_dmc(),
                                board.program_id(),
                                board.side,
                                config.get_station_name(),
                                board.user,
                                board.result,
                                board.boards_on_panel,
                                board.date_time,
                                board.date_time,
                                board.log.file_name().unwrap().to_string_lossy()
                            );
                    }
                    qtext.pop(); // removes last ','

                    // 5 - execute query
                    debug!("Upload: {}", qtext);
                    let query = Query::new(qtext);
                    let result = query.execute(&mut client).await;

                    debug!("Result: {:?}", result);

                    if let Err(e) = result {
                        error!("Upload failed: {e}");
                    } else {
                        debug!("Upload succesfull!");
                        // 6 - move files to subdir
                        todo!();
                    }
                }
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

async fn create_connection(
    config: &ICT_config::Config,
) -> Result<Client<tokio_util::compat::Compat<TcpStream>>> {
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

// Return value: Result<(Vec<logfiles>, Vec<pdf_files>)>
fn get_logs(dir: &Path) -> Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let mut ret = Vec::new();
    let mut ret_pdf = Vec::new();

    for file in fs::read_dir(dir)? {
        let file = file?;
        let path = file.path();

        if path.is_file() {
            let file_name = path.filename().unwrap();
            if file_name.starts_with("V1") {
                if path.extension().is_some_and(|f| f == "txt") {
                    ret.push(path);
                } else if path.extension().is_some_and(|f| f == "pdf") {
                    ret_pdf.push(path);
                }
            }
        }
    }

    Ok((ret, ret_pdf))
}

fn move_logs(dest: &Path, logs: (Vec<PathBuf>, Vec<PathBuf>)) -> Result<()> {


    Ok(())
}


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
