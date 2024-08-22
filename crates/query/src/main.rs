#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release
#![allow(non_snake_case)]

use std::{
    path::PathBuf, sync::{Arc, Mutex}
};

use chrono::NaiveDateTime;
use egui::Vec2;
use egui_extras::{Column, TableBuilder};
use tiberius::{Client, Query};
use tokio::net::TcpStream;
use tokio_stream::StreamExt;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

use ICT_config::*;

mod scan_for_logs;
use scan_for_logs::*;

const PRODUCT_LIST: &str = ".\\products";
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

    _ = eframe::run_native(
        format!("ICT Query (v{VERSION})").as_str(),
        options,
        Box::new(|_| Box::new(IctResultApp::default(client, log_reader))),
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
    boards: u8,
    product: String,
    selected_pos: u8,
    serials: Vec<String>,
    results: Vec<PanelResult>,
}

impl Panel {
    fn empty() -> Self {
        Panel {
            boards: 0,
            product: String::new(),
            selected_pos: 0,
            serials: Vec::new(),
            results: Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.serials.is_empty()
    }

    fn new(product: &Product) -> Self {
        Panel {
            boards: product.get_bop(),
            product: product.get_name().to_string(),
            selected_pos: 0,
            serials: Vec::new(),
            results: Vec::new(),
        }
    }

    fn push(
        &mut self,
        position: u8,
        serial: String,
        station: String,
        result: String,
        date_time: NaiveDateTime,
        log_file_name: String,
    ) {
        if self.serials.is_empty() {
            self.serials = generate_serials(serial, position, self.boards);
            self.selected_pos = position;
            println!("Serials: {:#?}", self.serials);
        }

        let mut results = vec![BoardResult::Unknown; self.boards as usize];
        results[position as usize] = if result == "Passed" {
            BoardResult::Passed
        } else {
            BoardResult::Failed
        };

        let mut logs = vec![String::new(); self.boards as usize];
        logs[position as usize] = log_file_name;

        self.results.push(PanelResult {
            time: date_time,
            station,
            results,
            logs,
        })
    }

    fn add_result(&mut self, i: u8, result: String, log: String) {
        let res = if result == "Passed" {
            BoardResult::Passed
        } else {
            BoardResult::Failed
        };

        for x in self.results.iter_mut() {
            if x.results[i as usize] == BoardResult::Unknown {
                x.results[i as usize] = res;
                x.logs[i as usize] = log;
                break;
            }
        }
    }
}

struct PanelResult {
    time: NaiveDateTime,
    station: String,
    results: Vec<BoardResult>,
    logs: Vec<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum BoardResult {
    Passed,
    Failed,
    Unknown,
}

impl BoardResult {
    pub fn into_color(self) -> egui::Color32 {
        match self {
            BoardResult::Passed => egui::Color32::GREEN,
            BoardResult::Failed => egui::Color32::RED,
            BoardResult::Unknown => egui::Color32::YELLOW,
        }
    }
}

struct IctResultApp {
    client: Arc<tokio::sync::Mutex<Client<Compat<TcpStream>>>>,
    log_viewer: String,

    products: Vec<Product>,
    panel: Arc<Mutex<Panel>>,

    DMC_input: String,
    scan: ScanForLogs
}

impl IctResultApp {
    fn default(client: Client<Compat<TcpStream>>, log_viewer: String) -> Self {
        IctResultApp {
            client: Arc::new(tokio::sync::Mutex::new(client)),
            log_viewer,
            products: load_product_list(PRODUCT_LIST, true),
            panel: Arc::new(Mutex::new(Panel::empty())),
            DMC_input: String::new(),
            scan: ScanForLogs::default()
        }
    }

    fn open_log(&self, log: &str) {
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
}

impl eframe::App for IctResultApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("SNBR").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.monospace("DMC:");

                let mut text_edit = egui::TextEdit::singleline(&mut self.DMC_input).desired_width(300.0).show(ui);

                let ok_button = ui.add(egui::Button::new("OK"));

                if ui.button("Logok mentése").clicked() && !self.panel.lock().unwrap().is_empty() {
                    self.scan.clear();

                    for result in &self.panel.lock().unwrap().results {
                        self.scan.push(result.logs.iter().map(|f| f.as_str()).collect::<Vec<&str>>());
                    }

                    self.scan.set_selected(self.panel.lock().unwrap().selected_pos);
                    self.scan.enable();
                }

                if ( ok_button.clicked() || (text_edit.response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))) && self.DMC_input.len() > 15 {
                    println!("Query DMC: {}", self.DMC_input);
                    let DMC = self.DMC_input.clone();

                    let new_range = 
                    egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(DMC.len()));
                    text_edit.response.request_focus();
                    text_edit.state.cursor.set_char_range(Some(new_range));
                    text_edit.state.store(ui.ctx(), text_edit.response.id);

                    // Identify product type
                    println!("Product id: {}", &DMC[13..]);

                    let product = 'prod: {
                        for p in &self.products {
                            println!("{:?}", p);
                            if p.check_serial(&DMC) {
                                println!("Product is: {}", p.get_name());
                                break 'prod p.clone()                          
                            }
                        };

                        Product::unknown()
                    };                    
                    

                    self.panel = Arc::new(Mutex::new(Panel::new(&product)));

                    // 1 - query to given DMC
                    // 2 - from Log_file_name get the board position
                    // 3 - push result to panel to the given position
                    // 4 - calculate the rest of the serials
                    // 5 - query the remaining serials

                    let panel_lock = self.panel.clone();
                    let client_lock = self.client.clone();
                    let context = ctx.clone();                    

                    tokio::spawn(async move {
                        let mut c = client_lock.lock().await;                        

                        let mut query =
                            Query::new(
                            "SELECT [Serial_NMBR],[Station],[Result],[Date_Time],[Log_File_Name] 
                            FROM [dbo].[SMT_Test] WHERE [Serial_NMBR] = @P1 
                            ORDER BY [Date_Time] DESC");
                        query.bind(&DMC);

                        println!("Query: {:?}", query);

                        let mut failed_query = true;
                        let mut position: u8 = 0;
                        if let Ok(mut result) = query.query(&mut c).await {
                            while let Some(row) = result.next().await {
                                let row = row.unwrap();
                                match row {
                                    tiberius::QueryItem::Row(x) => {
                                        // [Serial_NMBR],[Station],[Result],[Date_Time],[Log_File_Name] 
                                        let serial = x.get::<&str, usize>(0).unwrap().to_owned();
                                        let station = x.get::<&str, usize>(1).unwrap().to_owned();
                                        let result = x.get::<&str, usize>(2).unwrap().to_owned();
                                        let date_time = x.get::<NaiveDateTime, usize>(3).unwrap();
                                        let log_file_name = x.get::<&str, usize>(4).unwrap().to_owned();
                                        
                                         
                                        position = product.get_pos_from_logname(&log_file_name).min(product.get_bop());

                                        panel_lock.lock().unwrap().push(position, serial, station, result, date_time, log_file_name);

                                        failed_query = false;
                                    }
                                    tiberius::QueryItem::Metadata(_) => (),
                                }
                            }
                        }

                        if product.get_bop() > 1 && !failed_query {
                            for i in 0..product.get_bop() {
                                if i == position {
                                    continue;
                                }

                                let DMC = panel_lock.lock().unwrap().serials[i as usize].clone();

                                let mut query =
                                Query::new("SELECT [Result],[Log_File_Name] FROM [dbo].[SMT_Test] WHERE [Serial_NMBR] = @P1 ORDER BY [Date_Time] DESC");
                                query.bind(&DMC);
        
                                println!("Query #{i}: {:?}", query);

                                if let Ok(mut result) = query.query(&mut c).await {
                                    while let Some(row) = result.next().await {
                                        let row = row.unwrap();
                                        match row {
                                            tiberius::QueryItem::Row(x) => {
                                                // [Result], [Log_File_Name]
                                                let result = x.get::<&str, usize>(0).unwrap().to_owned();
                                                let log = x.get::<&str, usize>(1).unwrap().to_owned();
                                                print!("{}, ", result);
                                                panel_lock.lock().unwrap().add_result(i, result, log);
                                            }
                                            tiberius::QueryItem::Metadata(_) => (),
                                        }
                                    }
                                }

                                println!();
                            }
                        }

                        context.request_repaint();
                    });
                }
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            let panel_lock = self.panel.lock().unwrap();

            if !panel_lock.is_empty() {
                ui.label(format!("Termék: {}", panel_lock.product));
                ui.label(format!("Fő DMC: {}", panel_lock.serials[0]));
                ui.separator();

                TableBuilder::new(ui)
                    .striped(true)
                    .column(Column::initial(40.0).resizable(true))
                    .column(Column::initial(250.0).resizable(true)) // Result
                    .column(Column::initial(100.0).resizable(true)) // Station
                    .column(Column::initial(150.0).resizable(true)) // Time
                    .header(20.0, |mut header| {
                        header.col(|ui| {
                            ui.label("#");
                        });
                        header.col(|ui| {
                            ui.label("Eredmények");
                        });
                        header.col(|ui| {
                            ui.label("Állomás");
                        });
                        header.col(|ui| {
                            ui.label("Időpont");
                        });
                    })
                    .body(|mut body| {
                        for (x, result) in panel_lock.results.iter().enumerate() {
                            body.row(14.0, |mut row| {
                                row.col(|ui| {
                                    ui.label(format!("{}", x + 1));
                                });
                                row.col(|ui| {
                                    ui.spacing_mut().interact_size = Vec2::new(0.0, 0.0);
                                    ui.spacing_mut().item_spacing = Vec2::new(3.0, 3.0);

                                    ui.horizontal(|ui| {
                                        for (i, board) in result.results.iter().enumerate() {
                                            if draw_result_box(
                                                ui,
                                                board,
                                                i == panel_lock.selected_pos as usize,
                                            )
                                            .clicked()
                                            {
                                                self.open_log(&result.logs[i]);
                                            }
                                        }
                                    });
                                });
                                row.col(|ui| {
                                    ui.label(&result.station);
                                });
                                row.col(|ui| {
                                    ui.label(format!("{}", result.time.format("%Y-%m-%d %H:%M")));
                                });
                            });
                        }
                    });
            }
        });

        if self.scan.enabled() {
            self.scan.update(ctx);
        }
    }
}

fn draw_result_box(ui: &mut egui::Ui, result: &BoardResult, highlight: bool) -> egui::Response {
    let desired_size = egui::vec2(10.0, 10.0);

    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());

    let rect = if highlight { rect.expand(2.0) } else { rect };

    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        let rect = rect.expand(visuals.expansion);
        ui.painter().rect_filled(rect, 2.0, result.into_color());
    }

    response
}
