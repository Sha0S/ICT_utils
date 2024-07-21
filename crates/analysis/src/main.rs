#![allow(non_snake_case)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use eframe::egui;
use egui::{Color32, ImageButton, Layout, ProgressBar, RichText, Sense, Stroke, Vec2};
use egui_dropdown::DropDownBox;
use egui_extras::{Column, TableBuilder};
use egui_plot::{Bar, BarChart, Line, Plot, PlotPoints};

use chrono::*;

use ICT_config::*;
use ICT_log_file::*;

mod log_info_window;
use log_info_window::*;

mod scan_dir;
use scan_dir::*;

mod daily_yield;
use daily_yield::*;

use std::fs;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::thread;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const PRODUCT_LIST: &str = ".\\products";
include!("locals.rs");

/*
Currently in the _t functions it checks if the last modification to the files is between the limits.
This wasn't the original behaviour, but it should be fine? It is also really fast.
*/

fn get_logs_in_path(
    p: &Path,
    pm_lock: Arc<RwLock<u32>>,
) -> Result<Vec<(PathBuf, u64)>, std::io::Error> {
    let mut ret: Vec<(PathBuf, u64)> = Vec::new();

    for file in fs::read_dir(p)? {
        let file = file?;
        let path = file.path();
        if path.is_dir() {
            ret.append(&mut get_logs_in_path(&path, pm_lock.clone())?);
        } else if let Ok(x) = path.metadata() {
            ret.push((path.to_path_buf(), x.len()));
            *pm_lock.write().unwrap() += 1;
        }
    }

    Ok(ret)
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

fn move_file_to_subdir(base_dir: &Path, subdir_name: String, file: &Path) -> std::io::Result<()> {
    let new_dir = base_dir.join(subdir_name);
    if !new_dir.exists() {
        fs::create_dir(&new_dir)?;
    }
    let final_path = new_dir.join(file.file_name().unwrap());
    println!("Moving {} to {}", file.display(), final_path.display());

    fs::rename(file, final_path)?;

    Ok(())
}

// For AutoUpdater. Grabs files after time 't', but it will not scan subdirectories
type PathAndTime = (PathBuf, DateTime<Local>);

fn get_logs_after_t(
    base_path: &Path,
    t: DateTime<Local>,
) -> Result<Vec<PathAndTime>, std::io::Error> {
    let mut ret: Vec<PathAndTime> = Vec::new();
    let now = Local::now();

    for file in fs::read_dir(base_path)? {
        let file = file?;
        let path = file.path();
        let path_ext = path.extension();
        if path.is_file() && (path_ext.is_none() || path_ext.is_some_and(|f| f == "ict")) {
            if let Ok(x) = path.metadata() {
                let ct: DateTime<Local> = x.modified().unwrap().into();
                if ct > t {
                    ret.push((path.to_path_buf(), ct));
                } else if now - ct > Duration::try_hours(4).unwrap() {
                    // if the log is older than 4 hours, then move it to a subdir
                    move_file_to_subdir(base_path, format!("{}", ct.format("%Y_%m_%d")), &path)?;
                }
            }
        }
    }

    ret.sort_by_key(|k| k.1);

    Ok(ret)
}

// Turn YYMMDDHH format u64 int to "YY.MM.DD HH:00 - HH:59"
fn u64_to_timeframe(mut x: u64) -> String {
    let y = x / u64::pow(10, 6);
    x %= u64::pow(10, 6);

    let m = x / u64::pow(10, 4);
    x %= u64::pow(10, 4);

    let d = x / u64::pow(10, 2);
    x %= u64::pow(10, 2);

    format!(
        "{0:02.0}.{1:02.0}.{2:02.0} {3:02.0}:00 - {3:02.0}:59",
        y, m, d, x
    )
}

fn load_icon() -> egui::IconData {
    let (icon_rgba, icon_width, icon_height) = {
        let icon = include_bytes!("..\\..\\..\\icons\\info.png");
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

fn main() -> Result<(), eframe::Error> {
    env_logger::init(); // Log to stderr (if you run with `RUST_LOG=debug`).

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(Vec2 { x: 830.0, y: 450.0 })
            .with_icon(load_icon()),
        ..Default::default()
    };

    eframe::run_native(
        format!("ICT Analysis (v{VERSION})").as_str(),
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Box::<MyApp>::default()
        }),
    )
}

#[derive(PartialEq)]
enum AppMode {
    None,
    Plot,
    Hourly,
    Multiboards,
    Export,
}

#[derive(PartialEq)]
enum YieldMode {
    SingleBoard,
    MultiBoard,
}
enum LoadMode {
    Folder(PathBuf),
    ProductList(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum AUState {
    Standby,
    Loading,
    Loaded,
}

struct AutoUpdate {
    usable: bool,
    enabled: bool,
    state: Arc<RwLock<AUState>>,

    product: Option<usize>,
    last_log: Option<DateTime<Local>>,
    update_start_time: Option<DateTime<Local>>,
    last_scan_time: Option<DateTime<Local>>,

    log_buffer: Arc<RwLock<Vec<PathAndTime>>>,
}

/*
 Standby -> its_time --Loading--> gather_logs --Loaded--> push_logs -> Standby
*/

impl AutoUpdate {
    fn default() -> Self {
        AutoUpdate {
            usable: false,
            enabled: false,
            state: Arc::new(RwLock::new(AUState::Standby)),
            product: None,
            last_log: None,
            update_start_time: None,
            last_scan_time: None,

            log_buffer: Arc::new(RwLock::new(Vec::new())),
        }
    }

    fn clear(&mut self) {
        self.usable = false;
        self.enabled = false;
        self.state = Arc::new(RwLock::new(AUState::Standby));
        self.product = None;
        self.last_log = None;
        self.update_start_time = None;
        self.last_scan_time = None;
        self.log_buffer.write().unwrap().clear();
    }

    fn state(&self) -> AUState {
        *self.state.read().unwrap()
    }

    fn its_time(&self) -> bool {
        if self.enabled && *self.state.read().unwrap() == AUState::Standby {
            if let Some(t) = self.last_scan_time {
                return (Local::now() - t).num_seconds() > 30;
            }
        }

        false
    }

    fn gather_logs(&mut self, products: &[Product]) {
        if let Some(prod) =
            products.get(self.product.expect("ERR: Auto Updater has no product ID!"))
        {
            self.update_start_time = Some(Local::now());
            let state_lock = self.state.clone();
            let log_lock = self.log_buffer.clone();
            let path = prod.get_log_dir().clone();

            // ToDo:
            // Idealy we would get last-log from the last manual load.
            // That would need the re-write of the fn.
            let start = if let Some(x) = self.last_log {
                x - Duration::try_seconds(5).unwrap()
            } else {
                self.last_scan_time.unwrap() - Duration::try_minutes(5).unwrap()
            };

            thread::spawn(move || {
                *state_lock.write().unwrap() = AUState::Loading;
                if let Ok(logs) = get_logs_after_t(&path, start) {
                    *log_lock.write().unwrap() = logs;
                }

                *state_lock.write().unwrap() = AUState::Loaded;
            });
        }
    }

    fn push_logs(&mut self, lfh: Arc<RwLock<LogFileHandler>>) -> (Duration, usize) {
        if self.state() != AUState::Loaded {
            panic!("ERR: AutoUpdate -> Push logs called at wrong time!");
        }

        let mut new_logs: usize = 0;

        for log in self.log_buffer.read().unwrap().iter() {
            if lfh.write().unwrap().push_from_file(&log.0) {
                new_logs += 1;
            }
        }

        if let Some((_, x)) = self.log_buffer.read().unwrap().last() {
            self.last_log = Some(*x);
        }

        self.log_buffer.write().unwrap().clear();
        self.last_scan_time = Some(Local::now());
        *self.state.write().unwrap() = AUState::Standby;
        let update_time = Local::now() - self.update_start_time.unwrap();

        println!("Autoupdate done in {update_time}, new logs: {new_logs}");
        (update_time, new_logs)
    }
}

struct MyApp {
    status: String,
    lang: usize,
    selected_product: usize,
    product_list: Vec<Product>,
    log_master: Arc<RwLock<LogFileHandler>>,

    date_start: NaiveDate,
    date_end: NaiveDate,

    time_start: NaiveTime,
    time_start_string: String,
    time_end: NaiveTime,
    time_end_string: String,
    time_end_use: bool,

    auto_update: AutoUpdate,

    loading: bool,
    progress_x: Arc<RwLock<u32>>,
    progress_m: Arc<RwLock<u32>>,

    yield_mode: YieldMode,
    yields: [Yield; 3],
    mb_yields: [Yield; 3],
    fl_setting: FlSettings,
    failures: Vec<FailureList>,
    limitchanges: Option<Vec<(usize, String)>>,

    mode: AppMode,

    hourly_stats: Vec<HourlyStats>,
    hourly_gs: bool,
    hourly_boards: bool,

    multiboard_results: Vec<MbStats>,

    selected_test: usize,
    selected_test_buf: String,
    selected_test_index: usize,
    selected_test_results: (TType, Vec<(u64, usize, TResult, TLimit)>),

    export_settings: ExportSettings,

    info_vp: LogInfoWindow,
    scan_vp: ScanDirWindow,
    daily_yield_vp: DailyYieldWindow,
}

impl Default for MyApp {
    fn default() -> Self {
        let time_start = NaiveTime::from_hms_opt(0, 0, 0).unwrap();
        let time_end = NaiveTime::from_hms_opt(23, 59, 59).unwrap();

        let product_list = load_product_list(PRODUCT_LIST, false);

        let path_list: Vec<PathBuf> = product_list
            .iter()
            .map(|f| f.get_log_dir().clone())
            .collect();

        Self {
            status: "".to_owned(),
            lang: 0,
            product_list,
            selected_product: 0,
            log_master: Arc::new(RwLock::new(LogFileHandler::new())),

            date_start: Local::now().date_naive(),
            date_end: Local::now().date_naive(),

            time_start,
            time_start_string: time_start.format("%H:%M:%S").to_string(),
            time_end,
            time_end_string: time_end.format("%H:%M:%S").to_string(),
            time_end_use: false,

            auto_update: AutoUpdate::default(),

            loading: false,
            progress_x: Arc::new(RwLock::new(0)),
            progress_m: Arc::new(RwLock::new(1)),

            yield_mode: YieldMode::SingleBoard,
            yields: [Yield(0, 0), Yield(0, 0), Yield(0, 0)],
            mb_yields: [Yield(0, 0), Yield(0, 0), Yield(0, 0)],
            fl_setting: FlSettings::AfterRetest,
            failures: Vec::new(),
            limitchanges: None,

            mode: AppMode::None,
            hourly_stats: Vec::new(),
            hourly_gs: false,
            hourly_boards: false,

            multiboard_results: Vec::new(),

            selected_test: 0,
            selected_test_buf: String::new(),
            selected_test_index: 0,
            selected_test_results: (TType::Unknown, Vec::new()),

            export_settings: ExportSettings::default(),
            info_vp: LogInfoWindow::default(),
            scan_vp: ScanDirWindow::default(),
            daily_yield_vp: DailyYieldWindow::default(path_list),
        }
    }
}

impl MyApp {
    fn update_stats(&mut self, ctx: &egui::Context) {
        let mut lock = self.log_master.write().unwrap();

        lock.update();
        self.yields = lock.get_yields();
        self.mb_yields = lock.get_mb_yields();
        self.failures = lock.get_failures(self.fl_setting);
        self.hourly_stats = lock.get_hourly_mb_stats();
        self.multiboard_results = lock.get_mb_results();
        self.limitchanges = lock.get_tests_w_limit_changes();

        ctx.request_repaint();
    }

    // Do I even need to clear these?
    fn clear_stats(&mut self) {
        self.hourly_stats.clear();
        self.multiboard_results.clear();
        self.auto_update.clear();
        self.selected_test = 0;
        *self.progress_x.write().unwrap() = 0;
        *self.progress_m.write().unwrap() = 1;
    }

    fn load_logs(&mut self, ctx: &egui::Context, mode: LoadMode) {
        //let input_path = product.path.clone();

        let input_path = match mode {
            LoadMode::Folder(ref x) => x.clone(),
            LoadMode::ProductList(ref x) => PathBuf::from(x),
        };

        let start_dt = TimeZone::from_local_datetime(
            &Local,
            &NaiveDateTime::new(self.date_start, self.time_start),
        )
        .unwrap();

        let end_dt = {
            if self.time_end_use {
                TimeZone::from_local_datetime(
                    &Local,
                    &NaiveDateTime::new(self.date_end, self.time_end),
                )
                .unwrap()
            } else {
                Local::now()
            }
        };

        self.loading = true;
        self.clear_stats();

        if matches!(mode, LoadMode::ProductList(_)) && !self.time_end_use {
            self.auto_update.enabled = true;
            self.auto_update.usable = true;
            self.auto_update.product = Some(self.selected_product);
            self.auto_update.last_scan_time = Some(Local::now());
        }

        let lb_lock = self.log_master.clone();
        let pm_lock = self.progress_m.clone();
        let px_lock = self.progress_x.clone();
        let frame = ctx.clone();

        thread::spawn(move || {
            let logs_result = match mode {
                LoadMode::Folder(_) => get_logs_in_path(&input_path, pm_lock.clone()),
                LoadMode::ProductList(_) => get_logs_in_path_t(&input_path, start_dt, end_dt),
            };

            if let Ok(mut logs) = logs_result {
                *pm_lock.write().unwrap() = logs.len() as u32;
                (*lb_lock.write().unwrap()).clear();
                frame.request_repaint_after(std::time::Duration::from_millis(500));

                println!("Found {} logs to load.", logs.len());
                logs.sort_by_key(|k| k.1);

                for log in logs.iter().rev() {
                    (*lb_lock.write().unwrap()).push_from_file(&log.0);
                    *px_lock.write().unwrap() += 1;
                    frame.request_repaint_after(std::time::Duration::from_millis(500));
                }
            }
        });
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(std::time::Duration::from_secs(5));

        egui::SidePanel::left("Settings_panel").show(ctx, |ui| {
            ui.set_min_width(270.0);

            // "Menu" bar
            ui.horizontal(|ui| {
                ui.set_enabled(!self.loading);

                if ui.button("📁").clicked() && !self.loading {
                    if let Some(input_path) = rfd::FileDialog::new().pick_folder() {
                        self.load_logs(ctx, LoadMode::Folder(input_path));
                    }
                }

                egui::ComboBox::from_label("")
                    .width(200.0)
                    .selected_text(match self.product_list.get(self.selected_product) {
                        Some(sel) => sel.get_name().to_string(),
                        None => "".to_string(),
                    })
                    .show_ui(ui, |ui| {
                        for (i, t) in self.product_list.iter().enumerate() {
                            ui.selectable_value(
                                &mut self.selected_product,
                                i,
                                t.get_name().to_string(),
                            );
                        }
                    });
            });

            ui.separator();

            // Date and time pickers:
            ui.horizontal(|ui| {
                ui.add(
                    egui_extras::DatePickerButton::new(&mut self.date_start)
                        .id_source("Starting time"),
                );

                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.time_start_string).desired_width(70.0),
                );
                if response.lost_focus() {
                    match NaiveTime::parse_from_str(self.time_start_string.as_str(), "%H:%M:%S") {
                        Ok(new_t) => {
                            self.time_start = new_t;
                        }
                        Err(_) => {
                            println!("ERR: Failed to pares time string, reverting!");
                            self.time_start_string = self.time_start.format("%H:%M:%S").to_string();
                        }
                    }
                }

                // Set timeframe to this shift
                if ui.button(MESSAGE[SHIFT][self.lang]).clicked() {
                    self.date_start = Local::now().date_naive();
                    self.date_end = Local::now().date_naive();

                    let time_now = Local::now().naive_local();
                    let hours_now = time_now.hour();
                    if (6..14).contains(&hours_now) {
                        self.time_start = NaiveTime::from_hms_opt(6, 0, 0).unwrap();
                        self.time_end = NaiveTime::from_hms_opt(13, 59, 59).unwrap();
                    } else if (14..22).contains(&hours_now) {
                        self.time_start = NaiveTime::from_hms_opt(14, 0, 0).unwrap();
                        self.time_end = NaiveTime::from_hms_opt(21, 59, 59).unwrap();
                    } else {
                        if hours_now < 6 {
                            self.date_start = self.date_start.pred_opt().unwrap();
                        } else {
                            self.date_end = self.date_end.succ_opt().unwrap();
                        }
                        self.time_start = NaiveTime::from_hms_opt(22, 0, 0).unwrap();
                        self.time_end = NaiveTime::from_hms_opt(5, 59, 59).unwrap();
                    }

                    self.time_start_string = self.time_start.format("%H:%M:%S").to_string();
                    self.time_end_string = self.time_end.format("%H:%M:%S").to_string();
                }

                // Set timeframe to the last 24h
                if ui.button(MESSAGE[A_DAY][self.lang]).clicked() {
                    self.date_start = Local::now().date_naive().pred_opt().unwrap();
                    self.time_start = Local::now().time();
                    self.date_end = Local::now().date_naive();
                    self.time_end = Local::now().time();

                    self.time_start_string = self.time_start.format("%H:%M:%S").to_string();
                    self.time_end_string = self.time_end.format("%H:%M:%S").to_string();
                }
            });

            ui.horizontal(|ui| {
                ui.horizontal(|ui| {
                    ui.set_enabled(self.time_end_use);

                    ui.add(
                        egui_extras::DatePickerButton::new(&mut self.date_end)
                            .id_source("Ending time"),
                    );

                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.time_end_string).desired_width(70.0),
                    );
                    if response.lost_focus() {
                        match NaiveTime::parse_from_str(self.time_end_string.as_str(), "%H:%M:%S") {
                            Ok(new_t) => {
                                self.time_end = new_t;
                            }
                            Err(_) => {
                                println!("ERR: Failed to parse time string, reverting!");
                                self.time_end_string = self.time_end.format("%H:%M:%S").to_string();
                            }
                        }
                    }
                });

                ui.add(egui::Checkbox::without_text(&mut self.time_end_use));

                if ui.button(MESSAGE[LOAD][self.lang]).clicked() && !self.loading {
                    if let Some(product) = self.product_list.get(self.selected_product) {
                        self.load_logs(ctx, LoadMode::ProductList(product.get_log_dir().clone()));
                    }
                }
            });

            // Auto-update checkbox
            ui.horizontal(|ui| {
                ui.set_enabled(self.auto_update.usable);

                ui.monospace(MESSAGE[AUTO_UPDATE][self.lang]);
                ui.add(egui::Checkbox::without_text(&mut self.auto_update.enabled));

                if self.auto_update.state() != AUState::Standby {
                    ui.add(egui::Spinner::new());
                }
            });

            // Loading Bar
            if self.loading {
                ui.separator();

                let mut xx: u32 = 0;
                let mut mm: u32 = 1;

                if let Ok(m) = self.progress_m.try_read() {
                    mm = *m;
                }
                if let Ok(x) = self.progress_x.try_read() {
                    xx = *x;
                }

                ui.add(
                    ProgressBar::new(xx as f32 / mm as f32)
                        .text(RichText::new(format!("{} / {}", xx, mm)))
                        .animate(true),
                );

                self.status =
                    format!("{}: {} / {}", MESSAGE[LOADING_MESSAGE][self.lang], xx, mm).to_owned();

                if xx == mm {
                    self.loading = false;
                    self.update_stats(ctx);
                }
            } else if self.auto_update.enabled {
                match self.auto_update.state() {
                    AUState::Standby => {
                        if self.auto_update.its_time() {
                            self.auto_update.gather_logs(&self.product_list);
                        }
                    }
                    AUState::Loaded => {
                        let (duration, number) =
                            self.auto_update.push_logs(self.log_master.clone());

                        self.status = format!(
                            "{}{}{}{}",
                            MESSAGE[AU_DONE_1][self.lang],
                            duration.num_milliseconds(),
                            MESSAGE[AU_DONE_2][self.lang],
                            number
                        );

                        if number != 0 {
                            self.update_stats(ctx);
                        }
                    }
                    AUState::Loading => (),
                }
            }

            // Statistics:
            ui.separator();

            ui.horizontal(|ui| {
                ui.monospace(MESSAGE[YIELD][self.lang]);

                // Localiazation?
                ui.selectable_value(&mut self.yield_mode, YieldMode::SingleBoard, "Single");
                ui.selectable_value(&mut self.yield_mode, YieldMode::MultiBoard, "Multiboard");
            });

            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.monospace("");
                    ui.monospace(MESSAGE[FIRST_T][self.lang]);
                    ui.monospace(MESSAGE[TOTAL][self.lang]);
                    ui.monospace(MESSAGE[AFTER_RT][self.lang]);
                });

                ui.add(egui::Separator::default().vertical());

                let x = match self.yield_mode {
                    YieldMode::SingleBoard => &self.yields,
                    YieldMode::MultiBoard => &self.mb_yields,
                };

                ui.vertical(|ui| {
                    ui.monospace("OK");
                    ui.monospace(format!("{}", x[0].0));
                    ui.monospace(format!("{}", x[2].0));
                    ui.monospace(format!("{}", x[1].0));
                });

                ui.add(egui::Separator::default().vertical());

                ui.vertical(|ui| {
                    ui.monospace("NOK");
                    ui.monospace(format!("{}", x[0].1));
                    ui.monospace(format!("{}", x[2].1));
                    ui.monospace(format!("{}", x[1].1));
                });

                ui.add(egui::Separator::default().vertical());

                ui.vertical(|ui| {
                    ui.monospace("%");
                    ui.monospace(format!("{0:.2}", x[0].precentage()));
                    ui.monospace(format!("{0:.2}", x[2].precentage()));
                    ui.monospace(format!("{0:.2}", x[1].precentage()));
                });
            });

            // Failure list:

            ui.vertical(|ui| {
                ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
                ui.separator();

                let mut fl_change = false;
                egui::ComboBox::from_id_source("Fails")
                    .selected_text(match self.fl_setting {
                        FlSettings::FirstPass => MESSAGE[FIRST_T][self.lang],
                        FlSettings::All => MESSAGE[TOTAL][self.lang],
                        FlSettings::AfterRetest => MESSAGE[AFTER_RT][self.lang],
                    })
                    .show_ui(ui, |ui| {
                        fl_change = ui
                            .selectable_value(
                                &mut self.fl_setting,
                                FlSettings::FirstPass,
                                MESSAGE[FIRST_T][self.lang],
                            )
                            .changed()
                            || ui
                                .selectable_value(
                                    &mut self.fl_setting,
                                    FlSettings::All,
                                    MESSAGE[TOTAL][self.lang],
                                )
                                .changed()
                            || ui
                                .selectable_value(
                                    &mut self.fl_setting,
                                    FlSettings::AfterRetest,
                                    MESSAGE[AFTER_RT][self.lang],
                                )
                                .changed();
                    });
                if fl_change {
                    println!("reloading tests with mode {:?}", self.fl_setting);
                    self.failures = self
                        .log_master
                        .read()
                        .unwrap()
                        .get_failures(self.fl_setting);
                }

                if !self.failures.is_empty() {
                    TableBuilder::new(ui)
                        .striped(true)
                        .column(Column::initial(220.0).resizable(true))
                        .column(Column::remainder())
                        .body(|mut body| {
                            for fail in &self.failures {
                                body.row(16.0, |mut row| {
                                    row.col(|ui| {
                                        if ui
                                            .add(
                                                egui::Label::new(fail.name.to_owned())
                                                    .truncate(true)
                                                    .sense(Sense::click()),
                                            )
                                            .clicked()
                                        {
                                            self.selected_test_buf = fail.name.clone();
                                            self.mode = AppMode::Plot;
                                        }
                                    });
                                    row.col(|ui| {
                                        ui.label(format!("{}", fail.total));
                                    });
                                });
                            }
                        });
                }
            });
        });

        // Status panel + language change
        egui::TopBottomPanel::bottom("Status_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add(ImageButton::new(egui::include_image!("../res/HU.png")))
                    .clicked()
                {
                    self.lang = LANG_HU;
                    self.status = MESSAGE[LANG_CHANGE][self.lang].to_owned();
                }

                if ui
                    .add(ImageButton::new(egui::include_image!("../res/UK.png")))
                    .clicked()
                {
                    self.lang = LANG_EN;
                    self.status = MESSAGE[LANG_CHANGE][self.lang].to_owned();
                }

                ui.monospace(self.status.to_string());
            });
        });

        // Failed DMC list for Plot view - needs its own panel!
        if self.mode == AppMode::Plot && !self.failures.is_empty() {
            if let Some(x) = self
                .failures
                .iter()
                .find(|k| k.test_id == self.selected_test)
            {
                egui::TopBottomPanel::bottom("failed panels")
                    .resizable(true)
                    .show(ctx, |ui| {
                        ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();

                        ui.with_layout(Layout::left_to_right(egui::Align::Center), |ui| {
                            TableBuilder::new(ui)
                                .striped(true)
                                .column(Column::auto())
                                .column(Column::auto())
                                .body(|mut body| {
                                    for fail in &x.failed {
                                        body.row(20.0, |mut row| {
                                            row.col(|ui| {
                                                if ui
                                                    .add(
                                                        egui::Label::new(fail.0.to_string())
                                                            .sense(Sense::click()),
                                                    )
                                                    .clicked()
                                                {
                                                    self.info_vp.open(
                                                        fail.0.to_string(),
                                                        self.log_master.clone(),
                                                    );
                                                }
                                            });
                                            row.col(|ui| {
                                                ui.label(u64_to_string(fail.1));
                                            });
                                        });
                                    }
                                });

                            if x.by_index.len() > 1 {
                                let mut bars: Vec<Bar> = Vec::new();
                                for bar in x.by_index.iter().enumerate() {
                                    bars.push(Bar {
                                        name: format!("{}.", bar.0 as u64 + 1),
                                        orientation: egui_plot::Orientation::Vertical,
                                        argument: bar.0 as f64 + 1.0,
                                        value: *bar.1 as f64,
                                        base_offset: None,
                                        bar_width: 0.5,
                                        stroke: Stroke {
                                            width: 1.0,
                                            color: Color32::GRAY,
                                        },
                                        fill: Color32::RED,
                                    });
                                }
                                let chart = BarChart::new(bars);

                                Plot::new("failure by index")
                                    .show_x(false)
                                    .show_y(false)
                                    .allow_scroll(false)
                                    .allow_drag(false)
                                    .allow_boxed_zoom(false)
                                    .clamp_grid(true)
                                    .set_margin_fraction(Vec2 { x: 0.05, y: 0.1 })
                                    .width(std::cmp::max(8, x.by_index.len()) as f32 * 30.0)
                                    .show(ui, |ui| {
                                        ui.bar_chart(chart);
                                    });
                            }
                        });
                    });
            }
        }

        // Central panel
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
            ui.set_enabled(!self.loading);

            // Top "menu bar"
            ui.horizontal(|ui| {
                if ui.button("🔎").clicked() {
                    self.info_vp.enable();
                }

                if ui.button(MESSAGE_E[EXPORT_LABEL][self.lang]).clicked() {
                    self.mode = AppMode::Export;
                }

                if ui.button(MESSAGE_H[HOURLY_LABEL][self.lang]).clicked() {
                    self.mode = AppMode::Hourly;
                }

                if ui.button(MESSAGE_H[MULTI_LABEL][self.lang]).clicked() {
                    self.mode = AppMode::Multiboards;
                }

                if ui.button(MESSAGE_P[PLOT_LABEL][self.lang]).clicked() {
                    self.mode = AppMode::Plot;
                }

                // Right side first:
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Scan").clicked() {
                        self.scan_vp.enable();
                    }

                    if ui.button("Daily Yield").clicked() {
                        self.daily_yield_vp.enable();
                    }
                });
            });

            ui.separator();

            // Plot mode
            if self.mode == AppMode::Plot && !self.loading {
                let lfh = self.log_master.read().unwrap();
                let testlist = lfh.get_testlist();
                let mut reset_plot = false;
                if !testlist.is_empty() {
                    // I will need to replace this latter with something edittable
                    ui.horizontal(|ui| {
                        ui.add(DropDownBox::from_iter(
                            testlist.iter().map(|f| &f.0),
                            "test_dropbox",
                            &mut self.selected_test_buf,
                            |ui, text| ui.selectable_label(false, text),
                        ));

                        if ui.button("Reload").clicked() {
                            self.selected_test_results.1.clear();
                        }

                        ui.label("Index:");
                        ui.add(
                            egui::DragValue::new(&mut self.selected_test_index)
                                .speed(1.0)
                                .clamp_range(0..=20),
                        );
                    });

                    ui.separator();

                    if let Some(x) = testlist.iter().position(|p| p.0 == self.selected_test_buf) {
                        if x != self.selected_test || self.selected_test_results.1.is_empty() {
                            self.selected_test = x;
                            println!("INFO: Loading results for test nbr {}!", self.selected_test);
                            self.selected_test_results = lfh.get_stats_for_test(self.selected_test);
                            reset_plot = true;
                            if self.selected_test_results.1.is_empty() {
                                println!("\tERR: Loading failed!");
                            } else {
                                println!("\tINFO: Loading succefull!");
                            }
                        }
                    }

                    // Insert plot here

                    let ppoints: PlotPoints = self
                        .selected_test_results
                        .1
                        .iter()
                        .filter_map(|r| {
                            if self.selected_test_index != 0 && self.selected_test_index != r.1 {
                                return None;
                            }

                            if r.2 .0 == BResult::Unknown {
                                return None;
                            }
                            Some([r.0 as f64, r.2 .1 as f64])
                        })
                        .collect();

                    //Lim2 (f32,f32),     // UL - LL
                    //Lim3 (f32,f32,f32)  // Nom - UL - LL
                    let upper_limit_p: PlotPoints = self
                        .selected_test_results
                        .1
                        .iter()
                        .filter_map(|r| {
                            if self.selected_test_index != 0 && self.selected_test_index != r.1 {
                                return None;
                            }

                            if let TLimit::Lim3(_, x, _) = r.3 {
                                Some([r.0 as f64, x as f64])
                            } else if let TLimit::Lim2(x, _) = r.3 {
                                Some([r.0 as f64, x as f64])
                            } else {
                                None
                            }
                        })
                        .collect();

                    let nominal_p: PlotPoints = self
                        .selected_test_results
                        .1
                        .iter()
                        .filter_map(|r| {
                            if self.selected_test_index != 0 && self.selected_test_index != r.1 {
                                return None;
                            }

                            if let TLimit::Lim3(x, _, _) = r.3 {
                                Some([r.0 as f64, x as f64])
                            } else {
                                None
                            }
                        })
                        .collect();

                    let lower_limit_p: PlotPoints = self
                        .selected_test_results
                        .1
                        .iter()
                        .filter_map(|r| {
                            if self.selected_test_index != 0 && self.selected_test_index != r.1 {
                                return None;
                            }

                            if let TLimit::Lim3(_, _, x) = r.3 {
                                Some([r.0 as f64, x as f64])
                            } else if let TLimit::Lim2(_, x) = r.3 {
                                Some([r.0 as f64, x as f64])
                            } else {
                                None
                            }
                        })
                        .collect();

                    let points = egui_plot::Points::new(ppoints)
                        .highlight(true)
                        .color(Color32::BLUE)
                        .name(testlist[self.selected_test].0.to_owned());

                    let upper_limit = Line::new(upper_limit_p).color(Color32::RED).name("MAX");

                    let nominal = Line::new(nominal_p).color(Color32::GREEN).name("Nom");

                    let lower_limit = Line::new(lower_limit_p).color(Color32::RED).name("MIN");

                    let mut plot = Plot::new("Test results")
                        .custom_x_axes(vec![egui_plot::AxisHints::new_x().formatter(x_formatter)])
                        .custom_y_axes(vec![egui_plot::AxisHints::new_y()
                            .formatter(y_formatter)
                            .label(self.selected_test_results.0.unit())])
                        .coordinates_formatter(
                            egui_plot::Corner::RightTop,
                            egui_plot::CoordinatesFormatter::new(c_formater),
                        )
                        .label_formatter(|name, value| {
                            if !name.is_empty() {
                                format!("{}: {:+1.4E}", name, value.y)
                            } else {
                                "".to_owned()
                            }
                        })
                        .height(ui.available_height() - 20.0);

                    if reset_plot {
                        plot = plot.reset();
                    }

                    plot.show(ui, |plot_ui| {
                        plot_ui.points(points);
                        plot_ui.line(upper_limit);
                        plot_ui.line(nominal);
                        plot_ui.line(lower_limit);
                    });
                }
            }

            // Hourly mode
            if self.mode == AppMode::Hourly && !self.hourly_stats.is_empty() {
                let width_for_last_col = ui.available_width() - 250.0;

                ui.push_id("hourly", |ui| {
                    TableBuilder::new(ui)
                        .striped(true)
                        .column(Column::initial(150.0))
                        .column(Column::initial(50.0))
                        .column(Column::initial(50.0))
                        .column(Column::remainder())
                        .header(20.0, |mut header| {
                            header.col(|ui| {
                                ui.heading(MESSAGE_H[TIME][self.lang]);
                            });
                            header.col(|ui| {
                                ui.heading("OK");
                            });
                            header.col(|ui| {
                                ui.heading("NOK");
                            });
                            header.col(|ui| {
                                ui.horizontal(|ui| {
                                    ui.heading(MESSAGE_H[RESULTS][self.lang]);
                                    ui.checkbox(&mut self.hourly_gs, "GS");
                                    ui.checkbox(&mut self.hourly_boards, "Boards")
                                });
                            });
                        })
                        .body(|mut body| {
                            for hour in &self.hourly_stats {
                                let results_per_row =
                                    20.0_f32.max((width_for_last_col / 14.0).floor());
                                let needed_rows = (hour.2.len() as f32 / results_per_row).ceil();

                                let used_yield = 
                                if self.hourly_gs {
                                    if self.hourly_boards {
                                        &hour.1.boards_with_gs
                                    } else {
                                        &hour.1.panels_with_gs
                                    }
                                } else if self.hourly_boards {
                                    &hour.1.boards
                                } else {
                                    &hour.1.panels
                                };

                                body.row(14.0 * needed_rows, |mut row| {
                                    row.col(|ui| {
                                        ui.label(u64_to_timeframe(hour.0));
                                    });
                                    row.col(|ui| {
                                        ui.label(format!("{}", used_yield.0));
                                    });
                                    row.col(|ui| {
                                        ui.label(format!("{}", used_yield.1));
                                    });
                                    row.col(|ui| {
                                        ui.spacing_mut().interact_size = Vec2::new(0.0, 0.0);
                                        ui.spacing_mut().item_spacing = Vec2::new(3.0, 3.0);

                                        let chunks = hour.2.chunks(results_per_row as usize);
                                        for chunk in chunks {
                                            ui.horizontal(|ui| {
                                                for (r, _, DMC, gs) in chunk {
                                                    if draw_result_box(ui, r, *gs).clicked() {
                                                        self.info_vp.open_first_NOK(
                                                            DMC.clone(),
                                                            self.log_master.clone(),
                                                        )
                                                    }
                                                }
                                            });
                                        }
                                    });
                                });
                            }
                        });
                });
            }

            // Multiboards mode
            if self.mode == AppMode::Multiboards && !self.multiboard_results.is_empty() {
                ui.push_id("multib", |ui| {
                    TableBuilder::new(ui)
                        .striped(true)
                        .column(Column::initial(40.0).resizable(true))
                        .column(Column::initial(200.0).resizable(true))
                        .column(Column::initial(130.0).resizable(true))
                        .column(Column::remainder())
                        .body(|mut body| {
                            for (i, mb) in self.multiboard_results.iter().enumerate() {
                                let gs = mb.2;
                                let color_mb = if gs {
                                    DARK_GOLD
                                } else {
                                    mb.1.last().unwrap().result.into_dark_color()
                                };

                                for (i2, sb) in mb.1.iter().enumerate() {
                                    let color_sb = if gs {
                                        DARK_GOLD
                                    } else {
                                        sb.result.into_dark_color()
                                    };

                                    body.row(15.0, |mut row| {
                                        row.col(|ui| {
                                            if i2 == 0 {
                                                //ui.label(format!("{}.", i+1));
                                                ui.label(
                                                    egui::RichText::new(format!("{}.", i + 1))
                                                        .color(color_mb),
                                                );
                                            }
                                        });
                                        row.col(|ui| {
                                            if i2 == 0 {
                                                //ui.label(mb.0.clone());
                                                if ui
                                                    .add(
                                                        egui::Label::new(
                                                            egui::RichText::new(mb.0.clone())
                                                                .color(color_mb),
                                                        )
                                                        .sense(Sense::click()),
                                                    )
                                                    .clicked()
                                                {
                                                    self.info_vp.open_first_NOK(
                                                        mb.0.clone(),
                                                        self.log_master.clone(),
                                                    );
                                                }
                                            }
                                        });
                                        row.col(|ui| {
                                            //ui.label(u64_to_string( sb.0));
                                            ui.label(
                                                egui::RichText::new(u64_to_string(sb.start))
                                                    .color(color_sb),
                                            );
                                        });
                                        row.col(|ui| {
                                            ui.spacing_mut().item_spacing = Vec2::new(3.0, 0.0);
                                            ui.horizontal(|ui| {
                                                for (sb_index, r) in sb.panels.iter().enumerate() {
                                                    if draw_result_box(ui, r, gs).clicked() {
                                                        self.info_vp.open_w_index(
                                                            mb.0.clone(),
                                                            sb_index,
                                                            self.log_master.clone(),
                                                        );
                                                    }
                                                }
                                            });
                                        });
                                    });
                                }
                            }
                        });
                });
            }

            // Export mode
            if self.mode == AppMode::Export {
                ui.heading(MESSAGE_E[SETTINGS][self.lang]);
                ui.checkbox(
                    &mut self.export_settings.vertical,
                    MESSAGE_E[VERTICAL_O][self.lang],
                );
                ui.checkbox(
                    &mut self.export_settings.only_failed_panels,
                    MESSAGE_E[EXPORT_NOK_ONLY][self.lang],
                );
                ui.checkbox(
                    &mut self.export_settings.only_final_logs,
                    MESSAGE_E[EXPORT_FINAL_ONLY][self.lang],
                );
                ui.horizontal(|ui| {
                    ui.monospace(MESSAGE_E[EXPORT_MODE][self.lang]);
                    ui.selectable_value(
                        &mut self.export_settings.mode,
                        ExportMode::All,
                        MESSAGE_E[EXPORT_MODE_ALL][self.lang],
                    );
                    ui.selectable_value(
                        &mut self.export_settings.mode,
                        ExportMode::FailuresOnly,
                        MESSAGE_E[EXPORT_MODE_FTO][self.lang],
                    );
                    ui.selectable_value(
                        &mut self.export_settings.mode,
                        ExportMode::Manual,
                        MESSAGE_E[EXPORT_MODE_MANUAL][self.lang],
                    );
                });

                if self.export_settings.mode == ExportMode::Manual {
                    ui.monospace(MESSAGE_E[EXPORT_MANUAL][self.lang]);
                    ui.text_edit_singleline(&mut self.export_settings.list);
                    ui.monospace(MESSAGE_E[EXPORT_MANUAL_EX][self.lang]);
                }

                ui.separator();

                if ui.button(MESSAGE_E[SAVE][self.lang]).clicked() && !self.loading {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("XLSX", &["xlsx"])
                        .set_file_name("out.xlsx")
                        .save_file()
                    {
                        self.log_master
                            .read()
                            .unwrap()
                            .export(path, &self.export_settings);
                    }
                }

                // If there are tests with limit changes, then notify the user
                if let Some(changed_tests) = &self.limitchanges {
                    ui.add_space(10.0);
                    for (_, name) in changed_tests {
                        if ui
                            .add(
                                egui::Label::new(
                                    egui::RichText::new(format!(
                                        "{} {} {}",
                                        MESSAGE_E[LIMIT_W][self.lang],
                                        name,
                                        MESSAGE_E[LIMIT_W2][self.lang]
                                    ))
                                    .color(Color32::RED)
                                    .size(14.0),
                                )
                                .sense(Sense::click()),
                            )
                            .clicked()
                        {
                            self.selected_test_buf = name.clone();
                            self.mode = AppMode::Plot;
                        }
                    }
                }
            }
        });

        if self.info_vp.enabled() {
            self.info_vp.update(ctx, self.log_master.clone());
        }

        if self.scan_vp.enabled() {
            self.scan_vp.update(ctx);
        }

        if self.daily_yield_vp.enabled() {
            self.daily_yield_vp.update(ctx);
        }
    }
}

// Formaters for the plot

fn y_formatter(
    tick: egui_plot::GridMark,
    _max_digits: usize,
    _range: &RangeInclusive<f64>,
) -> String {
    format!("{:+1.1E}", tick.value)
}

fn x_formatter(
    tick: egui_plot::GridMark,
    _max_digits: usize,
    _range: &RangeInclusive<f64>,
) -> String {
    let t: DateTime<Utc> = DateTime::from_timestamp(tick.value as i64, 0).unwrap();

    format!("{}\n{}", t.format("%m-%d"), t.format("%R"))
}

fn c_formater(point: &egui_plot::PlotPoint, _: &egui_plot::PlotBounds) -> String {
    let t: DateTime<Utc> = DateTime::from_timestamp(point.x as i64, 0).unwrap();

    format!("x: {:+1.4E}\t t: {}", point.y, t.format("%F %R"))
}

fn draw_result_box(ui: &mut egui::Ui, result: &BResult, gs: bool) -> egui::Response {
    let desired_size = egui::vec2(10.0, 10.0);
    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());

    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);

        let rect = rect.expand(visuals.expansion);

        ui.painter()
            .rect_filled(rect, 2.0, result.into_dark_color());
        if gs {
            ui.painter()
                .rect_stroke(rect.shrink(1.0), 1.0, Stroke::new(2.0, DARK_GOLD));
        }
    }

    response
}
