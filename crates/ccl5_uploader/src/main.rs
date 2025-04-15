#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(non_snake_case)]

use anyhow::{bail, Result};
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

macro_rules! error_and_bail {
    ($($arg:tt)+) => {
        error!($($arg)+);
        bail!($($arg)+);
    };
}

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
        let config = Config::read();
        if config.is_err() {
            error!("Failed to load configuration! Terminating.");
            sql_tx.send(Message::FatalError).unwrap();
            return;
        }
        let config = config.unwrap();

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
            let processed_files = get_logs(&config.log_dir);
            if let Ok((logs, pdfs)) = processed_files {
                // 3 - uploading in chunks
                for chunk in logs.chunks(config.chunks) {
                    // 2 - process_logs
                    let mut processed_logs = Vec::new();
                    for log in chunk {
                        if let Ok(plog) = CCL5_log_file::Board::load(log) {
                            processed_logs.push(plog);
                        } else {
                            error!("Failed to process log: {:?}", log);
                        }
                    }

                    // 4 - craft the SQL query
                    let mut qtext = String::from(
                        "INSERT INTO [dbo].[AOI_RESULTS] 
                        ([Barcode], [Lead_DMC], [ShortDMC], [Board], [Side], [Line], [Operator], [Result], [Palette_size], [FileDate], [RowUpdated], [FileName])
                        VALUES",
                    );

                    for board in processed_logs {
                        qtext += &format!(
                                "('{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}'),",
                                board.serial,
                                board.serial,
                                board.short_dmc(),
                                board.program_id(),
                                board.side,
                                config.station,
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
                        if let Err(e) = move_files(&config.dest_dir, chunk) {
                            error!("Moving log files failed: {e}");
                        }
                    }
                }
                if let Err(e) = move_files(&config.dest_dir, &pdfs) {
                    error!("Moving pdf files failed: {e}");
                }
            }

            // wait and repeat
            sleep(Duration::from_secs(config.deltat)).await;
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
    config: &Config,
) -> Result<Client<tokio_util::compat::Compat<TcpStream>>> {
    // Tiberius configuartion:

    let mut tib_config = tiberius::Config::new();
    tib_config.host(&config.server);
    tib_config.authentication(tiberius::AuthMethod::sql_server(
        &config.username,
        &config.password,
    ));
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
    let qtext = format!("USE [{}]", config.database);
    debug!("USE DB: {}", qtext);
    let query = Query::new(qtext);
    query.execute(&mut client).await?;

    Ok(client)
}

// Return value: Result<(Vec<logfiles>, Vec<pdf_files>)>
fn get_logs<P: AsRef<Path>>(dir: &P) -> Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let mut ret = Vec::new();
    let mut ret_pdf = Vec::new();

    for file in fs::read_dir(dir)? {
        let file = file?;
        let path = file.path();

        if path.is_file() {
            let file_name = path.file_stem().unwrap().to_string_lossy().into_owned();
            let file_extension = path.extension();
            if file_name.starts_with("V1") {
                if file_extension.is_some_and(|f| f == "txt") {
                    ret.push(path);
                } else if file_extension.is_some_and(|f| f == "pdf") {
                    ret_pdf.push(path);
                }
            }
        }
    }

    Ok((ret, ret_pdf))
}

// Moving a array for files to the dest directory,
// in a subdir based on their creation date
fn move_files<P: AsRef<Path>>(dest: &P, files: &[PathBuf]) -> Result<()> {
    let dest_dir = dest.as_ref();
    if !dest_dir.is_dir() {
        info!(
            "Found no directory [{:?}], attempting to create it.",
            dest_dir
        );
        std::fs::create_dir_all(dest_dir)?;
    }
    

    // iterating over the files, and moving them
    for file in files {
        if let Some(filename) = file.file_name() {

            // Generating subdir based on the file dreation date
            let datetime: chrono::DateTime<chrono::Local> = file.metadata()?.modified()?.into();
            let subdir = datetime.format("%Y_%m_%d").to_string();
            let dest_dir_final = dest_dir.join(&subdir);
            if !dest_dir_final.is_dir() {
                info!(
                    "Found no directory [{:?}], attempting to create it.",
                    dest_dir_final
                );
                std::fs::create_dir_all(&dest_dir_final)?;
            }

            let mut dest_file_name = dest_dir_final.clone().join(filename);

            // If the dest_file_name already exists, then add a counter as a sufix
            if dest_file_name.is_file() {
                let mut index = 1;
                let stem = file.file_stem().unwrap().to_string_lossy();
                let ext = file.extension().unwrap().to_string_lossy();

                while dest_file_name.is_file() {
                    let new_filename = format!("{}_{}.{}", stem, index, ext);
                    dest_file_name = dest_dir_final.clone().join(new_filename);
                    index += 1;
                }
            }

            debug!("Moving file from {:?} to {:?}", file, dest_file_name);
            std::fs::rename(file, dest_file_name)?;
        }
    }

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

    ret.push(tray.inner_mut().add_label_with_id("AOI Uploader").unwrap());

    tray.inner_mut().add_separator().unwrap();

    let quit_tx = tx.clone();
    tray.add_menu_item("Quit", move || {
        quit_tx.send(Message::Quit).unwrap();
    })
    .unwrap();

    (tray, ret)
}

#[derive(Debug, Default)]
pub struct Config {
    server: String,
    database: String,
    password: String,
    username: String,

    station: String,
    log_dir: String,
    dest_dir: String,
    chunks: usize,
    deltat: u64,
}

impl Config {
    pub fn read() -> anyhow::Result<Config> {
        let mut c = Config::default();

        if let Ok(config) = ini::Ini::load_from_file(".\\config.ini") {
            if let Some(jvserver) = config.section(Some("JVSERVER")) {
                // mandatory fields:
                if let Some(server) = jvserver.get("SERVER") {
                    c.server = server.to_owned();
                }
                if let Some(password) = jvserver.get("PASSWORD") {
                    c.password = password.to_owned();
                }
                if let Some(username) = jvserver.get("USERNAME") {
                    c.username = username.to_owned();
                }
                if let Some(database) = jvserver.get("DATABASE") {
                    c.database = database.to_owned();
                }

                if c.server.is_empty()
                    || c.password.is_empty()
                    || c.username.is_empty()
                    || c.database.is_empty()
                {
                    error_and_bail!("ER: Missing fields from configuration file!");
                }
            } else {
                error_and_bail!("ER: Could not find [JVSERVER] field!");
            }

            if let Some(app) = config.section(Some("AOI")) {
                if let Some(station) = app.get("STATION") {
                    c.station = station.to_owned();
                } else {
                    c.station = "LINE5".to_string();
                }

                if let Some(dir) = app.get("DIR") {
                    c.log_dir = dir.to_owned();
                }

                if let Some(dir) = app.get("DEST") {
                    c.dest_dir = dir.to_owned();
                }

                if let Some(chunks) = app.get("CHUNKS") {
                    c.chunks = chunks.parse().unwrap_or(10);
                } else {
                    c.chunks = 10;
                }

                if let Some(chunks) = app.get("DELTA_T") {
                    c.deltat = chunks.parse().unwrap_or(600);
                } else {
                    c.deltat = 600;
                }

                if c.log_dir.is_empty() || c.dest_dir.is_empty() {
                    error_and_bail!("ER: Missing field in configuration file! AOI/DIR and AOI/DEST fields are mandatory!");
                }
            } else {
                error_and_bail!("ER: Could not find [JVSERVER] field!");
            }
        } else {
            error_and_bail!("ER: Could not read configuration file!");
        }

        Ok(c)
    }
}
