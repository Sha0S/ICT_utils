use chrono::{DateTime, Duration, Local};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, RwLock},
    thread,
};

const ROOT_DIRS: [&str; 2] = ["C:\\Agilent_ICT\\boards\\", "C:\\Keysight_ICT\\boards\\"];
fn get_board_directories() -> Result<Vec<PathBuf>, std::io::Error> {
    let mut ret: Vec<PathBuf> = Vec::new();

    for root in ROOT_DIRS {
        if let Ok(dirs) = fs::read_dir(root) {
            for dir in dirs {
                let dir = dir?;
                let path = dir.path();
                if path.is_dir() && path.join("testplan").exists() {
                    ret.push(path);
                }
            }
        }
    }

    Ok(ret)
}

type FileStats = (PathBuf, DateTime<Local>, DateTime<Local>);

fn get_changed_files(
    root: &PathBuf,
    time_limit: Duration,
) -> Result<Vec<FileStats>, std::io::Error> {
    let mut ret: Vec<FileStats> = Vec::new();

    for dir in fs::read_dir(root)? {
        let dir = dir?;
        let path = dir.path();
        if path.is_dir() {
            ret.append(&mut get_changed_files(&path, time_limit)?);
        } else if let Ok(x) = path.metadata() {
            let modified: DateTime<Local> = x.modified().unwrap().into();
            let created: DateTime<Local> = x.created().unwrap().into();
            if Local::now() - modified < time_limit || Local::now() - created < time_limit {
                ret.push((path, created, modified));
            }
        }
    }

    Ok(ret)
}

struct ScannedDir {
    dir: PathBuf,
    changed_files: Vec<FileStats>,
}
pub struct ScanDirWindow {
    enabled: bool,
    time_limit: i64,
    scanning: Arc<RwLock<bool>>,
    scanned_dirs: Arc<RwLock<Vec<ScannedDir>>>,
}

impl ScanDirWindow {
    pub fn default() -> Self {
        ScanDirWindow {
            enabled: false,
            time_limit: 7,

            scanning: Arc::new(RwLock::new(false)),
            scanned_dirs: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn enable(&mut self) {
        self.enabled = true;
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn update(&mut self, ctx: &egui::Context) {
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("SDirWindow"),
            egui::ViewportBuilder::default()
                .with_title("ScanDir")
                .with_inner_size([500.0, 300.0]),
            |ctx, class| {
                assert!(
                    class == egui::ViewportClass::Immediate,
                    "This egui backend doesn't support multiple viewports"
                );

                egui::TopBottomPanel::top("Top").show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Scan!").clicked() && !*self.scanning.read().unwrap() {
                            self.scanned_dirs.write().unwrap().clear();
                            *self.scanning.write().unwrap() = true;

                            let sd_lock = self.scanned_dirs.clone();
                            let scan_lock = self.scanning.clone();
                            let timelimit = Duration::try_days(self.time_limit).unwrap();

                            thread::spawn(move || {
                                if let Ok(directories) = get_board_directories() {
                                    for dir in &directories {
                                        match get_changed_files(dir, timelimit) {
                                            Ok(files) => {
                                                sd_lock.write().unwrap().push(ScannedDir {
                                                    dir: dir.clone(),
                                                    changed_files: files,
                                                });
                                            }

                                            Err(err) => {
                                                println!(
                                                    "Err: could not scan directories! {err:?}"
                                                );
                                            }
                                        }
                                    }
                                }

                                *scan_lock.write().unwrap() = false;
                            });
                        }

                        if *self.scanning.read().unwrap() {
                            ui.spinner();
                        }

                        ui.label("Days:");
                        ui.add(egui::DragValue::new(&mut self.time_limit).speed(1.0));
                    });
                });

                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
                    egui::ScrollArea::vertical()
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            egui::Grid::new("table").show(ui, |ui| {
                                for dir in self.scanned_dirs.read().unwrap().iter() {
                                    ui.label(format!("{}", dir.dir.display()));
                                    ui.end_row();

                                    for file in &dir.changed_files {
                                        ui.add_space(50.0);
                                        ui.label(format!("{}", file.0.display()));
                                        ui.label(format!("{}", file.1.format("%F %R")));
                                        ui.label(format!("{}", file.2.format("%F %R")));
                                        ui.end_row();
                                    }
                                }
                            });
                        });
                });

                if ctx.input(|i| i.viewport().close_requested()) {
                    self.enabled = false;
                }
            },
        );
    }
}
