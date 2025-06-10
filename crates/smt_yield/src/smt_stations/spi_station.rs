use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::{
    connection::{check_connection, create_connection},
    TimeFrame,
};
use chrono::NaiveDateTime;
use egui::{style::ScrollStyle, Color32, Layout, RichText};
use egui_extras::{Column, TableBuilder};
use log::{debug, error, info};
use tiberius::{Client, Query};
use tokio::net::TcpStream;
use tokio_stream::StreamExt;
use tokio_util::compat::Compat;

use SPI_log_file::{self, helpers::FailedCompCounter};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Status {
    UnInitialized,
    Initializing,
    Standby,
    Loading,
    Error,
}

#[derive(Debug, Default)]
struct Stations {
    lines: Vec<Line>,
    products: Vec<Product>,
}

#[derive(Debug)]
struct Line {
    name: String,
    selected: bool,
}

#[derive(Debug)]
struct Product {
    name: String,
    selected: bool,
    available_by_selected_lines: bool,
    used_by_line: Vec<bool>,
}

impl Stations {
    fn push(&mut self, line: String, product: String) {
        let line = line.to_ascii_uppercase();

        let line_number = if let Some(ln) = self.lines.iter().rposition(|f| f.name == line) {
            ln
        } else {
            self.lines.push(Line {
                name: line,
                selected: false,
            });
            for product in &mut self.products {
                product.used_by_line.push(false);
            }

            self.lines.len() - 1
        };

        if let Some(pn) = self.products.iter().position(|f| f.name == product) {
            self.products[pn].used_by_line[line_number] = true;
        } else {
            let mut prod = Product {
                name: product,
                selected: false,
                available_by_selected_lines: true,
                used_by_line: vec![false; self.lines.len()],
            };
            prod.used_by_line[line_number] = true;
            self.products.push(prod);
        }
    }

    fn sort(&mut self) {
        self.products.sort_by(|a, b| a.name.cmp(&b.name));
    }

    fn get_line_selection(&self) -> Vec<bool> {
        self.lines.iter().map(|f| f.selected).collect()
    }

    fn get_selected_lines(&self) -> Vec<String> {
        let mut ret = Vec::new();

        for line in &self.lines {
            if line.selected {
                ret.push(line.name.clone());
            }
        }

        ret
    }

    fn get_selected_products(&self) -> Vec<String> {
        let mut ret = Vec::new();

        for product in &self.products {
            if product.selected && product.available_by_selected_lines {
                ret.push(product.name.clone());
            }
        }

        ret
    }
}

impl Product {
    fn update_availability(&mut self, selected_lines: &[bool]) {
        if !selected_lines.iter().any(|f| *f) {
            // if no lines are selected
            self.available_by_selected_lines = true;
        } else {
            self.available_by_selected_lines = false;
            for (line, product) in self.used_by_line.iter().zip(selected_lines) {
                if *line && *product {
                    self.available_by_selected_lines = true;
                    return;
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct SpiStation {
    status: Arc<Mutex<Status>>,
    status_message: Arc<Mutex<String>>,
    stations: Arc<Mutex<Stations>>,

    boards: Arc<Mutex<Vec<SPI_log_file::helpers::SingleBoard>>>,
    failure_counter: Arc<Mutex<Option<FailedCompCounter>>>,
}

impl Default for SpiStation {
    fn default() -> Self {
        Self {
            status: Arc::new(Mutex::new(Status::UnInitialized)),
            status_message: Arc::new(Mutex::new(String::new())),
            stations: Arc::new(Mutex::new(Stations::default())),

            boards: Arc::new(Mutex::new(Vec::new())),
            failure_counter: Arc::new(Mutex::new(None)),
        }
    }
}

impl SpiStation {
    fn initialized(&self) -> bool {
        match *self.status.lock().unwrap() {
            Status::UnInitialized | Status::Initializing | Status::Error => false,
            Status::Standby | Status::Loading => true,
        }
    }

    fn initialize(
        &mut self,
        ctx: &egui::Context,
        connection: Arc<tokio::sync::Mutex<Option<Client<Compat<TcpStream>>>>>,
    ) {
        if *self.status.lock().unwrap() != Status::UnInitialized {
            return;
        }

        info!("Starting SPI module initialization");

        *self.status.lock().unwrap() = Status::Initializing;
        *self.status_message.lock().unwrap() = String::from("Modul inicializáció...");

        let ctx = ctx.clone();
        let status = self.status.clone();
        let message = self.status_message.clone();
        let stations = self.stations.clone();

        tokio::spawn(async move {
            let mut client_opt = connection.lock().await;

            loop {
                if client_opt.is_none() {
                    match create_connection().await {
                        Ok(conn) => *client_opt = Some(conn),
                        Err(e) => {
                            error!("Initialization FAILED! {}", e);
                            *status.lock().unwrap() = Status::Error;
                            *message.lock().unwrap() = format!(
                                "Az SQL kapcsolat sikertelen!\n10mp múlva újra próbáljuk!\n{e}"
                            );

                            tokio::time::sleep(Duration::from_secs(10)).await;
                            continue;
                        }
                    }
                }

                if !check_connection(client_opt.as_mut().unwrap()).await {
                    *client_opt = None;
                    error!("SQL server diconnected!");
                    *status.lock().unwrap() = Status::Error;
                    *message.lock().unwrap() =
                        "Az kapcsolat megszakadt! Újracsatlakozás...".to_string();
                } else {
                    break;
                }
            }

            let client = client_opt.as_mut().unwrap();

            let query = Query::new(
                "SELECT Station, Program
                FROM dbo.SMT_AOI_RESULTS
                WHERE Station LIKE 'SPI%'
                GROUP BY Station, Program",
            );

            if let Ok(mut result) = query.query(client).await {
                while let Some(row) = result.next().await {
                    let row = row.unwrap();
                    match row {
                        tiberius::QueryItem::Row(x) => {
                            // Station, Program, Variant
                            let station = x.get::<&str, usize>(0).unwrap().to_owned();
                            let program = x.get::<&str, usize>(1).unwrap().to_owned();

                            debug!("Result: {station} - {program}");

                            // Populating stations/products/variants structs
                            stations.lock().unwrap().push(station, program);
                        }
                        tiberius::QueryItem::Metadata(_) => (),
                    }
                }

                stations.lock().unwrap().sort();

                info!("Initialization OK!");
                *status.lock().unwrap() = Status::Standby;
            } else {
                error!("Initialization FAILED!");
                *status.lock().unwrap() = Status::Error;
                *message.lock().unwrap() =
                    String::from("Inícializáció sikertelen! Ellenőrizze a kapcsolatot!");
            }

            ctx.request_repaint();
        });
    }

    fn query(
        &mut self,
        ctx: &egui::Context,
        timeframe: TimeFrame<'_>,
        connection: Arc<tokio::sync::Mutex<Option<Client<Compat<TcpStream>>>>>,
    ) {
        if *self.status.lock().unwrap() != Status::Standby {
            return;
        }

        info!("Starting AOI query.");

        *self.status.lock().unwrap() = Status::Loading;
        *self.status_message.lock().unwrap() = String::from("Lekérdezés SQL-ből...");

        let ctx = ctx.clone();
        let status = self.status.clone();
        let message = self.status_message.clone();

        self.boards.lock().unwrap().clear();
        *self.failure_counter.lock().unwrap() = None;
        let boards = self.boards.clone();
        let failure_counter = self.failure_counter.clone();

        let stations = self.stations.lock().unwrap();
        let lines = stations.get_selected_lines();
        let programs = stations.get_selected_products();

        let start_datetime = timeframe
            .0
            .and_hms_opt(timeframe.1 .0, timeframe.1 .1, 0)
            .unwrap();
        let end_datetime = if *timeframe.2 {
            Some(
                timeframe
                    .3
                    .and_hms_opt(timeframe.4 .0, timeframe.4 .1, 0)
                    .unwrap(),
            )
        } else {
            None
        };

        tokio::spawn(async move {
            let mut client_opt = connection.lock().await;

            loop {
                if client_opt.is_none() {
                    match create_connection().await {
                        Ok(conn) => *client_opt = Some(conn),
                        Err(e) => {
                            error!("Connection FAILED! {}", e);
                            *status.lock().unwrap() = Status::Error;
                            *message.lock().unwrap() = format!(
                                "Az SQL kapcsolat sikertelen!\n10mp múlva újra próbáljuk!\n{e}"
                            );

                            tokio::time::sleep(Duration::from_secs(10)).await;
                            continue;
                        }
                    }
                }

                if !check_connection(client_opt.as_mut().unwrap()).await {
                    *client_opt = None;
                    error!("SQL server diconnected!");
                    *status.lock().unwrap() = Status::Error;
                    *message.lock().unwrap() =
                        "Az kapcsolat megszakadt! Újracsatlakozás...".to_string();
                } else {
                    break;
                }
            }

            let client = client_opt.as_mut().unwrap();
            /*
                SELECT q1.Serial_NMBR, q1.Date_Time, q1.Station, q1.Program, q1.Variant, q1.Result, q1.Data
                FROM dbo.SMT_AOI_RESULTS q1 INNER JOIN
                (
                SELECT Serial_NMBR, Program, MAX(Date_Time) AS Time
                FROM dbo.SMT_AOI_RESULTS
                WHERE Station = 'JV_Line10' AND Date_Time > '2025-04-17 00:00:00.000' AND NOT Serial_NMBR = 'NO__BC'
                GROUP BY Serial_NMBR, Program
                ) q2
                ON (q1.Serial_NMBR = q2.Serial_NMBR AND q1.Program = q2.Program AND q1.Date_Time = q2.Time)
            */

            // Crafting the query
            let mut query_text = String::from(
                "SELECT q1.Serial_NMBR, q1.Date_Time, q1.Station, q1.Program, q1.Variant, q1.Result, q1.Data
                FROM dbo.SMT_AOI_RESULTS q1 INNER JOIN 
                (
                SELECT Serial_NMBR, Program, MAX(Date_Time) AS Time
                FROM dbo.SMT_AOI_RESULTS
                WHERE ",
            );

            if !lines.is_empty() {
                let x = lines
                    .iter()
                    .map(|f| format!("'{}'", f))
                    .collect::<Vec<String>>()
                    .join(", ");
                query_text += &format!("Station IN ({x}) AND ");
            } else {
                query_text += "Station LIKE 'SPI%' AND "
            }

            if !programs.is_empty() {
                let x = programs
                    .iter()
                    .map(|f| format!("'{}'", f))
                    .collect::<Vec<String>>()
                    .join(", ");
                query_text += &format!("Program IN ({x}) AND ");
            }

            if let Some(et) = end_datetime {
                query_text += &format!(
                    "Date_Time BETWEEN '{}' AND '{}'",
                    start_datetime.format("%Y-%m-%d %H:%M:%S"),
                    et.format("%Y-%m-%d %H:%M:%S")
                );
            } else {
                query_text += &format!(
                    "Date_Time > '{}'",
                    start_datetime.format("%Y-%m-%d %H:%M:%S")
                );
            }

            query_text += "
                GROUP BY Serial_NMBR, Program
                ) q2
                ON (q1.Serial_NMBR = q2.Serial_NMBR AND q1.Program = q2.Program AND q1.Date_Time = q2.Time)";

            debug!("Query text: {}", query_text);

            if let Ok(mut result) = client.query(query_text, &[]).await {
                while let Some(row) = result.next().await {
                    let row = row.unwrap();
                    match row {
                        tiberius::QueryItem::Row(x) => {
                            let barcode = x.get::<&str, usize>(0).unwrap().to_owned();
                            let date_time = x.get::<NaiveDateTime, usize>(1).unwrap().to_owned();
                            let station = x.get::<&str, usize>(2).unwrap().to_owned();
                            let inspection_plan = x.get::<&str, usize>(3).unwrap().to_owned();
                            let variant = x.get::<&str, usize>(4).unwrap().to_owned();
                            let result_text = x.get::<&str, usize>(5).unwrap().to_owned();
                            let data = x.get::<&str, usize>(6).unwrap().to_owned();

                            // Populating stations/products/variants structs
                            boards
                                .lock()
                                .unwrap()
                                .push(SPI_log_file::helpers::SingleBoard {
                                    barcode,
                                    result: result_text == "Pass",
                                    inspection_plan,
                                    variant,
                                    station,
                                    date_time,
                                    failed_board_data: serde_json::from_str(&data).unwrap(),
                                });
                        }
                        tiberius::QueryItem::Metadata(_) => (),
                    }
                }

                let boards_len = boards.lock().unwrap().len();
                info!("Query OK!");
                *message.lock().unwrap() =
                    format!("Lekérdezés sikeres! {boards_len} eredmény feldolgozása...");

                info! {"Lekérdezés sikeres! {boards_len} eredmény feldolgozása..."};
                let mut counter =
                    SPI_log_file::helpers::FailedCompCounter::generate(&boards.lock().unwrap());
                counter.sort();
                *failure_counter.lock().unwrap() = Some(counter);

                *status.lock().unwrap() = Status::Standby;
            } else {
                error!("Query FAILED!");
                *status.lock().unwrap() = Status::Error;
                *message.lock().unwrap() = String::from("Lekérdezés sikertelen! SQL hiba!");
            }

            ctx.request_repaint();
        });
    }

    pub fn side_panel(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        timeframe: TimeFrame<'_>,
        connection: Arc<tokio::sync::Mutex<Option<Client<Compat<TcpStream>>>>>,
    ) {
        if !self.initialized() {
            self.initialize(ctx, connection.clone());
            return;
        }

        let mut stations = self.stations.lock().unwrap();
        let mut stations_changed = false;

        ui.style_mut().spacing.scroll = ScrollStyle::solid();

        for line in &mut stations.lines {
            ui.horizontal(|ui| {
                if ui.checkbox(&mut line.selected, &line.name).changed() {
                    stations_changed = true;
                }
            });
        }

        let station_update = if stations_changed {
            Some(stations.get_line_selection())
        } else {
            None
        };

        ui.separator();
        let mut query = false;

        egui::ScrollArea::vertical().show(ui, |ui| {
            for product in &mut stations.products {
                if let Some(line_sel) = &station_update {
                    product.update_availability(line_sel);
                }

                if product.available_by_selected_lines {
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut product.selected, &product.name);
                    });
                }
            }

            ui.add_space(10.0);

            ui.vertical_centered(|ui| {
                if ui.button("Lekérdezés").clicked() {
                    query = true;
                }
            });
        });

        drop(stations);

        if query {
            self.query(ctx, timeframe, connection.clone());
        }
    }

    pub fn central_panel(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        ui.style_mut().spacing.scroll = ScrollStyle::solid();

        if *self.status.lock().unwrap() != Status::Standby {
            ui.vertical_centered(|ui| {
                ui.add(egui::Spinner::new().size(200.0));

                ui.label(self.status_message.lock().unwrap().as_str());
            });

            return;
        }

        if let Some( failures) = self.failure_counter.lock().unwrap().as_mut() {
            TableBuilder::new(ui)
                .id_salt("SpiErrTable")
                .striped(true)
                .auto_shrink([false, true])
                .cell_layout(Layout::from_main_dir_and_cross_align(
                    egui::Direction::LeftToRight,
                    egui::Align::Center,
                ))
                .column(Column::auto().at_least(200.0))
                .columns(Column::auto().resizable(true), 2)
                .header(20.0, |mut header| {
                    header.col(|ui| {});
                    header.col(|ui| {
                        ui.label("Pseudo NOK");
                    });
                    header.col(|ui| {
                        ui.label("True NOK");
                    });
                })
                .body(|mut body| {
                    for ip in &mut failures.inspection_plans {
                        body.row(20.0, |mut row| {
                            row.col(|ui| {
                                colapsing_button(ui, &mut ip.show);
                                ui.label(format!("{} ({} pcb OK)", ip.name, ip.count_ok));
                            });
                            row.col(|ui| {
                                ui.label(&ip.count_pseudo_nok.to_string());
                            });
                            row.col(|ui| {
                                ui.label(&ip.count_nok.to_string());
                            });
                        });

                        if ip.show {
                            for comp in &mut ip.components {
                                body.row(20.0, |mut row| {
                                    row.col(|ui| {
                                        ui.add_space(20.0);
                                        colapsing_button(ui, &mut comp.show);
                                        ui.label(&comp.name);
                                    });
                                    row.col(|ui| {
                                        ui.label(&comp.count_pseudo_nok.to_string());
                                    });
                                    row.col(|ui| {
                                        ui.label(&comp.count_nok.to_string());
                                    });
                                });

                                if comp.show {
                                    for pad in &mut comp.pads {
                                        body.row(20.0, |mut row| {
                                            row.col(|ui| {
                                                ui.add_space(40.0);
                                                colapsing_button(ui, &mut pad.show);
                                                ui.label(&pad.name);
                                            });
                                            row.col(|ui| {
                                                ui.label(&pad.count_pseudo_nok.to_string());
                                            });
                                            row.col(|ui| {
                                                ui.label(&pad.count_nok.to_string());
                                            });
                                        });

                                        if pad.show {
                                            for feature in &mut pad.features {
                                                body.row(20.0, |mut row| {
                                                    row.col(|ui| {
                                                        ui.add_space(60.0);
                                                        colapsing_button(ui, &mut feature.show);
                                                        ui.label(&feature.name);
                                                    });
                                                    row.col(|ui| {
                                                        ui.label(&feature.count_pseudo_nok.to_string());
                                                    });
                                                    row.col(|ui| {
                                                        ui.label(&feature.count_nok.to_string());
                                                    });
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                });
        }
    }
}

fn colapsing_button(ui: &mut egui::Ui, b: &mut bool) {
    if ui.button(if *b { "v" } else { "-" }).clicked() {
        *b = !*b;
    }
}
