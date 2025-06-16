#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release
#![allow(non_snake_case)]

use egui::{Color32, Context, RichText};
use log::{debug, error, info};
use std::{collections::VecDeque, sync::{Arc, Mutex}};
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
    port: Arc<Mutex<Box<dyn serialport::SerialPort + 'static>>>,

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
                (300.0, 80.0),
                egui::Label::new(RichText::new(&self.dmc).size(20.0)),
            );
            ui.add_space(10.0);
            self.ict.frame(ui);
            ui.add_space(10.0);
            self.fct.frame(ui);
        });
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
            .inner_margin(10)
            .show(ui, |ui| {
                ui.add_sized(
                    (100.0, 50.0),
                    egui::Label::new(RichText::new(self.print()).color(Color32::BLACK).size(40.0)),
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
            .expect("Failed to open port");

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

        let port = self.port.clone();
        let serials = self.serials.clone();

        tokio::spawn(async move {
            let buf = "Enable\r\n".as_bytes();
            port.lock().unwrap().write(buf).unwrap();

            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            serials.lock().unwrap().clear();

            let buf = "Disable\r\n".as_bytes();
            port.lock().unwrap().write(buf).unwrap();
            ctx.request_repaint();
        });
    }    

    fn push(&mut self, serial: String, ctx: Context) {
        if *self.status.lock().unwrap() != Status::Standby {
            return;
        }

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
                *self.error_msg.lock().unwrap() =
                    String::from("A termék típus nem eggyezik!\nThe product types do not match!\nТипи товарів не збігаються!");
                return;
            }
        } else {
            self.product = Some(product.clone());
        }

        // check for duplicated serial numbers
        let serials_lock = self.serials.lock().unwrap();
        for s in serials_lock.iter() {
            if s.dmc == serial {
                /*
                *self.error_msg.lock().unwrap() =
                    format!("{serial}:\nA keretben már szerepel egy azonos DMC!\nFrame already contains this DMC!\nРамка вже містить цей DMC!");
                */
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

            // Connection is OK.
            let mut c_lock = client.lock().await;
            let mut sql_client = c_lock.client().unwrap();

            // Query the serial for ICT
            let mut query = tiberius::Query::new(
                "SELECT TOP(1) Result
                FROM SMT_Test 
                WHERE Serial_NMBR = @P1 
                ORDER BY Date_Time DESC",
            );
            query.bind(&serial);

            let mut ict_result = TestResult::None;
            if let Ok(qstream) = query.query(&mut sql_client).await {
                if let Some(row) = qstream.into_row().await.unwrap() {
                    ict_result = TestResult::parse(row.get::<&str, usize>(0).unwrap());
                }
            } else {
                *err_message.lock().unwrap() = String::from("SQL hiba!\nSQL error!");
            }

            // Query the serial for FCT, if the product uses it.
            let mut fct_result = TestResult::NotTested;
            if product.uses_fct {
                let mut query = tiberius::Query::new(
                    "SELECT TOP(1) Result
                    FROM SMT_FCT_Test 
                    WHERE Serial_NMBR = @P1 
                    ORDER BY Date_Time DESC",
                );
                query.bind(&serial);

                if let Ok(qstream) = query.query(&mut sql_client).await {
                    if let Some(row) = qstream.into_row().await.unwrap() {
                        fct_result = TestResult::parse(row.get::<&str, usize>(0).unwrap());
                    } else {
                        fct_result = TestResult::None;
                    }
                } else {
                    *err_message.lock().unwrap() = String::from("SQL hiba!\nSQL error!");
                }
            }

            if ict_result == TestResult::Pass {
                if fct_result == TestResult::Pass || fct_result == TestResult::NotTested {
                    serials.lock().unwrap().push(Serial {
                        dmc: serial,
                        ict: ict_result,
                        fct: fct_result,
                    });
                } else {
                    *err_message.lock().unwrap() = format!(
                        "{serial}:\nA termék FCT NOK!\nThe product is FCT NOK\nПродукт є FCT NOK!"
                    );
                }
            } else {
                *err_message.lock().unwrap() = format!(
                    "{serial}:\nA termék ICT NOK!\nThe product is ICT NOK\nПродукт є ICT NOK!"
                );
            }

            *status.lock().unwrap() = Status::Standby;
            message.lock().unwrap().clear();

            ctx.request_repaint();
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::bottom("Input").show(ctx, |ui| {
            let mut text_edit = egui::TextEdit::singleline(&mut self.input_string)
                .desired_width(550.0)
                .show(ui);

            if text_edit.response.lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
            {
                self.queue.push_back(self.input_string.clone());
                self.input_string.clear();
                text_edit.response.request_focus();
            }
        });

        if *self.status.lock().unwrap() == Status::Standby {
            if let Some(next) = self.queue.pop_front() {
                self.push(next, ctx.clone());
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
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

                        let serial_lock = self.serials.lock().unwrap();

                        for serial in serial_lock.iter() {
                            serial.frame(ui);
                        }

                        if status == Status::Loading {
                            ui.vertical_centered(|ui| {
                                ui.add(egui::Spinner::new().size(100.0));
                            });
                        } else if serial_lock.len() >= p.boards_per_frame as usize {
                            self.send_enable(ctx.clone());
                            ui.vertical_centered_justified(|ui| {
                                egui::Frame::new()
                                    .fill(Color32::GREEN)
                                    .corner_radius(5)
                                    .inner_margin(10)
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new(
                                                "Nyomd meg a zöld gombot!\nPress the green button!\nТисніть зелену кнопку!\nPindutin ang berdeng pindutan!"
                                            )
                                                .color(Color32::BLACK)
                                                .size(16.0),
                                        );
                                    });
                            });
                        }

                        if serial_lock.is_empty() && status != Status::Loading {
                            self.product = None;
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
            drop(err_lock);
        });
    }
}
