#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release
#![allow(non_snake_case)]
#![allow(clippy::too_many_arguments)]

use std::{
    env,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use chrono::{NaiveDateTime, TimeDelta};
use egui::Vec2;
use egui_extras::{Column, TableBuilder};
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
            println!("{e}");
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
        println!("ER: Connection to DB failed!");
        return Ok(());
    }
    let mut client = client_tmp?;

    // USE [DB]
    let qtext = format!("USE [{}]", config.get_database());
    let query = Query::new(qtext);
    query.execute(&mut client).await?;

    // Start egui
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(egui::Vec2 { x: 550.0, y: 250.0 })
            .with_icon(load_icon()),
        ..Default::default()
    };

    let log_reader = config.get_log_reader().to_string();
    let starting_serial = if args.len() >= 2 {
        args[1].clone()
    } else {
        String::new()
    };

    _ = eframe::run_native(
        format!("ICT Query (v{VERSION})").as_str(),
        options,
        Box::new(|_| Box::new(IctResultApp::default(client, log_reader, starting_serial))),
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

        for board in &self.boards {
            for result in &board.results {
                // For backwards compatibility, we don't except the Date_Times to match.
                // 10s seems to be a sensible threshold, but might have to change it.
                if let Some(r) = ret
                    .iter_mut()
                    .find(|f| (f.Date_Time - result.Date_Time).abs() < TimeDelta::seconds(10))
                {
                    r.results.push(result);
                } else {
                    ret.push(Test {
                        Date_Time: result.Date_Time,
                        Station: result.Station.clone(),
                        results: vec![result],
                    });
                }
            }
        }

        ret
    }
}

struct Test<'a> {
    Date_Time: NaiveDateTime,
    Station: String,
    results: Vec<&'a TestResult>,
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
    mode: AppMode,
    log_viewer: String,

    panel: Arc<Mutex<Panel>>,
    error_message: Arc<Mutex<Option<String>>>,

    DMC_input: String,
    scan_instantly: bool,

    scan: ScanForLogs,
}

impl IctResultApp {
    fn default(client: Client<Compat<TcpStream>>, log_viewer: String, DMC_input: String) -> Self {
        let scan_instantly = !DMC_input.is_empty();

        IctResultApp {
            client: Arc::new(tokio::sync::Mutex::new(client)),
            mode: AppMode::Board,
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

        println!("Trying to open log: {log}");
        if let Some(path) = search_for_log(log) {
            let res = std::process::Command::new(&self.log_viewer)
                .arg(path)
                .spawn();
            println!("{:?}", res);
        } else {
            println!("Log not found!");
        }
    }

    fn query(&mut self, context: egui::Context) {
        println!("Query DMC: {}", self.DMC_input);

        let DMC = self.DMC_input.clone();

        self.panel.lock().unwrap().clear();

        let panel_lock = self.panel.clone();
        let client_lock = self.client.clone();
        let error_clone = self.error_message.clone();

        tokio::spawn(async move {
            let mut c = client_lock.lock().await;

            // 1 - Get Log_File_Name only.

            let mut query = Query::new(
                "SELECT TOP(1) Log_File_Name
                FROM SMT_Test 
                WHERE Serial_NMBR = @P1",
            );
            query.bind(&DMC);

            let logname: String;
            if let Ok(result) = query.query(&mut c).await {
                if let Some(row) = result.into_row().await.unwrap() {
                    logname = row.get::<&str, usize>(0).unwrap().to_string();
                } else {
                    // No result found for the DMC
                    *error_clone.lock().unwrap() =
                        Some(format!("Nem található eredmény a DMC-re: {}", DMC));
                    return;
                }
            } else {
                // SQL error
                *error_clone.lock().unwrap() = Some("SQL hiba lekérdezés közben!".to_string());
                return;
            }

            println!("Logname: {logname}");
            panel_lock.lock().unwrap().set_selected(&DMC);

            //  2 - We use it to determine the board number on the panel.
            //  And with the board number and the DMC we can generate all the serials on the panel.

            let serials: Vec<String> = if let Some(product) =
                ICT_config::get_product_for_serial(ICT_config::PRODUCT_LIST, &DMC)
            {
                println!("Product is: {}", product.get_name());
                if let Some(pos) = product.get_pos_from_logname(&logname) {
                    println!("Position is: {pos} (using base 0)");
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

            println!("Serials: {:?}", serials);
            let qtext = format!(
                "SELECT Serial_NMBR, Station, Result, Date_Time, Log_File_Name, Notes
                FROM SMT_Test 
                WHERE Serial_NMBR IN ( {} );",
                serial_string
            );

            println!("Query: {qtext}");
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
                return;
            }

            // 4 - Query the serials for CCL results

            println!("Serials: {:?}", serials);
            let qtext = format!(
                "SELECT Barcode, Result, Side, Line, Operator, RowUpdated
                FROM AOI_RESULTS 
                WHERE Barcode IN ( {} );",
                serial_string
            );

            println!("Query: {qtext}");
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
                return;
            }

            panel_lock.lock().unwrap().sort();
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
                    && self.DMC_input.len() > 15
                {
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
            });

            ui.horizontal_centered(|ui| {
                ui.monospace("Nézet: ");
                ui.selectable_value(&mut self.mode, AppMode::Board, "Board");
                ui.selectable_value(&mut self.mode, AppMode::Panel, "Panel");
            });
        });

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
                                .column(Column::initial(100.0).resizable(true)) // Station
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
                                                ui.add(
                                                    egui::Label::new(&result.Notes).truncate(true),
                                                );
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
                                                    let response = draw_result_box(
                                                        ui,
                                                        board.Result,
                                                        panel_lock.selected_pos(i),
                                                    );

                                                    if response.clicked() {
                                                        self.open_log(&board.Log_File_Name);
                                                    } else if response
                                                        .clicked_by(egui::PointerButton::Secondary)
                                                    {
                                                        switch_selected = Some(i);
                                                    }

                                                    if !board.Notes.is_empty() {
                                                        response.on_hover_text(&board.Notes);
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
