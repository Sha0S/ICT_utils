#![allow(non_snake_case)]
#![allow(clippy::collapsible_match)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use anyhow::{bail, Result};
use chrono::NaiveDateTime;
use log::{debug, error, info, warn};
use std::{
    fs,
    path::PathBuf,
    sync::mpsc::{self, SyncSender},
    time::Duration,
};
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

fn get_entries(dir: &str) -> Vec<PathBuf> {
    let mut ret = Vec::new();

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|e| e == "xml" || e == "XML") {
                ret.push(path);
            }
        }
    }

    ret
}

/*
- Serial_NMBR: VARCHAR[30]
- Board_NMBR: INT // The board number in the XML is wrong!
- Program: VARCHAR[30]
- Station: VARCHAR[30]
- Operator: VARCHAR[30]
- Result: VARCHAR[10]
- Date_Time: DATETIME
- Pseudo_Errors: VARCHAR[MAX] (allow NULL)
- True_Errors:   VARCHAR[MAX] (allow NULL)
*/

#[derive(Debug, Default)]
struct Panel {
    Program: String,
    Station: String,
    Operator: String,
    Date_Time: NaiveDateTime,

    Boards: Vec<Board>,
}

#[derive(Debug, Default, Clone)]
struct Board {
    Serial_NMBR: String,
    Result: String,
    Pseudo_Errors: String,
    True_Errors: String,
}

fn parse_xml(path: &PathBuf) -> Result<Panel> {
    let mut ret = Panel::default();

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
                "Station" => {
                    if let Some(x) = sub_child.children().find(|f| f.has_tag_name("Name")) {
                        ret.Station = x.text().unwrap_or_default().to_owned();
                        debug!("Station: {}", ret.Station);
                    }
                }
                "Program" => {
                    if let Some(x) = sub_child
                        .children()
                        .find(|f| f.has_tag_name("InspectionPlanName"))
                    {
                        ret.Program = x.text().unwrap_or_default().to_owned();
                        debug!("Program: {}", ret.Program);
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
                        ret.Date_Time =
                            NaiveDateTime::parse_from_str(&t, "%Y%m%d %H%M%S").unwrap_or_default();
                        debug!("Date_Time: {:?}", ret.Date_Time);
                    }
                }
                _ => (),
            }
        }
    } else {
        error!("Could not find <GlobalInformation>!");
        bail!("Could not find <GlobalInformation>!");
    }

    if let Some(pcb_info) = root
    .children()
    .find(|f| f.has_tag_name("PCBInformation"))
    {
        let count = pcb_info.children().filter(|f| f.is_element()).count();
        debug!("PCB count: {}", count);
        ret.Boards = vec![Board::default(); count];

        for (i, child) in pcb_info.children().filter(|f| f.tag_name().name() == "SinglePCB").enumerate() {
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
                    _ => {
                    } 
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
        debug!("Searching for repair information");
        if let Some(comp_info) = root
        .children()
        .find(|f| f.has_tag_name("ComponentInformation"))
        {
            // to do
        }
    } else if failed {
        debug!("Panel awaiting repair!");
        bail!("Panel awaiting repair!")
    }

    Ok(ret)
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

    // spawn TCP thread
    tokio::spawn(async move {
        let config = match ICT_config::Config::read(ICT_config::CONFIG) {
            Ok(c) => c,
            Err(e) => {
                error!("{e}");
                sql_tx.send(Message::FatalError).unwrap();
                panic!("{e}");
            }
        };

        let dir = config.get_AOI_dir();
        let chunks = config.get_AOI_chunks();

        sql_tx.send(Message::SetIcon(IconCollor::Green)).unwrap();
        let mut panels: Vec<Panel> = Vec::new();

        loop {
            let entries = get_entries(dir);
            for chunk in entries.chunks(chunks) {
                for entry in chunk {
                    if let Ok(panel) = parse_xml(entry) {
                        panels.push(panel);
                    }
                }

                //upload_panels(&panels);
                panels.clear();
            }

            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });

    let (mut tray, _) = init_tray(tx.clone());
    let mut last_color = String::new();

    loop {
        match rx.recv() {
            Ok(Message::Quit) => {
                info!("Stoping server due user request");
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
