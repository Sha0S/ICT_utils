#![allow(non_snake_case)]
#![allow(clippy::collapsible_match)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use anyhow::{bail, Result};
use chrono::{DateTime, Local, NaiveDateTime};
use log::{debug, error, info, warn};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc::{self, SyncSender},
    time::Duration,
};
use tiberius::{Client, Query};
use tokio::net::TcpStream;
use tokio_util::compat::TokioAsyncWriteCompatExt;
use tray_item::{IconSource, TrayItem};

#[derive(Debug, Clone, Copy, PartialEq)]
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

/*
    Get the [.xml] files in the target [dir], which don't end in "_AOI" or "_AXI"
*/
fn get_entries<P: AsRef<Path> + std::fmt::Debug>(dir: P) -> Vec<PathBuf> {
    let mut ret = Vec::new();

    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|e| e == "xml" || e == "XML") {
                if let Some(filestem) = path.file_stem() {
                    let filestem = filestem.to_string_lossy();
                    if !(filestem.ends_with("_AOI") || filestem.ends_with("_AXI")) {
                        ret.push(path);
                    }
                }
            }
        }
    }

    debug!("Scanned {dir:?}, found {} entries.", ret.len());

    ret
}

/*
- Serial_NMBR: VARCHAR[30]
- Board_NMBR: TINYINT // The board number in the XML is wrong!
- Program: VARCHAR[30]
- Station: VARCHAR[30]
- Operator: VARCHAR[30] (allow NULL)
- Result: VARCHAR[10]
- Date_Time: DATETIME
- Failed: VARCHAR[500] (allow NULL)
*/

#[derive(Debug, Default)]
struct Panel {
    path: PathBuf,

    Program: String,
    Station: String,
    Operator: String,
    Repair_DT: NaiveDateTime,
    Inspection_DT: NaiveDateTime,

    Boards: Vec<Board>,
}

#[derive(Debug, Default, Clone)]
struct Board {
    Serial_NMBR: String,
    Board_NMBR: usize,
    Result: String,
    Failed: Vec<String>,
}

fn parse_xml(path: &PathBuf, line: &str) -> Result<Panel> {
    debug!("Processing XML: {path:?}");

    let mut ret = Panel {
        path: path.clone(),
        ..Default::default()
    };

    let file = std::fs::read_to_string(path)?;
    let xml = roxmltree::Document::parse(&file)?;

    let root = xml.root_element();
    let mut repaired = false;
    let mut failed = false;

    if let Some(ginfo) = root
        .children()
        .find(|f| f.has_tag_name("GlobalInformation"))
    {
        for sub_child in ginfo.children().filter(|f| f.is_element()) {
            match sub_child.tag_name().name() {
                /*"Station" => {
                    if let Some(x) = sub_child.children().find(|f| f.has_tag_name("Name")) {
                        ret.Station = x.text().unwrap_or_default().to_owned();
                        debug!("Station: {}", ret.Station);
                    }
                }*/
                "Program" => {
                    if let Some(x) = sub_child
                        .children()
                        .find(|f| f.has_tag_name("InspectionPlanName"))
                    {
                        ret.Program = x.text().unwrap_or_default().to_owned();
                        debug!("Program: {}", ret.Program);
                    }
                }
                "Inspection" => {
                    let date =
                        if let Some(x) = sub_child.children().find(|f| f.has_tag_name("Date")) {
                            if let Some(y) = x.children().find(|f| f.has_tag_name("End")) {
                                y.text().unwrap_or_default()
                            } else {
                                ""
                            }
                        } else {
                            ""
                        };
                    let time =
                        if let Some(x) = sub_child.children().find(|f| f.has_tag_name("Time")) {
                            if let Some(y) = x.children().find(|f| f.has_tag_name("End")) {
                                y.text().unwrap_or_default()
                            } else {
                                ""
                            }
                        } else {
                            ""
                        };

                    if !date.is_empty() && !time.is_empty() {
                        let t = format!("{date} {time}");
                        debug!("Raw time string: {t}");
                        ret.Inspection_DT =
                            NaiveDateTime::parse_from_str(&t, "%Y%m%d %H%M%S").unwrap_or_default();
                        debug!("Date_Time: {:?}", ret.Inspection_DT);
                    }
                }
                "Repair" => {
                    repaired = true;

                    if let Some(x) = sub_child
                        .children()
                        .find(|f| f.has_tag_name("OperatorName"))
                    {
                        ret.Operator = x.text().unwrap_or_default().to_owned();
                        debug!("OperatorName: {}", ret.Operator);
                    }

                    let date =
                        if let Some(x) = sub_child.children().find(|f| f.has_tag_name("Date")) {
                            if let Some(y) = x.children().find(|f| f.has_tag_name("End")) {
                                y.text().unwrap_or_default()
                            } else {
                                ""
                            }
                        } else {
                            ""
                        };
                    let time =
                        if let Some(x) = sub_child.children().find(|f| f.has_tag_name("Time")) {
                            if let Some(y) = x.children().find(|f| f.has_tag_name("End")) {
                                y.text().unwrap_or_default()
                            } else {
                                ""
                            }
                        } else {
                            ""
                        };

                    if !date.is_empty() && !time.is_empty() {
                        let t = format!("{date} {time}");
                        debug!("Raw time string: {t}");
                        ret.Repair_DT =
                            NaiveDateTime::parse_from_str(&t, "%Y%m%d %H%M%S").unwrap_or_default();
                        debug!("Date_Time: {:?}", ret.Repair_DT);
                    }
                }
                _ => (),
            }
        }
    } else {
        error!("Could not find <GlobalInformation>!");
        bail!("Could not find <GlobalInformation>!");
    }

    if let Some(pcb_info) = root.children().find(|f| f.has_tag_name("PCBInformation")) {
        let count = pcb_info.children().filter(|f| f.is_element()).count();
        debug!("PCB count: {}", count);
        ret.Boards = vec![Board::default(); count];

        for (i, child) in pcb_info
            .children()
            .filter(|f| f.tag_name().name() == "SinglePCB")
            .enumerate()
        {
            let mut serial = String::new();
            let mut result = String::new();

            for sub_child in child.children().filter(|f| f.is_element()) {
                match sub_child.tag_name().name() {
                    "Barcode" => {
                        serial = sub_child.text().unwrap_or_default().to_owned();
                    }
                    "Result" => {
                        result = sub_child.text().unwrap_or_default().to_owned();
                    }
                    _ => {}
                }
            }

            debug!("{i}: serial: {serial}, result: {result}");
            if !serial.is_empty() && !result.is_empty() {
                if result != "PASS" {
                    failed = true;
                }
                ret.Boards[i].Serial_NMBR = serial;
                ret.Boards[i].Result = result;
            } else {
                error!("SinglePCB sub-fields missing!");
                bail!("SinglePCB sub-fields missing!");
            }
        }
    }

    if repaired {
        debug!("XML is for repair station. Searching for repair information");
        if let Some(comp_info) = root
            .children()
            .find(|f| f.has_tag_name("ComponentInformation"))
        {
            for window in comp_info.children().filter(|f| f.is_element()) {
                let mut WinID = String::new();
                let mut PCBNumber = String::new();
                let mut Result = String::new();

                for sub_child in window.children().filter(|f| f.is_element()) {
                    match sub_child.tag_name().name() {
                        "WinID" => {
                            WinID = sub_child.text().unwrap_or_default().to_string();
                        }
                        "PCBNumber" => {
                            PCBNumber = sub_child.text().unwrap_or_default().to_string();
                        }
                        "Result" => {
                            if let Some(t) = sub_child
                                .children()
                                .find(|f| f.has_tag_name("ErrorDescription"))
                            {
                                Result = t.text().unwrap_or_default().to_string();
                            }
                        }
                        _ => {}
                    }
                }

                if !(WinID.is_empty() || PCBNumber.is_empty() || Result.is_empty()) {
                    debug!(
                        "Window found! WinID: {WinID}, PCBNumber: {PCBNumber}, Result: {Result}"
                    );

                    if Result != "Pszeudohiba" {
                        if let Ok(x) = PCBNumber.parse::<usize>() {
                            if let Some(board) = ret.Boards.get_mut(x) {

                                if let Some(c) = WinID.rfind('-') {
                                    let split = WinID.split_at(c);
                                    WinID = split.0.to_string();
                                }
                                
                                board.Failed.push(WinID);
                            }
                        } else {
                            error!("Could not parse PCBNumber: {PCBNumber}");
                        }
                    } else {
                        debug!("Window marked as pseudo error.");
                    }
                } else {
                    error!("Window interpreting error! WinID: {WinID}, PCBNumber: {PCBNumber}, Result: {Result}");
                }
            }
        }
    } else if failed {
        debug!("XML is for AOI/AXI station. Searching for failed windows.");
        if let Some(comp_info) = root
            .children()
            .find(|f| f.has_tag_name("ComponentInformation"))
        {
            for window in comp_info.children().filter(|f| f.is_element()) {
                let mut WinID = String::new();
                let mut PCBNumber = String::new();
                let mut Result = String::new();

                for sub_child in window.children().filter(|f| f.is_element()) {
                    match sub_child.tag_name().name() {
                        "WinID" => {
                            WinID = sub_child.text().unwrap_or_default().to_string();
                        }
                        "PCBNumber" => {
                            PCBNumber = sub_child.text().unwrap_or_default().to_string();
                        }
                        "Analysis" => {
                            if let Some(t) = sub_child.children().find(|f| f.has_tag_name("Result"))
                            {
                                Result = t.text().unwrap_or_default().to_string();
                            }
                        }
                        _ => {}
                    }
                }

                if !(WinID.is_empty() || PCBNumber.is_empty() || Result.is_empty())
                {
                    if Result != "0" {
                        debug!(
                            "Window found! WinID: {WinID}, PCBNumber: {PCBNumber}, Result: {Result}"
                        );
    
                        if let Ok(x) = PCBNumber.parse::<usize>() {
                            if x == 0 {
                                error!("BoardNumber is 0. Was excepting 1+");
                            } else if let Some(board) = ret.Boards.get_mut(x-1) {

                                if let Some(c) = WinID.rfind('-') {
                                    let split = WinID.split_at(c);
                                    WinID = split.0.to_string();
                                }

                                board.Failed.push(WinID);
                            } else {
                                error!("Could not find board number {x}");
                            }

                        } else {
                            error!("Could not parse PCBNumber: {PCBNumber}");
                        }
                    }

                } else {
                    error!("Window interpreting error! WinID: {WinID}, PCBNumber: {PCBNumber}, Result: {Result}");
                }
            }
        }
    }

    // Sort boards, so they will be in the "correct" order
    ret.Boards
        .sort_by(|p1, p2| p1.Serial_NMBR.cmp(&p2.Serial_NMBR));
    // Set board number to the "correct" value
    for (i, b) in ret.Boards.iter_mut().enumerate() {
        b.Board_NMBR = i + 1;
    }

    // Set station name
    ret.Station = if repaired {
        format!("{}_HARAN", line)
    } else {
        format!("{}_AOI_AXI", line)
    };

    Ok(ret)
}

async fn connect(
    tib_config: tiberius::Config,
) -> anyhow::Result<tiberius::Client<tokio_util::compat::Compat<TcpStream>>> {
    let tcp = TcpStream::connect(tib_config.get_addr()).await?;
    tcp.set_nodelay(true)?;
    let client = Client::connect(tib_config, tcp.compat_write()).await?;

    Ok(client)
}

async fn upload_panels(panels: &Vec<Panel>, config: &ICT_config::Config) -> Result<()> {
    if panels.is_empty() {
        error!("Panel buffer is empty!");
        bail!("Panel buffer is empty!");
    }

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

    // Upload new results
    let mut qtext = String::from(
        "INSERT INTO [dbo].[SMT_AOI] 
        ([Serial_NMBR], [Board_NMBR], [Program], [Station], [Operator], [Result], [Date_Time], [Failed])
        VALUES",
    );

    for panel in panels {
        for board in &panel.Boards {
            let mut fails = board.Failed.join(", ");
            fails.truncate(490);

            qtext += &format!(
                "('{}', '{}', '{}', '{}', '{}', '{}', '{}', '{}'),",
                board.Serial_NMBR,
                board.Board_NMBR,
                panel.Program,
                panel.Station,
                panel.Operator,
                board.Result,
                if panel.Operator.is_empty() {
                    panel.Inspection_DT
                } else {
                    panel.Repair_DT
                },
                fails
            );
        }
    }
    qtext.pop(); // removes last ','

    debug!("Upload: {}", qtext);
    let query = Query::new(qtext);
    query.execute(&mut client).await?;

    debug!("Upload OK");

    Ok(())
}

fn new_path(dir: &Path, path: &Path) -> Result<PathBuf> {
    if let Ok(x) = path.metadata() {
        let ct: DateTime<Local> = x.modified()?.into();
        let subdir = format!("{}", ct.format("%Y_%m_%d"));

        let new_dir = dir.join(subdir);
        if !new_dir.exists() {
            fs::create_dir(&new_dir)?;
        }

        if let Some(filename) = path.file_name() {
            let final_path = new_dir.join(filename);
            Ok(final_path)
        } else {
            error!("Could not read filename!");
            bail!("Could not read filename!");
        }
    } else {
        error!("Could not read metadata!");
        bail!("Could not read metadata!");
    }
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

    // spawn SQL thread
    tokio::spawn(async move {
        let config = match ICT_config::Config::read(ICT_config::CONFIG) {
            Ok(c) => c,
            Err(e) => {
                error!("{e}");
                sql_tx.send(Message::FatalError).unwrap();
                panic!("{e}");
            }
        };

        let dir = PathBuf::from(config.get_AOI_dir());
        let chunks = config.get_AOI_chunks();

        sql_tx.send(Message::SetIcon(IconCollor::Green)).unwrap();
        let mut panels: Vec<Panel> = Vec::new();

        loop {
            let entries = get_entries(&dir);
            for chunk in entries.chunks(chunks) {
                for entry in chunk {
                    if let Ok(panel) = parse_xml(entry, config.get_AOI_line()) {
                        panels.push(panel);
                    } else {
                        error!("XML parsing failed!");
                        sql_tx.send(Message::SetIcon(IconCollor::Yellow)).unwrap();
                    }
                }

                if upload_panels(&panels, &config).await.is_ok() {
                    for panel in panels {
                        if panel.path.exists() {
                            if let Ok(path) = new_path(&dir, &panel.path) {
                                if fs::rename(&panel.path, path).is_err() {
                                    error!("Failed to move {:?}", panel.path);
                                    sql_tx.send(Message::SetIcon(IconCollor::Yellow)).unwrap();
                                }
                            } else {
                                error!("Failed to make destination path for {:?}", panel.path);
                                sql_tx.send(Message::SetIcon(IconCollor::Yellow)).unwrap();
                            }
                        } else {
                            error!("Entry does not exist anymore: {:?}", panel.path);
                            sql_tx.send(Message::SetIcon(IconCollor::Yellow)).unwrap();
                        }
                    }
                } else {
                    error!("Upload failed!");
                    sql_tx.send(Message::SetIcon(IconCollor::Red)).unwrap();
                }
                panels = Vec::new();
            }

            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });

    let (mut tray, _) = init_tray(tx.clone());
    let mut last_color = String::new();

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
