#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release
#![allow(non_snake_case)]
#![allow(clippy::too_many_arguments)]

use std::{
    env,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use chrono::{NaiveDateTime, TimeDelta};
use egui::{Color32, RichText, Vec2};
use egui_extras::{Column, TableBuilder};
use log::{debug, error};
use tiberius::{Client, Query};
use tokio::net::TcpStream;
use tokio_stream::StreamExt;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

use ICT_config::*;

mod scan_for_logs;
use scan_for_logs::*;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn load_icon() -> egui::IconData {
    let (icon_rgba, icon_width, icon_height) = {
        let icon = include_bytes!("..\\..\\..\\icons\\question.png");
        let image = image::load_from_memory(icon)
            .expect("Failed to open icon path")
            .into_rgba8();
        let (width, height) = image.dimensions();
        let rgba = image.into_raw();
        (rgba, width, height)
    };

    egui::IconData {
        rgba: icon_rgba,
        width: icon_width,
        height: icon_height,
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let args: Vec<String> = env::args().collect();

    let config = match Config::read(PathBuf::from(".\\Config.ini")) {
        Ok(c) => c,
        Err(e) => {
            error!("{e}");
            std::process::exit(0)
        }
    };

    // Tiberius configuartion:
    let mut tib_config = tiberius::Config::new();
    tib_config.host(config.get_server());
    tib_config.authentication(tiberius::AuthMethod::sql_server(
        config.get_username(),
        config.get_password(),
    ));
    tib_config.trust_cert();

    // Connect to the DB:
    let mut client_tmp = connect(tib_config.clone()).await;
    let mut tries = 0;
    while client_tmp.is_err() && tries < 3 {
        client_tmp = connect(tib_config.clone()).await;
        tries += 1;
    }

    if client_tmp.is_err() {
        error!("ER: Connection to DB failed!");
        return Ok(());
    }
    let mut client = client_tmp?;

    // USE [DB]
    let qtext = format!("USE [{}]", config.get_database());
    let query = Query::new(qtext);
    query.execute(&mut client).await?;



    let log_reader = config.get_log_reader().to_string();
    let starting_serial = if args.len() >= 2 {
        args[1].clone()
    } else {
        String::new()
    };

    let station = config.get_station_name().to_string();
    let window_height = if station == "FW" { 450.0 } else { 250.0 };

    // Start egui
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(egui::Vec2 { x: 550.0, y: window_height })
            .with_icon(load_icon()),
        ..Default::default()
    };

    _ = eframe::run_native(
        format!("ICT Query (v{VERSION})").as_str(),
        options,
        Box::new(|_| {
            Ok(Box::new(IctResultApp::default(
                client,
                station,
                log_reader,
                starting_serial,
            )))
        }),
    );

    Ok(())
}

async fn connect(tib_config: tiberius::Config) -> anyhow::Result<Client<Compat<TcpStream>>> {
    let tcp = TcpStream::connect(tib_config.get_addr()).await?;
    tcp.set_nodelay(true)?;
    let client = Client::connect(tib_config, tcp.compat_write()).await?;

    Ok(client)
}

struct Panel {
    selected: String,
    selected_pos: usize,
    boards: Vec<Board>,
}

#[derive(Debug, PartialEq)]
enum PanelResult {
    Ok,
    Nok(String),
    Warning(String),
    None
}

impl Panel {
    fn new() -> Panel {
        Panel {
            selected: String::new(),
            selected_pos: 0,
            boards: Vec::new(),
        }
    }

    fn set_selected(&mut self, sel: &str) {
        self.selected = sel.to_string();
    }

    fn set_selected_pos(&mut self, sel: usize) {
        if sel < self.boards.len() {
            self.selected_pos = sel;
            self.selected = self.boards[sel].Serial_NMBR.clone();
        }
    }

    fn is_selected_ok(&self, station: &str) -> PanelResult {
        if !self.boards.is_empty() && station == "FW" {
            if let Some(board) = self.boards.get(self.selected_pos) {
                let ict_res = board.get_ict_result();
                let aoi_res = board.get_aoi_result();

                if aoi_res.0.is_some_and( |f | !f ) || aoi_res.1.is_some_and( |f | !f )  {
                    return PanelResult::Nok("Bukott AOI-on!".to_string());
                } else if ict_res.is_none() {
                    return PanelResult::Nok("Nincs ICT eredménye!".to_string());
                } if ict_res.is_some_and(|f| !f) {
                    return PanelResult::Nok("Bukott ICT-n!".to_string());
                } else if aoi_res.0.is_none() || aoi_res.1.is_none() {
                    return PanelResult::Warning("Nincs AOI eredménye!".to_string());
                } else {
                    return PanelResult::Ok;
                }
            }
        }


        PanelResult::None
    } 

    fn selected_pos(&self, i: usize) -> bool {
        self.selected_pos == i
    }

    fn clear(&mut self) {
        self.boards.clear();
    }

    fn is_empty(&self) -> bool {
        self.boards.is_empty()
    }

    fn get_logs(&self) -> Vec<Vec<&str>> {
        let mut ret = vec![Vec::new(); self.boards[0].results.len()];

        for board in self.boards.iter() {
            for (i, result) in board.results.iter().enumerate() {
                if !result.Log_File_Name.is_empty() {
                    ret[i].push(result.Log_File_Name.as_str());
                }
            }
        }

        ret
    }

    fn get_main_serial(&self) -> &str {
        if let Some(b) = self.boards.first() {
            b.Serial_NMBR.as_str()
        } else {
            "error"
        }
    }

    fn push(
        &mut self,
        Serial_NMBR: String,
        Station: String,
        Result: bool,
        Date_Time: NaiveDateTime,
        Log_File_Name: String,
        Notes: String,
    ) {
        if let Some(board) = self
            .boards
            .iter_mut()
            .find(|f| f.Serial_NMBR == Serial_NMBR)
        {
            board.push(Station, Result, Date_Time, Log_File_Name, Notes)
        } else {
            self.boards.push(Board::new(
                Serial_NMBR,
                Station,
                Result,
                Date_Time,
                Log_File_Name,
                Notes,
            ));
        }
    }

    fn sort(&mut self) {
        self.boards
            .sort_by(|a, b| a.Serial_NMBR.cmp(&b.Serial_NMBR));

        self.selected_pos = self
            .boards
            .iter()
            .position(|f| f.Serial_NMBR == self.selected)
            .unwrap_or_default();

        for board in &mut self.boards {
            board.sort();
        }
    }

    fn get_selected_board(&self) -> Option<&Board> {
        if let Some(board) = self.boards.get(self.selected_pos) {
            Some(board)
        } else {
            None
        }
    }

    fn get_tests(&self) -> Vec<Test> {
        let mut ret: Vec<Test> = Vec::new();

        for (i, board) in self.boards.iter().enumerate() {
            for result in &board.results {
                // For backwards compatibility, we don't except the Date_Times to match.
                // 10s seems to be a sensible threshold, but might have to change it.
                if let Some(r) = ret
                    .iter_mut()
                    .find(|f| (f.Date_Time - result.Date_Time).abs() < TimeDelta::seconds(10))
                {
                    r.results[i] = Some(result);
                } else {
                    ret.push(Test {
                        Date_Time: result.Date_Time,
                        Station: result.Station.clone(),
                        results: {
                            let mut res = vec![None; self.boards.len()];
                            res[i] = Some(result);
                            res
                        },
                    });
                }
            }
        }

        ret.sort_by_key(|f| f.Date_Time);
        ret.reverse();

        ret
    }
}

struct Test<'a> {
    Date_Time: NaiveDateTime,
    Station: String,
    results: Vec<Option<&'a TestResult>>,
}

struct Board {
    Serial_NMBR: String,
    results: Vec<TestResult>,
}

impl Board {
    fn new(
        Serial_NMBR: String,
        Station: String,
        Result: bool,
        Date_Time: NaiveDateTime,
        Log_File_Name: String,
        Notes: String,
    ) -> Board {
        let results = vec![TestResult {
            Station,
            Result,
            Date_Time,
            Log_File_Name,
            Notes,
        }];
        Board {
            Serial_NMBR,
            results,
        }
    }

    fn get_ict_result(&self) -> Option<bool> {
        for result in &self.results {
            if result.Station.starts_with("ICT") {
                return Some(result.Result)
            }
        }

        None
    }

    fn get_aoi_result(&self) -> (Option<bool>, Option<bool>) {
        let mut top = None;
        let mut bot = None;

        for result in &self.results {
            if result.Station.starts_with("AOI") && result.Station.starts_with("HARAN") {
                if result.Station.ends_with("TOP") && top.is_none() {
                    top = Some(result.Result);
                } else if result.Station.ends_with("BOT") && bot.is_none() {
                    bot = Some(result.Result);
                }
            }
        }

        (bot, top)
    }

    fn push(
        &mut self,
        Station: String,
        Result: bool,
        Date_Time: NaiveDateTime,
        Log_File_Name: String,
        Notes: String,
    ) {
        self.results.push(TestResult {
            Station,
            Result,
            Date_Time,
            Log_File_Name,
            Notes,
        });
    }

    fn sort(&mut self) {
        self.results.sort_by_key(|f| f.Date_Time);
        self.results.reverse();
    }
}

struct TestResult {
    Station: String,
    Result: bool,
    Date_Time: NaiveDateTime,
    Log_File_Name: String,
    Notes: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum AppMode {
    Board,
    Panel,
}

struct IctResultApp {
    client: Arc<tokio::sync::Mutex<Client<Compat<TcpStream>>>>,
    station: String,
    mode: AppMode,
    loading: Arc<Mutex<bool>>,
    log_viewer: String,

    panel: Arc<Mutex<Panel>>,
    error_message: Arc<Mutex<Option<String>>>,

    DMC_input: String,
    scan_instantly: bool,

    scan: ScanForLogs,
}

impl IctResultApp {
    fn default(client: Client<Compat<TcpStream>>, station: String, log_viewer: String, DMC_input: String) -> Self {
        let scan_instantly = !DMC_input.is_empty();

        IctResultApp {
            client: Arc::new(tokio::sync::Mutex::new(client)),
            station,
            mode: AppMode::Board,
            loading: Arc::new(Mutex::new(false)),
            log_viewer,
            panel: Arc::new(Mutex::new(Panel::new())),
            error_message: Arc::new(Mutex::new(None)),
            DMC_input,
            scan_instantly,
            scan: ScanForLogs::default(),
        }
    }

    fn open_log(&self, log: &str) {
        if log.is_empty() {
            return;
        }

        debug!("Trying to open log: {log}");
        if let Some(path) = search_for_log(log) {
            let res = std::process::Command::new(&self.log_viewer)
                .arg(path)
                .spawn();
            debug!("{:?}", res);
        } else {
            error!("Log not found!");
        }
    }

    fn query(&mut self, context: egui::Context) {
        debug!("Query DMC: {}", self.DMC_input);

        let mut DMC = self.DMC_input.clone();

        // Fix for DCDC.
        // The two sides have different DMC, but only the BOT side is saved in the DB.
        // If we get a TOP side DMC, then we will ad a 'B', turning it into the coresponding BOT side DMC.

        if DMC.starts_with('!')
            && DMC.len() > ICT_config::DMC_MIN_LENGTH
            && DMC[11..].starts_with("V664653")
        {
            let (start, end) = DMC.split_at(11);
            DMC = format!("{}B{}", start, end);
        }

        self.panel.lock().unwrap().clear();

        let panel_lock = self.panel.clone();
        let client_lock = self.client.clone();
        let error_clone = self.error_message.clone();
        let loading_lock = self.loading.clone();

        tokio::spawn(async move {
            let mut c = client_lock.lock().await;

            // 1 - Get Log_File_Name only.

            let mut query = Query::new(
                "SELECT TOP(1) Log_File_Name
                FROM SMT_Test 
                WHERE Serial_NMBR = @P1",
            );
            query.bind(&DMC);

            let mut logname: String = String::new();
            if let Ok(result) = query.query(&mut c).await {
                if let Some(row) = result.into_row().await.unwrap() {
                    logname = row.get::<&str, usize>(0).unwrap().to_string();
                } else {
                    // No result found for the DMC
                    *error_clone.lock().unwrap() =
                        Some(format!("Nem található eredmény a DMC-re: {}", DMC));
                }
            } else {
                // SQL error
                *error_clone.lock().unwrap() = Some("SQL hiba lekérdezés közben!".to_string());
                *loading_lock.lock().unwrap() = false;
                context.request_repaint();
                return;
            }

            panel_lock.lock().unwrap().set_selected(&DMC);

            //  2 - We use it to determine the board number on the panel.
            //  And with the board number and the DMC we can generate all the serials on the panel.

            let serials: Vec<String> = if logname.is_empty() {
                vec![DMC]
            } else if let Some(product) =
                ICT_config::get_product_for_serial(ICT_config::PRODUCT_LIST, &DMC)
            {
                debug!("Product is: {}", product.get_name());
                if let Some(pos) = product.get_pos_from_logname(&logname) {
                    debug!("Position is: {pos} (using base 0)");
                    ICT_config::generate_serials(&DMC, pos, product.get_bop())
                } else {
                    vec![DMC]
                }
            } else {
                vec![DMC]
            };

            // 3 - Query the serials.
            let serial_string = serials
                .iter()
                .map(|f| format!("'{f}'"))
                .collect::<Vec<String>>()
                .join(", ");

            debug!("Serials: {:?}", serials);
            let qtext = format!(
                "SELECT Serial_NMBR, Station, Result, Date_Time, Log_File_Name, Notes
                FROM SMT_Test 
                WHERE Serial_NMBR IN ( {} );",
                serial_string
            );

            debug!("Query: {qtext}");
            if let Ok(mut result) = c.query(qtext, &[]).await {
                while let Some(row) = result.next().await {
                    let row = row.unwrap();
                    match row {
                        tiberius::QueryItem::Row(x) => {
                            // [Serial_NMBR],[Station],[Result],[Date_Time],[Log_File_Name],[Notes]
                            let serial = x.get::<&str, usize>(0).unwrap().to_owned();
                            let station = x.get::<&str, usize>(1).unwrap().to_owned();
                            let result = x.get::<&str, usize>(2).unwrap().to_owned();
                            let date_time = x.get::<NaiveDateTime, usize>(3).unwrap();
                            let log_file_name = x.get::<&str, usize>(4).unwrap().to_owned();
                            let note = x.get::<&str, usize>(5).unwrap_or_default().to_owned(); // Notes can be NULL!

                            panel_lock.lock().unwrap().push(
                                serial,
                                station,
                                result == "Passed",
                                date_time,
                                log_file_name,
                                note,
                            );
                        }
                        tiberius::QueryItem::Metadata(_) => (),
                    }
                }
            } else {
                // SQL error
                *error_clone.lock().unwrap() =
                    Some("SQL hiba lekérdezés közben! (ICT)".to_string());
                *loading_lock.lock().unwrap() = false;
                context.request_repaint();
                return;
            }

            // 4 - Query the serials for CCL results

            let qtext = format!(
                "SELECT Barcode, Result, Side, Line, Operator, RowUpdated
                FROM AOI_RESULTS 
                WHERE Barcode IN ( {} );",
                serial_string
            );

            debug!("Query: {qtext}");
            if let Ok(mut result) = c.query(qtext, &[]).await {
                while let Some(row) = result.next().await {
                    let row = row.unwrap();
                    match row {
                        tiberius::QueryItem::Row(x) => {
                            //  Barcode, Result, Side, Line, Operator, RowUpdated
                            let serial = x.get::<&str, usize>(0).unwrap().to_owned();
                            let result = x.get::<&str, usize>(1).unwrap().to_owned();
                            let side = x.get::<&str, usize>(2).unwrap().to_owned();
                            let station = x.get::<&str, usize>(3).unwrap().to_owned();
                            let operator = x.get::<&str, usize>(4).unwrap_or_default().to_owned();
                            let date_time = x.get::<NaiveDateTime, usize>(5).unwrap();

                            let station_str = if station == "LINE1" {
                                format!("CCL {}", side)
                            } else {
                                format!("CCL-FW {}", side)
                            };

                            panel_lock.lock().unwrap().push(
                                serial,
                                station_str,
                                result == "PASS",
                                date_time,
                                "".to_string(),
                                format!("Operator: {operator}"),
                            );
                        }
                        tiberius::QueryItem::Metadata(_) => (),
                    }
                }
            } else {
                // SQL error
                *error_clone.lock().unwrap() =
                    Some("SQL hiba lekérdezés közben! (CCL)".to_string());

                panel_lock.lock().unwrap().sort();
                *loading_lock.lock().unwrap() = false;
                context.request_repaint();
                return;
            }

            // 5 - Query for FCT results
            let qtext = format!(
                "SELECT Serial_NMBR, Station, Result, Date_Time, Notes
                FROM SMT_FCT_Test 
                WHERE Serial_NMBR IN ( {} );",
                serial_string
            );

            debug!("Query: {qtext}");
            if let Ok(mut result) = c.query(qtext, &[]).await {
                while let Some(row) = result.next().await {
                    let row = row.unwrap();
                    match row {
                        tiberius::QueryItem::Row(x) => {
                            // [Serial_NMBR],[Station],[Result],[Date_Time],[Notes]
                            let serial = x.get::<&str, usize>(0).unwrap().to_owned();
                            let station = x.get::<&str, usize>(1).unwrap().to_owned();
                            let result = x.get::<&str, usize>(2).unwrap().to_owned();
                            let date_time = x.get::<NaiveDateTime, usize>(3).unwrap();
                            let note = x.get::<&str, usize>(4).unwrap_or_default().to_owned(); // Notes can be NULL!

                            panel_lock.lock().unwrap().push(
                                serial,
                                station,
                                result == "Passed",
                                date_time,
                                String::new(),
                                note,
                            );
                        }
                        tiberius::QueryItem::Metadata(_) => (),
                    }
                }
            }

            // 5 - Query for AOI results
            let qtext = format!(
                "SELECT Serial_NMBR, Station, Result, Date_Time, Program, Operator
                FROM SMT_AOI_RESULTS 
                WHERE Serial_NMBR IN ( {} );",
                serial_string
            );

            debug!("AOI query: {qtext}");
            if let Ok(mut result) = c.query(qtext, &[]).await {
                while let Some(row) = result.next().await {
                    let row = row.unwrap();
                    match row {
                        tiberius::QueryItem::Row(x) => {
                            // [Serial_NMBR],[Station],[Result],[Date_Time],[Failed],[Pseudo_error]
                            let serial = x.get::<&str, usize>(0).unwrap().to_owned();
                            let station = x.get::<&str, usize>(1).unwrap().to_owned();
                            let result = x.get::<&str, usize>(2).unwrap().to_owned();
                            let date_time = x.get::<NaiveDateTime, usize>(3).unwrap();
                            let program = x.get::<&str, usize>(4).unwrap_or_default().to_owned();
                            let operator = x.get::<&str, usize>(5).unwrap_or_default().to_owned();

                            if station.len() > 3 && program.len() > 3 {
                                let sub_station = if operator.is_empty() {
                                    "AOI/AXI"
                                } else {
                                    "HARAN"
                                };

                                let side = program[program.len()-3..].to_string();
                                let line_number = &station[station.len()-2..];


                                let station = format!("{sub_station} - L{line_number} - {side}");
                                


                                panel_lock.lock().unwrap().push(
                                    serial,
                                    station,
                                    result == "Pass",
                                    date_time,
                                    String::new(),
                                    program,
                                );
                            }
                        }
                        tiberius::QueryItem::Metadata(_) => (),
                    }
                }
            }

            panel_lock.lock().unwrap().sort();
            *loading_lock.lock().unwrap() = false;
            context.request_repaint();
        });
    }
}

impl eframe::App for IctResultApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("SNBR").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.monospace("DMC:");

                let mut text_edit = egui::TextEdit::singleline(&mut self.DMC_input)
                    .desired_width(300.0)
                    .show(ui);

                let ok_button = ui.add(egui::Button::new("OK"));

                if ui.button("Logok mentése").clicked() && !self.panel.lock().unwrap().is_empty() {
                    self.scan.clear();

                    for result in self.panel.lock().unwrap().get_logs() {
                        self.scan.push(result);
                    }

                    self.scan
                        .set_selected(self.panel.lock().unwrap().selected_pos);
                    self.scan.enable();
                }

                if (self.scan_instantly
                    || ok_button.clicked()
                    || (text_edit.response.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))))
                    && !*self.loading.lock().unwrap()
                {
                    *self.loading.lock().unwrap() = true;
                    self.scan_instantly = false;

                    let new_range = egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        egui::text::CCursor::new(self.DMC_input.len()),
                    );
                    text_edit.response.request_focus();
                    text_edit.state.cursor.set_char_range(Some(new_range));
                    text_edit.state.store(ui.ctx(), text_edit.response.id);

                    let context = ctx.clone();

                    self.query(context);
                }

                text_edit.response.request_focus();
            });

            ui.horizontal_centered(|ui| {
                ui.monospace("Nézet: ");
                ui.selectable_value(&mut self.mode, AppMode::Board, "Board");
                ui.selectable_value(&mut self.mode, AppMode::Panel, "Panel");
            });
        });

        if self.station == "FW" && self.mode == AppMode::Board {
            egui::TopBottomPanel::bottom("FW_bot_panel").exact_height(200.0).show(ctx, |ui| {
                match self.panel.lock().unwrap().is_selected_ok(&self.station) {
                    PanelResult::Ok => {
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new("OK").color(Color32::GREEN).size(100.0));
                        } );
                    },
                    PanelResult::Warning(message) => {
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new("NOK").color(Color32::ORANGE).size(100.0));
                            ui.label(RichText::new(&message).color(Color32::ORANGE).size(30.0));
                        } );
                    },
                    PanelResult::Nok(message) => {
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new("NOK").color(Color32::RED).size(100.0));
                            ui.label(RichText::new(&message).color(Color32::RED).size(30.0));
                        } );
                    },
                    PanelResult::None => {},
                } 
            });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            let mut panel_lock = self.panel.lock().unwrap();
            let mut switch_selected: Option<usize> = None;

            if !panel_lock.is_empty() {
                match self.mode {
                    AppMode::Board => {
                        if let Some(board) = panel_lock.get_selected_board() {
                            TableBuilder::new(ui)
                                .striped(true)
                                .column(Column::initial(30.0).resizable(true))
                                .column(Column::initial(80.0).resizable(true)) // Result
                                .column(Column::initial(150.0).resizable(true)) // Station
                                .column(Column::initial(110.0).resizable(true)) // Time
                                .column(Column::remainder().resizable(true)) // Notes
                                .header(20.0, |mut header| {
                                    header.col(|ui| {
                                        ui.centered_and_justified(|ui| {
                                            ui.label("#");
                                        });
                                    });
                                    header.col(|ui| {
                                        ui.centered_and_justified(|ui| {
                                            ui.label("Eredmény");
                                        });
                                    });
                                    header.col(|ui| {
                                        ui.centered_and_justified(|ui| {
                                            ui.label("Állomás");
                                        });
                                    });
                                    header.col(|ui| {
                                        ui.centered_and_justified(|ui| {
                                            ui.label("Időpont");
                                        });
                                    });
                                    header.col(|ui| {
                                        ui.centered_and_justified(|ui| {
                                            ui.label("Megjegyzések");
                                        });
                                    });
                                })
                                .body(|mut body| {
                                    for (x, result) in board.results.iter().enumerate() {
                                        body.row(14.0, |mut row| {
                                            row.col(|ui| {
                                                ui.centered_and_justified(|ui| {
                                                    ui.label(format!("{}", x + 1));
                                                });
                                            });
                                            row.col(|ui| {
                                                let response = draw_result_text(ui, result.Result);

                                                if response.clicked() {
                                                    self.open_log(&result.Log_File_Name);
                                                }
                                            });
                                            row.col(|ui| {
                                                ui.centered_and_justified(|ui| {
                                                    ui.label(&result.Station);
                                                });
                                            });
                                            row.col(|ui| {
                                                ui.centered_and_justified(|ui| {
                                                    ui.label(format!(
                                                        "{}",
                                                        result.Date_Time.format("%Y-%m-%d %H:%M")
                                                    ));
                                                });
                                            });
                                            row.col(|ui| {
                                                ui.add(egui::Label::new(&result.Notes).truncate());
                                            });
                                        });
                                    }
                                });
                        } else {
                            ui.centered_and_justified(|ui| {
                                ui.label("Belső hiba");
                            });
                        }
                    }
                    AppMode::Panel => {
                        ui.label(format!("Fő DMC: {}", panel_lock.get_main_serial()));
                        ui.separator();

                        TableBuilder::new(ui)
                            .striped(true)
                            .column(Column::initial(30.0).resizable(true))
                            .column(Column::initial(250.0).resizable(true)) // Result
                            .column(Column::initial(100.0).resizable(true)) // Station
                            .column(Column::remainder().resizable(true)) // Time
                            .header(20.0, |mut header| {
                                header.col(|ui| {
                                    ui.centered_and_justified(|ui| {
                                        ui.label("#");
                                    });
                                });
                                header.col(|ui| {
                                    ui.centered_and_justified(|ui| {
                                        ui.label("Eredmények");
                                    });
                                });
                                header.col(|ui| {
                                    ui.centered_and_justified(|ui| {
                                        ui.label("Állomás");
                                    });
                                });
                                header.col(|ui| {
                                    ui.centered_and_justified(|ui| {
                                        ui.label("Időpont");
                                    });
                                });
                            })
                            .body(|mut body| {
                                for (x, result) in panel_lock.get_tests().iter().enumerate() {
                                    body.row(14.0, |mut row| {
                                        row.col(|ui| {
                                            ui.centered_and_justified(|ui| {
                                                ui.label(format!("{}", x + 1));
                                            });
                                        });
                                        row.col(|ui| {
                                            ui.spacing_mut().interact_size = Vec2::new(0.0, 0.0);
                                            ui.spacing_mut().item_spacing = Vec2::new(3.0, 3.0);

                                            ui.horizontal(|ui| {
                                                for (i, board) in result.results.iter().enumerate()
                                                {
                                                    if let Some(res) = board {
                                                        let response = draw_result_box(
                                                            ui,
                                                            res.Result,
                                                            panel_lock.selected_pos(i),
                                                        );

                                                        if response.clicked() {
                                                            self.open_log(&res.Log_File_Name);
                                                        } else if response.clicked_by(
                                                            egui::PointerButton::Secondary,
                                                        ) {
                                                            switch_selected = Some(i);
                                                        }

                                                        if !res.Notes.is_empty() {
                                                            response.on_hover_text(&res.Notes);
                                                        }
                                                    } else {
                                                        ui.add_space(13.0);
                                                    }
                                                }
                                            });
                                        });
                                        row.col(|ui| {
                                            ui.centered_and_justified(|ui| {
                                                ui.label(&result.Station);
                                            });
                                        });
                                        row.col(|ui| {
                                            ui.centered_and_justified(|ui| {
                                                ui.label(format!(
                                                    "{}",
                                                    result.Date_Time.format("%Y-%m-%d %H:%M")
                                                ));
                                            });
                                        });
                                    });
                                }
                            });
                    }
                }
            } else if let Some(message) = self.error_message.lock().unwrap().as_ref() {
                ui.centered_and_justified(|ui| {
                    ui.label(message);
                });
            }

            if let Some(i) = switch_selected {
                panel_lock.set_selected_pos(i);
            }
        });

        if self.scan.enabled() {
            self.scan.update(ctx);
        }
    }
}

fn draw_result_box(ui: &mut egui::Ui, result: bool, highlight: bool) -> egui::Response {
    let desired_size = egui::vec2(10.0, 10.0);

    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());

    let rect = if highlight { rect.expand(2.0) } else { rect };

    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        let rect = rect.expand(visuals.expansion);
        ui.painter().rect_filled(
            rect,
            2.0,
            if result {
                egui::Color32::GREEN
            } else {
                egui::Color32::RED
            },
        );
    }

    response
}

fn draw_result_text(ui: &mut egui::Ui, result: bool) -> egui::Response {
    let desired_size = ui.available_size();
    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());

    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(
            rect,
            2.0,
            if result {
                egui::Color32::DARK_GREEN
            } else {
                egui::Color32::DARK_RED
            },
        );

        return ui.put(
            rect,
            egui::Label::new(
                egui::RichText::new(match result {
                    true => "OK",
                    false => "NOK",
                })
                .color(egui::Color32::WHITE),
            )
            .sense(egui::Sense::click()),
        );
    }

    response
}
