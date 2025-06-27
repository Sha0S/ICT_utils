#![allow(non_snake_case)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use eframe::egui;
use egui_extras::{Column, TableBuilder};
use std::{env, path::PathBuf};

use ICT_log_file::*;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn load_icon() -> egui::IconData {
    let (icon_rgba, icon_width, icon_height) = {
        let icon = include_bytes!("..\\..\\..\\icons\\search.png");
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

fn main() {
    env_logger::init();

    let args: Vec<String> = env::args().collect();
    let log_path = args.get(1).expect("No arg found!");
    let log_path = PathBuf::from(log_path);

    if !log_path.exists() {
        panic!("File {} does not exist!", log_path.to_string_lossy());
    }

    let logs = LogFile::load_panel(&log_path).expect("Failed to load logfile!");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(egui::Vec2 {
                x: 1000.0,
                y: 500.0,
            })
            .with_icon(load_icon()),
        ..Default::default()
    };

    _ = eframe::run_native(
        &format!("ICT Log Reader (v{VERSION})"),
        options,
        Box::new(|_| Ok(Box::new(IctLogReader::default(logs)))),
    );
}

struct IctLogReader {
    failed_only: bool,
    search: String,
    selected_log: usize,
    logs: Vec<LogFile>,
}

impl IctLogReader {
    fn default(logs: Vec<LogFile>) -> Self {
        IctLogReader {
            failed_only: logs[0].get_status() != 0,
            search: String::new(),
            selected_log: 0,
            logs,
        }
    }
}

impl eframe::App for IctLogReader {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.logs[self.selected_log].has_report() {
            egui::SidePanel::right("Report")
                .default_width(300.0)
                .resizable(true)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(
                                    &mut self.logs[self.selected_log].get_report(),
                                )
                                .desired_width(f32::INFINITY),
                            );
                        });
                });
        }

        egui::TopBottomPanel::top("Board Data").show(ctx, |ui| {
            egui::Grid::new("board_stats").show(ui, |ui| {
                ui.monospace("Fájl:");
                ui.monospace(format!(
                    "{}",
                    self.logs[self.selected_log].get_source().to_string_lossy()
                ));
                ui.end_row();

                ui.monospace("Termék:");
                ui.monospace(self.logs[self.selected_log].get_product_id());
                ui.end_row();

                ui.monospace("Fő DMC:");
                ui.monospace(self.logs[self.selected_log].get_main_DMC());
                ui.end_row();

                ui.monospace("DMC:");
                egui::ComboBox::from_id_salt("select_log")
                    .width(300.0)
                    .selected_text(format!(
                        "{} - {}",
                        self.selected_log,
                        self.logs[self.selected_log].get_DMC()
                    ))
                    .show_ui(ui, |ui| {
                        for (i, log) in self.logs.iter().enumerate() {
                            ui.selectable_value(
                                &mut self.selected_log,
                                i,
                                format!("{} - {}", i, log.get_DMC()),
                            );
                        }
                    });

                ui.end_row();

                ui.monospace("Teszt ideje:");
                ui.monospace(format!(
                    "{} - {}",
                    self.logs[self.selected_log]
                        .get_time_start()
                        .format("%Y-%m-%d %H:%M:%S"),
                    self.logs[self.selected_log]
                        .get_time_end()
                        .format("%Y-%m-%d %H:%M:%S")
                ));
                ui.end_row();

                ui.monospace("Eredmény:");
                ui.monospace(format!(
                    "{} - {}",
                    self.logs[self.selected_log].get_status(),
                    self.logs[self.selected_log].get_status_str()
                ));
                ui.end_row();
            });

            ui.separator();

            ui.horizontal(|ui| {
                ui.monospace("Keresés: ");
                ui.text_edit_singleline(&mut self.search);
                ui.checkbox(&mut self.failed_only, "Csak a kiesőket mutassa")
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
            TableBuilder::new(ui)
                .striped(true)
                .column(Column::initial(200.0).resizable(true))
                .column(Column::initial(30.0))
                .columns(Column::initial(100.0), 4)
                .header(16.0, |mut header| {
                    header.col(|ui| {
                        ui.label("Teszt");
                    });
                    header.col(|ui| {
                        ui.label(" ");
                    });
                    header.col(|ui| {
                        ui.label("Mért");
                    });
                    header.col(|ui| {
                        ui.label("Alsó határ");
                    });
                    header.col(|ui| {
                        ui.label("Középérték");
                    });
                    header.col(|ui| {
                        ui.label("Felső határ");
                    });
                })
                .body(|body| {
                    let selected_tests: Vec<&Test> = self.logs[self.selected_log]
                        .get_tests()
                        .iter()
                        .filter(|f| {
                            if self.failed_only {
                                f.get_name().contains(&self.search)
                                    && f.get_result().0 == BResult::Fail
                            } else {
                                f.get_name().contains(&self.search)
                            }
                        })
                        .collect();
                    let total_rows = selected_tests.len();

                    body.rows(14.0, total_rows, |mut row| {
                        let row_index = row.index();
                        if let Some(test) = selected_tests.get(row_index) {
                            row.col(|ui| {
                                ui.label(test.get_name());
                            });

                            let result = test.get_result();
                            row.col(|ui| {
                                ui.label(result.0.print());
                            });
                            row.col(|ui| {
                                ui.label(format!("{:+1.4E}", result.1));
                            });

                            match test.get_limits() {
                                TLimit::None => {}
                                TLimit::Lim2(u, l) => {
                                    row.col(|ui| {
                                        ui.label(format!("{:+1.4E}", l));
                                    });
                                    row.col(|_ui| {});
                                    row.col(|ui| {
                                        ui.label(format!("{:+1.4E}", u));
                                    });
                                }
                                // Nom - UL - LL
                                TLimit::Lim3(n, u, l) => {
                                    row.col(|ui| {
                                        ui.label(format!("{:+1.4E}", l));
                                    });
                                    row.col(|ui| {
                                        ui.label(format!("{:+1.4E}", n));
                                    });
                                    row.col(|ui| {
                                        ui.label(format!("{:+1.4E}", u));
                                    });
                                }
                            }
                        }
                    });
                });
        });
    }
}
