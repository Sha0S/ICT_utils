use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use chrono::NaiveDateTime;
use egui::{style::ScrollStyle, Color32, Layout, RichText};
use egui_extras::{Column, TableBuilder};
use log::{debug, error, info};
use tiberius::{Client, Query};
use tokio::net::TcpStream;
use tokio_stream::StreamExt;
use tokio_util::compat::Compat;

use crate::{
    connection::{check_connection, create_connection},
    TimeFrame,
};

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

#[derive(Debug, Clone, Copy, PartialEq)]
enum ActivePanel {
    PseudoErrors,
    Timeline,
    ErrorList,
}

#[derive(Debug)]
pub struct AoiStation {
    active_panel: ActivePanel,
    status: Arc<Mutex<Status>>,
    status_message: Arc<Mutex<String>>,
    stations: Arc<Mutex<Stations>>,

    daily: bool,
    pseudo_errors: bool,
    error_limit_per_board: usize,

    boards: Arc<Mutex<Vec<AOI_log_file::helpers::SingleBoard>>>,
    error_counter: Arc<Mutex<Option<AOI_log_file::helpers::PseudoErrC>>>,
    error_daily: Arc<Mutex<Option<AOI_log_file::helpers::ErrorTrackerT>>>,
    error_list: Arc<Mutex<Option<AOI_log_file::helpers::ErrorList>>>,
}

impl Default for AoiStation {
    fn default() -> Self {
        Self {
            active_panel: ActivePanel::PseudoErrors,
            status: Arc::new(Mutex::new(Status::UnInitialized)),
            status_message: Arc::new(Mutex::new(String::new())),
            stations: Arc::new(Mutex::new(Stations::default())),
            daily: false,
            pseudo_errors: true,
            error_limit_per_board: 50,
            boards: Arc::new(Mutex::new(Vec::new())),
            error_counter: Arc::new(Mutex::new(None)),
            error_daily: Arc::new(Mutex::new(None)),
            error_list: Arc::new(Mutex::new(None)),
        }
    }
}

impl AoiStation {
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

        info!("Starting AOI module initialization");

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
        let limit = self.error_limit_per_board;

        self.boards.lock().unwrap().clear();
        let boards = self.boards.clone();

        *self.error_counter.lock().unwrap() = None;
        let error_counter = self.error_counter.clone();

        *self.error_daily.lock().unwrap() = None;
        let error_daily = self.error_daily.clone();

        *self.error_list.lock().unwrap() = None;
        let error_list = self.error_list.clone();

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
                SELECT q1.Serial_NMBR, q1.Date_Time, q1.Station, q1.Program, q1.Variant, q1.Operator, q1.Result, q1.Data
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
                "SELECT q1.Serial_NMBR, q1.Date_Time, q1.Station, q1.Program, q1.Variant, q1.Operator, q1.Result, q1.Data
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

            query_text += " AND NOT Serial_NMBR = 'NO__BC'
                GROUP BY Serial_NMBR, Program
                ) q2
                ON (q1.Serial_NMBR = q2.Serial_NMBR AND q1.Program = q2.Program AND q1.Date_Time = q2.Time)";

            debug!("Query text: {}", query_text);

            if let Ok(mut result) = client.query(query_text, &[]).await {
                while let Some(row) = result.next().await {
                    let row = row.unwrap();
                    match row {
                        tiberius::QueryItem::Row(x) => {
                            // Serial_NMBR, Date_Time, Station, Program, Variant, Operator, Result, [Data]
                            let barcode = x.get::<&str, usize>(0).unwrap().to_owned();
                            let date_time = x.get::<NaiveDateTime, usize>(1).unwrap().to_owned();
                            let station = x.get::<&str, usize>(2).unwrap().to_owned();
                            let inspection_plan = x.get::<&str, usize>(3).unwrap().to_owned();
                            let variant = x.get::<&str, usize>(4).unwrap().to_owned();
                            let operator = x.get::<&str, usize>(5).unwrap().to_owned();
                            let result_text = x.get::<&str, usize>(6).unwrap().to_owned();
                            let data = x.get::<&str, usize>(7).unwrap().to_owned();

                            // Populating stations/products/variants structs
                            boards
                                .lock()
                                .unwrap()
                                .push(AOI_log_file::helpers::SingleBoard {
                                    barcode,
                                    result: result_text == "Pass",
                                    inspection_plan,
                                    variant,
                                    station,
                                    date_time,
                                    operator,
                                    windows: serde_json::from_str(&data).unwrap(),
                                });
                        }
                        tiberius::QueryItem::Metadata(_) => (),
                    }
                }

                let boards_len = boards.lock().unwrap().len();
                info!("Query OK!");
                *message.lock().unwrap() =
                    format!("Lekérdezés sikeres! {boards_len} eredmény feldolgozása...");

                let mut counter =
                    AOI_log_file::helpers::PseudoErrC::generate(limit, &boards.lock().unwrap());

                counter.sort_by_ip_id(None);
                *error_counter.lock().unwrap() = Some(counter);

                let daily =
                    AOI_log_file::helpers::ErrorTrackerT::generate(limit, &boards.lock().unwrap());
                *error_daily.lock().unwrap() = Some(daily);

                let elist =
                    AOI_log_file::helpers::ErrorList::generate(limit, &boards.lock().unwrap());
                *error_list.lock().unwrap() = Some(elist);

                *status.lock().unwrap() = Status::Standby;
            } else {
                error!("Query FAILED!");
                *status.lock().unwrap() = Status::Error;
                *message.lock().unwrap() = String::from("Lekérdezés sikertelen! SQL hiba!");
            }

            ctx.request_repaint();
        });
    }

    fn reload_after_limit_change(&mut self, ctx: &egui::Context) {
        if *self.status.lock().unwrap() != Status::Standby {
            return;
        }

        info!("Updating set limit.");

        *self.status.lock().unwrap() = Status::Loading;
        *self.status_message.lock().unwrap() = String::from("Adatok újra-generálása");

        let ctx = ctx.clone();
        let status = self.status.clone();
        let limit = self.error_limit_per_board;

        let boards = self.boards.clone();

        *self.error_counter.lock().unwrap() = None;
        let error_counter = self.error_counter.clone();

        *self.error_daily.lock().unwrap() = None;
        let error_daily = self.error_daily.clone();

        *self.error_list.lock().unwrap() = None;
        let error_list = self.error_list.clone();

        tokio::spawn(async move {
            let mut counter =
                AOI_log_file::helpers::PseudoErrC::generate(limit, &boards.lock().unwrap());

            counter.sort_by_ip_id(None);
            *error_counter.lock().unwrap() = Some(counter);

            let daily =
                AOI_log_file::helpers::ErrorTrackerT::generate(limit, &boards.lock().unwrap());
            *error_daily.lock().unwrap() = Some(daily);

            let elist = AOI_log_file::helpers::ErrorList::generate(limit, &boards.lock().unwrap());
            *error_list.lock().unwrap() = Some(elist);

            *status.lock().unwrap() = Status::Standby;
        });

        ctx.request_repaint();
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
        ui.horizontal(|ui| {
            if ui.button("Pszeudóhibák").clicked() {
                self.active_panel = ActivePanel::PseudoErrors;
            }

            if ui.button("Idő szerint").clicked() {
                self.active_panel = ActivePanel::Timeline;
            }

            if ui.button("Kieső PCB-k listája").clicked() {
                self.active_panel = ActivePanel::ErrorList;
            }
        });

        let mut limit_changed = false;

        ui.separator();

        if self.active_panel == ActivePanel::PseudoErrors {
            if let Some(counter) = self.error_counter.lock().unwrap().as_mut() {
                ui.horizontal(|ui| {
                    ui.label("Hiba limit:");
                    if ui
                        .add(egui::DragValue::new(&mut self.error_limit_per_board).range(0..=100))
                        .lost_focus()
                    {
                        limit_changed = true;
                    }
                });

                ui.separator();
                let mut sort_after = None;

                TableBuilder::new(ui)
                    .id_salt("Pszeudo")
                    .striped(true)
                    .auto_shrink([false, true])
                    .cell_layout(Layout::from_main_dir_and_cross_align(
                        egui::Direction::LeftToRight,
                        egui::Align::Center,
                    ))
                    .column(Column::auto().at_least(200.0))
                    .columns(
                        Column::auto().resizable(true),
                        counter.inspection_plans.len(),
                    )
                    .header(20.0, |mut header| {
                        header.col(|ui| {
                            ui.label("Macros");
                        });
                        for (i, iplan) in counter.inspection_plans.iter().enumerate() {
                            header.col(|ui| {
                                ui.vertical(|ui| {
                                    if ui.label(iplan).clicked() {
                                        sort_after = Some(i);
                                    }
                                    ui.label(format!(
                                        "{} / {} pcb",
                                        counter.failed_boards[i], counter.total_boards[i]
                                    ));
                                    ui.label(format!(
                                        "{} ({:.2} avg)",
                                        counter.total_pseudo[i], counter.pseudo_per_board[i]
                                    ));
                                });
                                ui.add_space(10.0);
                            });
                        }
                    })
                    .body(|mut body| {
                        for macroc in &mut counter.macros {
                            body.row(20.0, |mut row| {
                                row.col(|ui| {
                                    colapsing_button(ui, &mut macroc.show);
                                    ui.label(&macroc.name);
                                });
                                for iplanc in &macroc.total_pseudo {
                                    row.col(|ui| {
                                        ui.label(iplanc.to_string());
                                    });
                                }
                            });

                            if macroc.show {
                                for package in &mut macroc.packages {
                                    body.row(20.0, |mut row| {
                                        row.col(|ui| {
                                            ui.add_space(20.0);
                                            colapsing_button(ui, &mut package.show);
                                            ui.label(&package.name);
                                        });
                                        for iplanc in &package.total_pseudo {
                                            row.col(|ui| {
                                                ui.label(iplanc.to_string());
                                            });
                                        }
                                    });

                                    if package.show {
                                        for position in &package.positions {
                                            body.row(20.0, |mut row| {
                                                row.col(|ui| {
                                                    ui.add_space(60.0);
                                                    ui.label(&position.name);
                                                });
                                                for iplanc in &position.total_pseudo {
                                                    row.col(|ui| {
                                                        ui.label(iplanc.to_string());
                                                    });
                                                }
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    });

                if let Some(i) = sort_after {
                    counter.sort_by_ip_id(Some(i));
                }
            }
        } else if self.active_panel == ActivePanel::Timeline {
            // This uses a lot of duplicated code, could try to simplify it later
            // would potentially need to implement Traits for day/week structs.

            if let Some(daily) = self.error_daily.lock().unwrap().as_ref() {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.daily, true, "Napi");
                    ui.selectable_value(&mut self.daily, false, "Heti");
                    ui.add_space(30.0);
                    ui.selectable_value(&mut self.pseudo_errors, true, "Pszeudo hibák");
                    ui.selectable_value(&mut self.pseudo_errors, false, "Valós hibák");
                    ui.add_space(30.0);
                    ui.label("Hiba limit:");
                    if ui
                        .add(egui::DragValue::new(&mut self.error_limit_per_board).range(0..=100))
                        .lost_focus()
                    {
                        limit_changed = true;
                    }
                });

                ui.separator();

                egui::ScrollArea::both()
                    .scroll_bar_visibility(
                        egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                    )
                    .show(ui, |ui| {
                        if self.daily {
                            ui.add_space(50.0);
                            ui.heading("Fejlesztés alatt.");
                            ui.add_space(50.0);

                            for inspection_plan in &daily.inspection_plans {
                                ui.add_space(20.0);
                                ui.label(&inspection_plan.name);

                                TableBuilder::new(ui)
                                    .id_salt(&inspection_plan.name)
                                    .striped(true)
                                    .cell_layout(Layout::from_main_dir_and_cross_align(
                                        egui::Direction::LeftToRight,
                                        egui::Align::Center,
                                    ))
                                    .column(Column::auto().at_least(100.0))
                                    .columns(Column::auto(), inspection_plan.days.len())
                                    .header(20.0, |mut header| {
                                        header.col(|_ui| {});
                                        for day in inspection_plan.days.iter() {
                                            header.col(|ui| {
                                                ui.label(day.date.format("%m. %d.").to_string());
                                            });
                                        }
                                    })
                                    .body(|mut body| {
                                        body.row(20.0, |mut row| {
                                            row.col(|ui| {
                                                ui.label("Összes pcb");
                                            });

                                            for day in inspection_plan.days.iter() {
                                                row.col(|ui| {
                                                    ui.label(day.total_boards.to_string());
                                                });
                                            }
                                        });
                                        body.row(20.0, |mut row| {
                                            row.col(|ui| {
                                                ui.label("Kieső pcb");
                                            });

                                            for day in inspection_plan.days.iter() {
                                                row.col(|ui| {
                                                    ui.label(day.p_failed_boards.to_string());
                                                });
                                            }
                                        });
                                        body.row(20.0, |mut row| {
                                            row.col(|ui| {
                                                ui.label("Hiba átlag");
                                            });

                                            for day in inspection_plan.days.iter() {
                                                row.col(|ui| {
                                                    ui.label(format!(
                                                        "{:.2}",
                                                        day.pseudo_errors_per_board
                                                    ));
                                                });
                                            }
                                        });
                                        body.row(20.0, |mut row| {
                                            row.col(|ui| {
                                                ui.label("Delta");
                                            });

                                            let mut yesterday = None;

                                            for day in inspection_plan.days.iter() {
                                                row.col(|ui| {
                                                    if let Some(i) = yesterday {
                                                        let delta = day.pseudo_errors_per_board - i;

                                                        ui.label(
                                                            RichText::new(format!("{:+.2}", delta))
                                                                .color(if delta > 0.0 {
                                                                    Color32::RED
                                                                } else {
                                                                    Color32::GREEN
                                                                }),
                                                        );
                                                    }

                                                    yesterday = Some(day.pseudo_errors_per_board);
                                                });
                                            }
                                        });
                                    });
                            }
                        } else {
                            for inspection_plan in &daily.inspection_plans {
                                ui.add_space(20.0);
                                ui.label(&inspection_plan.name);

                                TableBuilder::new(ui)
                                    .id_salt(&inspection_plan.name)
                                    .striped(true)
                                    .vscroll(false)
                                    .auto_shrink([false, true])
                                    .cell_layout(Layout::from_main_dir_and_cross_align(
                                        egui::Direction::LeftToRight,
                                        egui::Align::Center,
                                    ))
                                    .column(Column::auto().at_least(100.0))
                                    .columns(
                                        Column::auto().at_least(100.0),
                                        inspection_plan.weeks.len(),
                                    )
                                    .header(20.0, |mut header| {
                                        header.col(|_ui| {});
                                        for week in inspection_plan.weeks.iter() {
                                            header.col(|ui| {
                                                ui.label(format!("wk{}", week.week));
                                            });
                                        }
                                    })
                                    .body(|mut body| {
                                        body.row(20.0, |mut row| {
                                            row.col(|ui| {
                                                ui.label("Összes pcb");
                                            });

                                            for week in inspection_plan.weeks.iter() {
                                                row.col(|ui| {
                                                    ui.label(week.total_boards.to_string());
                                                });
                                            }
                                        });
                                        body.row(20.0, |mut row| {
                                            row.col(|ui| {
                                                ui.label("Kieső pcb");
                                            });

                                            for week in inspection_plan.weeks.iter() {
                                                row.col(|ui| {
                                                    ui.label(if self.pseudo_errors {
                                                        week.p_failed_boards.to_string()
                                                    } else {
                                                        week.r_failed_boards.to_string()
                                                    });
                                                });
                                            }
                                        });
                                        body.row(20.0, |mut row| {
                                            row.col(|ui| {
                                                ui.label("Hiba összesen");
                                            });

                                            for week in inspection_plan.weeks.iter() {
                                                row.col(|ui| {
                                                    ui.label(if self.pseudo_errors {
                                                        week.total_pseudo_errors.to_string()
                                                    } else {
                                                        week.total_real_errors.to_string()
                                                    });
                                                });
                                            }
                                        });

                                        if self.pseudo_errors {
                                            body.row(20.0, |mut row| {
                                                row.col(|ui| {
                                                    ui.label("Hiba átlag");
                                                });

                                                for week in inspection_plan.weeks.iter() {
                                                    row.col(|ui| {
                                                        ui.label(format!(
                                                            "{:.2}",
                                                            week.pseudo_errors_per_board
                                                        ));
                                                    });
                                                }
                                            });
                                            body.row(40.0, |mut row| {
                                                row.col(|ui| {
                                                    ui.label("Delta");
                                                });

                                                let mut last_week = None;

                                                for week in inspection_plan.weeks.iter() {
                                                    row.col(|ui| {
                                                        if let Some(i) = last_week {
                                                            let delta =
                                                                week.pseudo_errors_per_board - i;
                                                            let deltap =
                                                                (week.pseudo_errors_per_board / i
                                                                    - 1.0)
                                                                    * 100.0;

                                                            ui.vertical(|ui| {
                                                                ui.label(
                                                                    RichText::new(format!(
                                                                        "{:+.2}",
                                                                        delta
                                                                    ))
                                                                    .color(if delta > 0.0 {
                                                                        Color32::RED
                                                                    } else {
                                                                        Color32::GREEN
                                                                    }),
                                                                );

                                                                ui.label(
                                                                    RichText::new(format!(
                                                                        "{:+.2}%",
                                                                        deltap
                                                                    ))
                                                                    .color(if delta > 0.0 {
                                                                        Color32::RED
                                                                    } else {
                                                                        Color32::GREEN
                                                                    }),
                                                                );
                                                            });
                                                        }

                                                        last_week =
                                                            Some(week.pseudo_errors_per_board);
                                                    });
                                                }
                                            });
                                        } else {
                                            body.row(20.0, |mut row| {
                                                row.col(|ui| {
                                                    ui.label("PPM");
                                                });

                                                for week in inspection_plan.weeks.iter() {
                                                    row.col(|ui| {
                                                        let ppm = week.r_failed_boards as f32
                                                            * (1_000_000.0
                                                                / week.total_boards as f32);
                                                        ui.label(format!("{:.2}", ppm));
                                                    });
                                                }
                                            });
                                            /*body.row(40.0, |mut row| {
                                                row.col(|ui| {
                                                    ui.label("Delta");
                                                });

                                                let mut last_week = None;

                                                for week in inspection_plan.weeks.iter() {
                                                    row.col(|ui| {
                                                        if let Some(i) = last_week {
                                                            let delta = if self.pseudo_errors {
                                                                week.pseudo_errors_per_board - i
                                                            } else {
                                                                week.real_errors_per_board - i
                                                            };
                                                            let deltap = if self.pseudo_errors {
                                                                (week.pseudo_errors_per_board / i - 1.0)
                                                                    * 100.0
                                                            } else {
                                                                (week.real_errors_per_board / i - 1.0)
                                                                    * 100.0
                                                            };

                                                            ui.vertical(|ui| {
                                                                ui.label(
                                                                    RichText::new(format!(
                                                                        "{:+.2}",
                                                                        delta
                                                                    ))
                                                                    .color(if delta > 0.0 {
                                                                        Color32::RED
                                                                    } else {
                                                                        Color32::GREEN
                                                                    }),
                                                                );

                                                                ui.label(
                                                                    RichText::new(format!(
                                                                        "{:+.2}%",
                                                                        deltap
                                                                    ))
                                                                    .color(if delta > 0.0 {
                                                                        Color32::RED
                                                                    } else {
                                                                        Color32::GREEN
                                                                    }),
                                                                );
                                                            });
                                                        }

                                                        last_week = Some(if self.pseudo_errors {
                                                            week.pseudo_errors_per_board
                                                        } else {
                                                            week.real_errors_per_board
                                                        });
                                                    });
                                                }
                                            });*/
                                        }
                                    });
                            }
                        }
                    });
            }
        } else if self.active_panel == ActivePanel::ErrorList {
            if let Some(list) = self.error_list.lock().unwrap().as_ref() {
                ui.horizontal(|ui| {
                    ui.label("Hiba limit:");
                    if ui
                        .add(egui::DragValue::new(&mut self.error_limit_per_board).range(0..=100))
                        .lost_focus()
                    {
                        limit_changed = true;
                    }
                });

                ui.separator();

                egui::ScrollArea::both()
                    .scroll_bar_visibility(
                        egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                    )
                    .show(ui, |ui| {
                        for inspection_plan in &list.inspection_plans {
                            ui.add_space(20.0);
                            ui.label(&inspection_plan.name);

                            TableBuilder::new(ui)
                                .id_salt(&inspection_plan.name)
                                .striped(true)
                                .vscroll(false)
                                .auto_shrink([false, true])
                                .cell_layout(Layout::from_main_dir_and_cross_align(
                                    egui::Direction::LeftToRight,
                                    egui::Align::Center,
                                ))
                                .column(Column::auto().at_least(100.0)) // Serial
                                .column(Column::auto().at_least(100.0)) // DateTime
                                .column(Column::remainder().at_least(200.0)) // Positions
                                .body(|mut body| {
                                    for board in &inspection_plan.failed_boards {
                                        body.row(20.0, |mut row| {
                                            row.col(|ui| {
                                                ui.label(&board.barcode);
                                            });
                                            row.col(|ui| {
                                                ui.label(
                                                    board
                                                        .date_time
                                                        .format("%Y-%m-%d %H:%M:%S")
                                                        .to_string(),
                                                );
                                            });
                                            row.col(|ui| {
                                                ui.label(board.failed_positions.join(", "));
                                            });
                                        });
                                    }
                                });
                        }
                    });
            }
        }

        if limit_changed {
            self.reload_after_limit_change(ctx)
        }
    }
}

fn colapsing_button(ui: &mut egui::Ui, b: &mut bool) {
    if ui.button(if *b { "V" } else { ">" }).clicked() {
        *b = !*b;
    }
}
