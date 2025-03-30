#![warn(clippy::all, rust_2018_idioms)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use chrono::NaiveDate;

mod connection;
mod smt_stations;
use smt_stations as SMT;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() -> eframe::Result {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "smt_yield=info");
    }

    env_logger::init(); // Log to stderr (if you run with `RUST_LOG=debug`).

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 300.0])
            .with_min_inner_size([700.0, 220.0]),
        ..Default::default()
    };

    eframe::run_native(
        &format!("SMT yield checker ({VERSION})"),
        native_options,
        Box::new(|cc| Ok(Box::new(SmtYieldApp::new(cc)))),
    )?;


    Ok(())
}

#[derive(Debug)]
struct SmtYieldApp {
    start_date: NaiveDate,
    start_time: (u32, u32), // hours, minutes

    use_end_date: bool,
    end_date: NaiveDate,
    end_time: (u32, u32), // hours, minutes

    station_handler: SMT::StationHandler,
}

impl SmtYieldApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self { 
            start_date: chrono::Local::now().date_naive(), 
            start_time: (0,0), 
            use_end_date: false, 
            end_date: chrono::Local::now().date_naive(), 
            end_time: (23,59), 
            station_handler: SMT::StationHandler::new() 
        }
    }
}

// start_date, start_time, use_end_date, end_date, end_time
type TimeFrame<'a> = (&'a NaiveDate, &'a (u32, u32), &'a bool, &'a NaiveDate, &'a (u32, u32));

impl eframe::App for SmtYieldApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {



        egui::SidePanel::left("settings_panel")
            .exact_width(250.0)
            .show(ctx, |ui| {

                // start time
                ui.horizontal(|ui| {
                    ui.add_space(26.0);
                    ui.add(
                        egui_extras::DatePickerButton::new(&mut self.start_date)
                            .id_salt("start_date"),
                    );

                    ui.add(
                        egui::DragValue::new(&mut self.start_time.0)
                            .speed(1.0)
                            .range(0..=23),
                    );
                    ui.label(":");
                    ui.add(
                        egui::DragValue::new(&mut self.start_time.1)
                            .speed(1.0)
                            .range(0..=59),
                    );
                });

                // end time
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.use_end_date, "");
                    ui.add_enabled_ui(self.use_end_date, |ui| {
                        ui.add(
                            egui_extras::DatePickerButton::new(&mut self.end_date)
                                .id_salt("end_date"),
                        );

                        ui.add(
                            egui::DragValue::new(&mut self.end_time.0)
                                .speed(1.0)
                                .range(0..=23),
                        );
                        ui.label(":");
                        ui.add(
                            egui::DragValue::new(&mut self.end_time.1)
                                .speed(1.0)
                                .range(0..=59),
                        );
                    });
                });

                // station type and line number
                ui.horizontal(|ui| {
                    ui.add_space(26.0);
                    let mut new_station = None;
                    // pick station type (AOI, ICT, etc...)
                    egui::ComboBox::from_id_salt("Station")
                        .width(50.0)
                        .selected_text(self.station_handler.print_selected_station())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut new_station,
                                Some(SMT::Station::Aoi),
                                "AOI",
                            );
                            ui.selectable_value(
                                &mut new_station,
                                Some(SMT::Station::Ict),
                                "ICT",
                            );
                            ui.selectable_value(
                                &mut new_station,
                                Some(SMT::Station::Fct),
                                "FCT",
                            );
                        });

                    if let Some(ns) = new_station {
                        self.station_handler.change_station(ns);
                    }
                });

                ui.separator();

                // station specific settings
                let timeframe: TimeFrame<'_> = (&self.start_date, &self.start_time, &self.use_end_date, &self.end_date, &self.end_time);
                self.station_handler.side_panel(ctx, ui, timeframe);
                ui.separator();
            });

            egui::CentralPanel::default().show(ctx, |ui| {
                self.station_handler.central_panel(ctx, ui);
            });
    }
}
