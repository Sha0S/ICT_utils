use chrono::{DateTime, Local, NaiveDate, NaiveTime};
use rust_xlsxwriter::*;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
};

use crate::LogFileHandler;
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn u64_to_hours(mut x: u64) -> String {
    x %= u64::pow(10, 2);

    format!("{0:02.0}:00 - {0:02.0}:59", x)
}

fn is_dir_in_t(s: &Path, start: DateTime<Local>, end: DateTime<Local>) -> bool {
    if let Ok(as_time) =
        NaiveDate::parse_from_str(s.file_name().unwrap().to_str().unwrap(), "%Y_%m_%d")
    {
        if start.date_naive() <= as_time && end.date_naive() >= as_time {
            return true;
        }
    }
    false
}

fn get_logs_in_path_t(
    p: &Path,
    start: DateTime<Local>,
    end: DateTime<Local>,
) -> Result<Vec<(PathBuf, u64)>, std::io::Error> {
    let mut ret: Vec<(PathBuf, u64)> = Vec::new();

    for file in fs::read_dir(p)? {
        let file = file?;
        let path = file.path();
        if path.is_dir() {
            if is_dir_in_t(&path, start, end) {
                ret.append(&mut get_logs_in_path_t(&path, start, end)?);
            }
        } else if let Ok(x) = path.metadata() {
            let ct: DateTime<Local> = x.modified().unwrap().into();
            if ct >= start && ct < end {
                ret.push((path.to_path_buf(), x.len()));
            }
        }
    }

    Ok(ret)
}

fn write_header(worksheet: &mut Worksheet, date: NaiveDate) -> Result<u32, XlsxError> {
    let format = Format::new().set_num_format("yyyy-mm-dd");
    worksheet.set_column_width_pixels(0, 120)?;
    worksheet.set_column_width_pixels(1, 100)?;
    worksheet.set_column_width_pixels(2, 100)?;
    worksheet.set_column_width_pixels(3, 100)?;
    worksheet.set_column_width_pixels(4, 100)?;
    worksheet.set_column_width_pixels(5, 300)?;
    worksheet.set_column_width_pixels(6, 200)?;
    worksheet.set_column_width_pixels(7, 50)?;
    worksheet.set_column_width_pixels(8, 300)?;

    worksheet.write(0, 0, "report generated:")?;
    worksheet.write_datetime_with_format(0, 1, Local::now().date_naive(), &format)?;

    worksheet.write(1, 0, "day reported:")?;
    worksheet.write_datetime_with_format(1, 1, date, &format)?;

    worksheet.write(0, 4, "sw version:")?;
    worksheet.write(0, 5, VERSION)?;

    worksheet.write(3, 0, "Product:")?;

    Ok(3)
}

fn write_product(
    worksheet: &mut Worksheet,
    lfh: LogFileHandler,
    mut row: u32,
) -> Result<u32, XlsxError> {
    let merge_format = Format::new()
        .set_bold()
        .set_border_bottom(FormatBorder::Medium);
    let header_format = Format::new().set_border_bottom(FormatBorder::Thin);
    let footer_format = Format::new().set_border_top(FormatBorder::Thin);

    worksheet.merge_range(row, 1, row, 8, &lfh.get_product_id(), &merge_format)?;
    row += 2;
    let mut row_b = row;

    // Vec<(time in yymmddhh, total ok, total nok, Vec<(result, mmss)> )>
    let prod_hourly = lfh.get_hourly_mb_stats();

    worksheet.write_blank(row, 1, &header_format)?;
    worksheet.write_with_format(row, 2, "OK", &header_format)?;
    worksheet.write_with_format(row, 3, "NOK", &header_format)?;
    row += 1;

    let mut total_ok = 0;
    let mut total_nok = 0;
    for hour in prod_hourly {
        worksheet.write(row, 1, u64_to_hours(hour.0))?;
        worksheet.write(row, 2, hour.1 as u32)?;
        worksheet.write(row, 3, hour.2 as u32)?;

        total_ok += hour.1 as u32;
        total_nok += hour.2 as u32;
        row += 1;
    }

    worksheet.write_blank(row, 1, &footer_format)?;
    worksheet.write_with_format(row, 2, total_ok, &footer_format)?;
    worksheet.write_with_format(row, 3, total_nok, &footer_format)?;
    row += 3;

    let failure_list_all = lfh.get_failures(crate::FlSettings::All);
    let failure_list_retest = lfh.get_failures(crate::FlSettings::AfterRetest);

    worksheet.write_with_format(row, 1, "Failed tests", &header_format)?;
    worksheet.write_with_format(row, 2, "All", &header_format)?;
    worksheet.write_with_format(row, 3, "After retest", &header_format)?;
    row += 1;

    let mut total_nok = 0;
    let mut total_nok_art = 0;
    for fail in failure_list_all {
        worksheet.write(row, 1, &fail.name)?;
        worksheet.write(row, 2, fail.total as u32)?;

        if let Some(x) = failure_list_retest.iter().find(|f| f.name == fail.name) {
            worksheet.write(row, 3, x.total as u32)?;
            total_nok_art += x.total as u32;
        }

        total_nok += fail.total as u32;
        row += 1;
    }

    worksheet.write_blank(row, 1, &footer_format)?;
    worksheet.write_with_format(row, 2, total_nok, &footer_format)?;
    worksheet.write_with_format(row, 3, total_nok_art, &footer_format)?;
    row += 3;

    let failed_boards = lfh.get_failed_boards();

    worksheet.write_with_format(row_b, 5, "Failed boards", &header_format)?;
    worksheet.write_with_format(row_b, 6, "Time", &header_format)?;
    worksheet.write_with_format(row_b, 7, "Result", &header_format)?;
    worksheet.write_with_format(row_b, 8, "Failed tests", &header_format)?;
    row_b += 1;

    let mut last_DMC = String::new();
    for board in failed_boards {
        if board.0 != last_DMC {
            worksheet.write(row_b, 5, &board.0)?;
            last_DMC = board.0
        }

        worksheet.write(row_b, 6, crate::u64_to_string(board.1))?;
        worksheet.write(row_b, 7, board.2.print())?;
        worksheet.write(row_b, 8, board.3.join(", "))?;
        row_b += 1;
    }
    row_b += 2;

    Ok(row.max(row_b))
}
pub struct DailyYieldWindow {
    enabled: bool,
    running: Arc<Mutex<bool>>,
    date: NaiveDate,
    out_path: String,
    output_message: Arc<Mutex<String>>,
    path_list: Vec<PathBuf>,
}

impl DailyYieldWindow {
    pub fn default(path_list: Vec<PathBuf>) -> Self {
        DailyYieldWindow {
            enabled: false,
            running: Arc::new(Mutex::new(false)),
            date: Local::now().date_naive().pred_opt().unwrap(),
            out_path: ".\\out.xlsx".to_string(),
            output_message: Arc::new(Mutex::new(String::new())),
            path_list,
        }
    }

    pub fn enable(&mut self) {
        self.enabled = true;
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    fn generate_report(&mut self, ctx: &egui::Context) {
        let running_lock = self.running.clone();
        let output_lock = self.output_message.clone();
        let paths = self.path_list.clone();
        let start_t = self
            .date
            .and_time(NaiveTime::from_hms_opt(6, 0, 0).unwrap())
            .and_local_timezone(Local)
            .unwrap();
        let end_t = self
            .date
            .succ_opt()
            .unwrap()
            .and_time(NaiveTime::from_hms_opt(6, 0, 0).unwrap())
            .and_local_timezone(Local)
            .unwrap();
        let context = ctx.clone();
        let out_path = self.out_path.clone();

        thread::spawn(move || {
            *running_lock.lock().unwrap() = true;
            output_lock
                .lock()
                .unwrap()
                .push_str("Starting report generation...\n");
            context.request_repaint();

            let mut output_row: u32 = 0;
            let mut workbook = Workbook::new();
            let worksheet = workbook.add_worksheet();

            match write_header(worksheet, start_t.date_naive()) {
                Ok(x) => {
                    output_row = x;
                }
                Err(x) => {
                    output_lock
                        .lock()
                        .unwrap()
                        .push_str(&format!("ERR: Failed to write header:\n {} \n", x));
                }
            }

            for path in paths {
                output_lock
                    .lock()
                    .unwrap()
                    .push_str(&format!("Scanning directory: {}\n", path.to_string_lossy()));
                context.request_repaint();

                if path.exists() {
                    if let Ok(logs) = get_logs_in_path_t(&path, start_t, end_t) {
                        output_lock
                            .lock()
                            .unwrap()
                            .push_str(&format!("\tFound {} logs.\n", logs.len()));
                        context.request_repaint();

                        let mut lfh = LogFileHandler::new();
                        for (log, _) in logs {
                            lfh.push_from_file(&log);
                        }
                        lfh.update();

                        if !lfh.is_empty() {
                            match write_product(worksheet, lfh, output_row) {
                                Ok(x) => {
                                    output_row = x;
                                }
                                Err(x) => {
                                    output_lock.lock().unwrap().push_str(&format!(
                                        "ERR: Failed to write product:\n {} \n",
                                        x
                                    ));
                                }
                            }
                        }
                    } else {
                        output_lock
                            .lock()
                            .unwrap()
                            .push_str("\tERR: Failed to read directory!\n");
                        context.request_repaint();
                    }
                } else {
                    output_lock
                        .lock()
                        .unwrap()
                        .push_str("\tERR: Directory not found!\n");
                    context.request_repaint();
                }
            }

            if let Err(x) = workbook.save(out_path) {
                output_lock
                    .lock()
                    .unwrap()
                    .push_str(&format!("ERR: Failed to write output:\n {} \n", x));
            } else {
                output_lock
                    .lock()
                    .unwrap()
                    .push_str("Writing output is succesfull!\n");
            }

            *running_lock.lock().unwrap() = false;
            context.request_repaint();
        });
    }

    pub fn update(&mut self, ctx: &egui::Context) {
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("DYWindow"),
            egui::ViewportBuilder::default()
                .with_title("Daily Yield")
                .with_inner_size([400.0, 400.0]),
            |ctx, class| {
                assert!(
                    class == egui::ViewportClass::Immediate,
                    "This egui backend doesn't support multiple viewports"
                );

                egui::TopBottomPanel::top("DatePicker").show(ctx, |ui| {
                    ui.set_enabled(!*self.running.lock().unwrap());
                    ui.horizontal(|ui| {
                        ui.monospace("Date: ");
                        ui.add(
                            egui_extras::DatePickerButton::new(&mut self.date)
                                .id_source("Date to scan"),
                        );

                        if ui.button("Run").clicked() {
                            self.generate_report(ctx);
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut self.out_path);

                        if ui.button("📁").clicked() {
                            if let Some(input_path) = rfd::FileDialog::new()
                                .add_filter("xlsx", &["xlsx"])
                                .save_file()
                            {
                                self.out_path = input_path.to_string_lossy().to_string();
                            }
                        }
                    });
                });

                egui::CentralPanel::default().show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(
                                    &mut self.output_message.lock().unwrap().as_str(),
                                )
                                .desired_width(f32::INFINITY),
                            );
                        });
                });

                if ctx.input(|i| i.viewport().close_requested()) {
                    self.enabled = false;
                }
            },
        );
    }
}
