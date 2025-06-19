#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release
#![allow(non_snake_case)]

/*
TODO:
- logging
- send in partial frame
- check for gs
*/

use egui::{Color32, Context, RichText};
use log::{debug, error, info};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use tokio_stream::StreamExt;
use SQL::SQL;

use crate::config::Product;

mod config;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(egui::Vec2 { x: 600.0, y: 600.0 }),
        ..Default::default()
    };

    _ = eframe::run_native(
        format!("CCL interlock (v{VERSION})").as_str(),
        options,
        Box::new(|_| Ok(Box::new(App::new()?))),
    );

    Ok(())
}

struct App {
    config: config::Config,
    status: Arc<Mutex<Status>>,
    status_msg: Arc<Mutex<String>>,
    error_msg: Arc<Mutex<String>>,

    client: Arc<tokio::sync::Mutex<SQL>>,
    port: Arc<Mutex<Option<Box<dyn serialport::SerialPort + 'static>>>>,

    input_string: String,
    queue: VecDeque<String>,

    product: Option<Product>,
    serials: Arc<Mutex<Vec<Serial>>>,
}

#[derive(Debug, Copy, Clone, PartialEq)]
enum Status {
    UnInitialized,
    Initializing,
    Error,
    Standby,
    SendingEnable,
    Loading,
}

struct Serial {
    dmc: String,
    ict: TestResult,
    fct: TestResult,
}

impl Serial {
    fn frame(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_sized(
                (300.0, 70.0),
                egui::Label::new(RichText::new(&self.dmc).size(20.0)),
            );
            ui.add_space(10.0);
            self.ict.frame(ui);
            ui.add_space(10.0);
            self.fct.frame(ui);
        });
    }

    fn ok(&self) -> bool {
        self.ict.ok() && self.fct.ok()
    }

    fn nok(&self) -> bool {
        !self.ok()
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum TestResult {
    NotTested,
    None,
    Pass,
    Fail,
}

impl TestResult {
    fn ok(&self) -> bool {
        match self {
            TestResult::NotTested | TestResult::Pass => true,
            TestResult::None | TestResult::Fail => false,
        }
    }

    fn print(&self) -> &str {
        match self {
            TestResult::NotTested => "",
            TestResult::None => "N/A",
            TestResult::Pass => "OK",
            TestResult::Fail => "NOK",
        }
    }

    fn color(&self) -> Color32 {
        match self {
            TestResult::NotTested => Color32::GRAY,
            TestResult::None => Color32::YELLOW,
            TestResult::Pass => Color32::GREEN,
            TestResult::Fail => Color32::RED,
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "Passed" => Self::Pass,
            _ => Self::Fail,
        }
    }

    fn frame(&self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(self.color())
            .corner_radius(5)
            .inner_margin(5)
            .show(ui, |ui| {
                ui.add_sized(
                    (100.0, 50.0),
                    egui::Label::new(RichText::new(self.print()).color(Color32::BLACK).size(36.0)),
                );
            });
    }
}

impl App {
    fn new() -> anyhow::Result<Self> {
        let config = config::Config::load("ccl_config.json")?;

        let client = Arc::new(tokio::sync::Mutex::new(SQL::new(
            &config.sql_ip,
            &config.sql_db,
            &config.sql_user,
            &config.sql_pass,
        )?));

        let port = serialport::new(&config.serial_port, 9600)
            .data_bits(serialport::DataBits::Eight)
            .parity(serialport::Parity::None)
            .stop_bits(serialport::StopBits::One)
            .flow_control(serialport::FlowControl::None)
            .timeout(std::time::Duration::from_millis(10))
            .open()
            .ok();

        Ok(App {
            config,
            status: Arc::new(Mutex::new(Status::UnInitialized)),
            status_msg: Arc::new(Mutex::new(String::from("Initializing..."))),
            error_msg: Arc::new(Mutex::new(String::new())),
            client,
            port: Arc::new(Mutex::new(port)),
            input_string: String::new(),
            queue: VecDeque::new(),
            product: None,
            serials: Arc::new(Mutex::new(Vec::new())),
        })
    }

    // Connect to the SQL server
    fn init(&mut self, ctx: Context) {
        if *self.status.lock().unwrap() != Status::UnInitialized {
            return;
        }

        info!("Starting initialization");

        *self.status.lock().unwrap() = Status::Initializing;
        *self.status_msg.lock().unwrap() = String::from("Inicializáció...");

        let ctx = ctx.clone();
        let status = self.status.clone();
        let message = self.status_msg.clone();
        let client = self.client.clone();

        tokio::spawn(async move {
            loop {
                match client.lock().await.create_connection().await {
                    Ok(_) => break,
                    Err(e) => {
                        *status.lock().unwrap() = Status::Error;
                        *message.lock().unwrap() =
                            format!("Sikertelen csatlakozás az SQL szerverhez!\n({e:?})");
                    }
                }
            }

            *status.lock().unwrap() = Status::Standby;
            message.lock().unwrap().clear();

            ctx.request_repaint();
        });
    }

    fn send_enable(&self, ctx: Context) {
        debug!("Sending enable signal!");
        let port = self.port.clone();
        let serials = self.serials.clone();

        let err_message = self.error_msg.clone();
        let status = self.status.clone();
        *status.lock().unwrap() = Status::SendingEnable;

        tokio::spawn(async move {
            if let Some(p) = port.lock().unwrap().as_mut() {
                let buf = "Enable\r\n".as_bytes();
                if p.write(buf).is_err() {
                    *err_message.lock().unwrap() = "COM port error!".to_string();
                }
            }

            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            serials.lock().unwrap().clear();
            debug!("Sending disable signal!");

            if let Some(p) = port.lock().unwrap().as_mut() {
                let buf = "Disable\r\n".as_bytes();
                if p.write(buf).is_err() {
                    *err_message.lock().unwrap() = "COM port error!".to_string();
                }
            }

            *status.lock().unwrap() = Status::Standby;
            ctx.request_repaint();
        });
    }

    fn push(&mut self, serial: String, ctx: Context) {
        info!("Push: {serial}");

        let product = self.config.get_product(&serial);
        self.error_msg.lock().unwrap().clear();

        if product.is_none() {
            *self.error_msg.lock().unwrap() =
                String::from("Ismeretlen termék!\nUnknown product type!\nНевідомий тип товару!");
            return;
        }

        let product = product.unwrap();

        if let Some(p) = &self.product {
            if p.name != product.name {
                if self.serials.lock().unwrap().is_empty() {
                    debug!("Replacing with product: {}", product.name);
                    self.product = Some(product.clone());
                } else {
                    error!("Product type mismatch!");
                    *self.error_msg.lock().unwrap() =
                    String::from("A termék típus nem eggyezik!\nThe product types do not match!\nТипи товарів не збігаються!");
                    return;
                }
            }
        } else {
            debug!("Initializing new product: {}", product.name);
            self.product = Some(product.clone());
        }

        // check for duplicated serial numbers
        let serials_lock = self.serials.lock().unwrap();
        for s in serials_lock.iter() {
            if s.dmc == serial {
                return;
            }
        }
        drop(serials_lock);

        info!("Starting query");

        *self.status.lock().unwrap() = Status::Loading;
        *self.status_msg.lock().unwrap() = format!("{} lekérdezése...", self.input_string);

        let status = self.status.clone();
        let message = self.status_msg.clone();
        let err_message = self.error_msg.clone();
        let client = self.client.clone();
        let serials = self.serials.clone();

        tokio::spawn(async move {
            /*
            loop {
                match client.lock().await.check_connection().await {
                    true => break,
                    false => {
                        *status.lock().unwrap() = Status::Error;
                        *message.lock().unwrap() =
                            format!("Megszakadt a kapcsolat az SQL szerverhez!\nConnection to SQL server was terminated!");
                        let _ = client.lock().await.create_connection().await;
                    }
                }
            }
             */

            // Connection is OK.
            let mut c_lock = client.lock().await;
            let mut sql_client = c_lock.client().unwrap();

            // Query the serial for ICT and FCT
            let mut query = tiberius::Query::new(
                "SELECT TOP(1) Result
                FROM dbo.SMT_Test
                WHERE Serial_NMBR = @P1
                ORDER BY Date_Time DESC;

                SELECT TOP(1) Result
                FROM dbo.SMT_FCT_Test
                WHERE Serial_NMBR = @P1
                ORDER BY Date_Time DESC;",
            );
            query.bind(&serial);

            let mut ict_result = TestResult::None;
            let mut fct_result = if product.uses_fct {
                TestResult::None
            } else {
                TestResult::NotTested
            };
            if let Ok(mut qstream) = query.query(&mut sql_client).await {
                while let Some(row) = qstream.next().await {
                    let row = row.unwrap();
                    match row {
                        tiberius::QueryItem::Row(x) if x.result_index() == 0 => {
                            ict_result = TestResult::parse(x.get::<&str, usize>(0).unwrap());
                        }
                        tiberius::QueryItem::Row(x) if x.result_index() == 1 => {
                            fct_result = TestResult::parse(x.get::<&str, usize>(0).unwrap());
                        }
                        _ => {}
                    }
                }
            } else {
                *err_message.lock().unwrap() = String::from("SQL hiba!\nSQL error!");
            }

            serials.lock().unwrap().push(Serial {
                dmc: serial,
                ict: ict_result,
                fct: fct_result,
            });

            *status.lock().unwrap() = Status::Standby;
            message.lock().unwrap().clear();

            ctx.request_repaint();
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut request_clear = false;

        egui::TopBottomPanel::bottom("Input").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let text_edit = egui::TextEdit::singleline(&mut self.input_string)
                    .desired_width(400.0)
                    .show(ui);

                if text_edit.response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
                {
                    self.queue.push_back(self.input_string.clone());
                    self.input_string.clear();
                }

                text_edit.response.request_focus();

                if ui.button("Reset").clicked() && *self.status.lock().unwrap() == Status::Standby {
                    debug!("Requested reset by user!");
                    request_clear = true;
                }
            });
        });

        let board_num = self.serials.lock().unwrap().len() as u8;
        if *self.status.lock().unwrap() == Status::Standby
            && self
                .product
                .as_ref()
                .is_none_or(|f| f.boards_per_frame.gt(&board_num))
        {
            if let Some(next) = self.queue.pop_front() {
                self.push(next, ctx.clone());
            }
        } else if *self.status.lock().unwrap() == Status::SendingEnable {
            self.queue.clear();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.port.lock().unwrap().is_none() {
                ui.colored_label(Color32::RED, "COM port is not connected!");
            }

            let status = *self.status.lock().unwrap();
            match status {
                Status::UnInitialized => {
                    self.init(ctx.clone());
                }
                Status::Initializing | Status::Error => {
                    ui.vertical_centered(|ui| {
                        ui.add(egui::Spinner::new().size(200.0));
                        ui.label(self.status_msg.lock().unwrap().as_str());
                    });
                }
                status => {
                    if let Some(p) = &self.product {
                        ui.heading(&p.name);

                        ui.add_space(30.0);

                        let mut all_ok = true;

                        let serial_lock = self.serials.lock().unwrap();

                        for serial in serial_lock.iter() {
                            serial.frame(ui);
                            if serial.nok() {
                                all_ok = false;
                            }
                        }

                        if !all_ok {
                            ui.vertical_centered_justified(|ui| {
                                egui::Frame::new()
                                    .fill(Color32::RED)
                                    .corner_radius(5)
                                    .inner_margin(5)
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new(
                                                "Távolítsa el a hibás terméket!
                                                        Remove the defective product!
                                                        Видаліть бракований товар!
                                                        Alisin ang sira na produkto!",
                                            )
                                            .color(Color32::BLACK)
                                            .size(16.0),
                                        );

                                        ui.add_space(10.0);

                                        if ui.button(RichText::new("OK").size(20.0)).clicked() {
                                            request_clear = true;
                                        }
                                    });
                            });
                        }

                        if status == Status::Loading {
                            ui.vertical_centered(|ui| {
                                ui.add(egui::Spinner::new().size(100.0));
                            });
                        } else if serial_lock.len() >= p.boards_per_frame as usize
                            && !serial_lock.iter().any(|f| f.nok())
                        {
                            if status == Status::Standby {
                                self.send_enable(ctx.clone());
                            }
                            ui.vertical_centered_justified(|ui| {
                                egui::Frame::new()
                                    .fill(Color32::GREEN)
                                    .corner_radius(5)
                                    .inner_margin(10)
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new(
                                                "Nyomd meg a zöld gombot!
                                                Press the green button!
                                                Тисніть зелену кнопку!
                                                Pindutin ang berdeng pindutan!",
                                            )
                                            .color(Color32::BLACK)
                                            .size(16.0),
                                        );
                                    });
                            });
                        }
                    }
                }
            }

            // error message
            let err_lock = self.error_msg.lock().unwrap();
            if !err_lock.is_empty() {
                ui.vertical_centered_justified(|ui| {
                    egui::Frame::new()
                        .fill(Color32::RED)
                        .corner_radius(5)
                        .inner_margin(10)
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(err_lock.as_str())
                                    .color(Color32::BLACK)
                                    .size(16.0),
                            );
                        });
                });
            }
        });

        if request_clear {
            self.serials.lock().unwrap().clear();
            self.product = None;
            self.queue.clear();
        }

        ctx.request_repaint_after_secs(0.5);
    }
}
