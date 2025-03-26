use std::sync::{Arc, Mutex};

use chrono::NaiveDateTime;
use egui::style::ScrollStyle;
use log::{debug, error, info};
use tiberius::{Client, Query};
use tokio::net::TcpStream;
use tokio_stream::StreamExt;
use tokio_util::compat::Compat;

use crate::TimeFrame;




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
    products: Vec<Product>
}

#[derive(Debug)]
struct Line {
    name: String,
    selected: bool
}

#[derive(Debug)]
struct Product {
    name: String,
    selected: bool,
    used_by_line: Vec<bool>,
}

impl Stations {
    fn push(&mut self, line: String, product: String ) {
        let line = line.to_ascii_uppercase();
        
        let line_number = if let Some(ln) = self.lines.iter().rposition(|f| f.name == line) {
            ln
        } else {
            self.lines.push(Line { name: line, selected: false });
            for product in &mut self.products {
                product.used_by_line.push(false);
            }

            self.lines.len()-1
        };

        if let Some(pn) = self.products.iter().position(|f| f.name == product) {
            self.products[pn].used_by_line[line_number] = true;
        } else {
            let mut prod = Product { name: product, selected: false, used_by_line: vec![false; self.lines.len()] };
            prod.used_by_line[line_number] = true;
            self.products.push(prod);
        }
        
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
            if product.selected {
                ret.push(product.name.clone());
            }
        }

        ret
    }
}


#[derive(Debug)]
pub struct AoiStation {
    status: Arc<Mutex<Status>>,
    status_message: Arc<Mutex<String>>,

    stations: Arc<Mutex<Stations>>,

    boards: Arc<Mutex<Vec<AOI_log_file::helpers::SingleBoard>>>,
    error_counter: Arc<Mutex<Option<AOI_log_file::helpers::ErrorCounter>>>,
}

impl Default for AoiStation {
    fn default() -> Self {
        Self {
            status: Arc::new(Mutex::new(Status::UnInitialized)),
            status_message: Arc::new(Mutex::new(String::new())),
            stations: Arc::new(Mutex::new(Stations::default())),
            boards: Arc::new(Mutex::new(Vec::new())),
            error_counter: Arc::new(Mutex::new(None)),
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
        connection: Arc<tokio::sync::Mutex<Client<Compat<TcpStream>>>>,
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
            let mut client = connection.lock().await;

            let query = Query::new(
                "SELECT Station, Program
                FROM dbo.SMT_AOI_RESULTS
                GROUP BY Station, Program",
            );

            if let Ok(mut result) = query.query(&mut client).await {
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
        connection: Arc<tokio::sync::Mutex<Client<Compat<TcpStream>>>>,
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
        let boards = self.boards.clone();

        *self.error_counter.lock().unwrap() = None;
        let error_counter = self.error_counter.clone();

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
            let mut client = connection.lock().await;

            /*
                SELECT Serial_NMBR, Date_Time, Station, Program, Variant, Operator, Result, "Data"
                FROM dbo.SMT_AOI_RESULTS
                WHERE
                Station =
                AND
                Program =
                AND
                Variant =
                AND
                Date_Time BETWEEN  x AND y
            */

            // Crafting the query
            let mut query_text = String::from(
                "SELECT Serial_NMBR, Date_Time, Station, Program, Variant, Operator, Result, [Data]
                FROM dbo.SMT_AOI_RESULTS
                WHERE ",
            );

            if !lines.is_empty() {
                let x = lines.iter().map(|f| format!("'{}'", f)).collect::<Vec<String>>().join(", ");
                query_text += &format!("Station IN ({x}) AND ");
            }

            if !programs.is_empty() {
                let x = programs.iter().map(|f| format!("'{}'", f)).collect::<Vec<String>>().join(", ");
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
                            boards.lock().unwrap().push(AOI_log_file::helpers::SingleBoard {
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

                let counter = AOI_log_file::helpers::ErrorCounter::generate(&boards.lock().unwrap());

                debug!(
                    "ErrorCounter: {} pseudo errors in {} boards",
                    counter.total_pseudo, counter.number_of_boards
                );

                *error_counter.lock().unwrap() = Some(counter);

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
        connection: Arc<tokio::sync::Mutex<Client<Compat<TcpStream>>>>,
    ) {
        if !self.initialized() {
            self.initialize(ctx, connection.clone());
            return;
        }

        
        let mut stations = self.stations.lock().unwrap();

        for line in &mut stations.lines {
            ui.horizontal(|ui| {
                ui.checkbox(&mut line.selected, &line.name);
            });
        }

        ui.separator();

        for product in &mut stations.products {
            ui.horizontal(|ui| {
                ui.checkbox(&mut product.selected, &product.name);
            });
        }

        
        drop(stations);


        ui.add_space(10.0);

        ui.vertical_centered(|ui| {
            if ui.button("Lekérdezés").clicked() {
                self.query(ctx, timeframe, connection.clone());
            }
        });
    }

    pub fn central_panel(&mut self, _ctx: &egui::Context, ui: &mut egui::Ui) {
        ui.style_mut().spacing.scroll = ScrollStyle::solid();
        ui.visuals_mut().collapsing_header_frame = true;     

        if *self.status.lock().unwrap() != Status::Standby {
            ui.vertical_centered(|ui| {
                ui.add(egui::Spinner::new().size(200.0));

                ui.label(self.status_message.lock().unwrap().as_str());
            });

            return;
        }

        if let Some(counter) = self.error_counter.lock().unwrap().as_ref() {

            
            egui::ScrollArea::vertical()
            .show(ui, |ui| {
                for program in &counter.inspection_plans {

                    let id = ui.make_persistent_id(&program.name);
                    egui::collapsing_header::CollapsingState::load_with_default_open(
                        ui.ctx(),
                        id,
                        false,
                    )
                    .show_header(ui, |ui| {
                        ui.label(&program.name);
                        ui.label(format!("({} pcb)", program.number_of_boards));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(program.total_pseudo.to_string());
                        });
                    }).body(|ui| {
                        for macro_name in &program.macros {
                            let id = ui.make_persistent_id(program.name.clone() + &macro_name.name);
                            egui::collapsing_header::CollapsingState::load_with_default_open(
                            ui.ctx(),
                            id,
                            false,
                        )
                        .show_header(ui, |ui| {
                            ui.label(&macro_name.name);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(macro_name.total_pseudo.to_string());
                                },
                            );
                        })
                        .body(|ui| {
                            for package in &macro_name.packages {
                                let id = ui.make_persistent_id(
                                    program.name.clone() + &macro_name.name + &package.name,
                                );
                                egui::collapsing_header::CollapsingState::load_with_default_open(
                                    ui.ctx(),
                                    id,
                                    false,
                                )
                                .show_header(ui, |ui| {
                                    ui.label(&package.name);
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(package.total_pseudo.to_string());
                                        },
                                    );
                                })
                                .body(|ui| {
                                    
                                        for position in &package.positions {
                                            ui.horizontal(|ui|{
                                                ui.label(&position.name);
                                                ui.with_layout(
                                                    egui::Layout::right_to_left(egui::Align::Center),
                                                    |ui| {
                                                        ui.label(position.total_pseudo.to_string());
                                                    },
                                                );
                                            });
                                            
                                        }
                                    
                                });
                            }
                        });
                        }
                    });
                }
            });
        }

        // show data
    }
}
