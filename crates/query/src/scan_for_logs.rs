use std::{fs, path::{Path, PathBuf}};


pub fn search_for_log(src: &str) -> Option<PathBuf> {
    println!("Searching for {src}");
    let path = PathBuf::from(src);

    if path.exists() {
        return Some(path);
    } else {
        // try log_dir\\date_of_log\\log_filename
        let dir = path.parent().unwrap();
        let file = path.file_name().unwrap();
        let (_, date_str) = file.to_str().unwrap().split_once('-').unwrap();
        let sub_dir = format!(
            "20{}_{}_{}",
            &date_str[0..2],
            &date_str[2..4],
            &date_str[4..6]
        );
        let mut final_path = dir.join(sub_dir);
        final_path.push(file);

        println!("Final path: {:?}", final_path);
        if final_path.exists() {
            return Some(final_path);
        }
    }

    println!("Path not found!");
    None
}

struct SLog {
    path_found: Option<PathBuf>
}

impl SLog {
    fn new(src: &str) -> Self {
        SLog { path_found: search_for_log(src)}
    }

    fn found(&self) -> bool {
        self.path_found.is_some()
    }

    fn save(&self, output_dir: &Path) {
        if let Some(path) = &self.path_found {
            println!("Saving: {:?}", path);
            let filename = path.file_name().unwrap();

            let mut out_path = PathBuf::from(output_dir);
            out_path.push(filename);
            
            println!("Out path: {:?}", out_path);
            if fs::copy(path, out_path).is_ok() {
                println!("\tSuccess!")
            } else {
                println!("\tFailed!")
            }
        }
    }
} 

struct SPanel {
    logs: Vec<SLog>
}

impl SPanel {
    fn new(src: Vec<&str>) -> Self {
        let mut logs = Vec::new();
        for log in src {
            logs.push(SLog::new(log));
        }

        SPanel { logs }
    }
}

#[derive(Default)]
pub struct ScanForLogs {
    logs: Vec<SPanel>,

    total: u8,
    found: u8,

    selected: u8,
    only_save_selected: bool,

    enabled: bool
}

impl ScanForLogs {
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn clear(&mut self) {
        self.logs.clear();
        self.total = 0;
        self.found = 0;
    }

    pub fn set_selected(&mut self, selected: u8) {
        self.selected = selected
    }

    pub fn push(&mut self, logs: Vec<&str>) {
        self.logs.push(SPanel::new(logs));

        self.total = 0;
        self.found = 0;
        for log in &self.logs {
            for lo in &log.logs {
                self.total += 1;
                if lo.found() {
                    self.found += 1;
                }
            }
        }
    }

    fn save_logs(&self) {
        if let Some(output_dir) = rfd::FileDialog::new().pick_folder() {
            for log in &self.logs {
                for (i, lo) in log.logs.iter().enumerate() {
                    if !self.only_save_selected || i == self.selected as usize {
                        lo.save(&output_dir);
                    }
                }
            }
        }
    }

    pub fn update(&mut self, ctx: &egui::Context) {
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("SFLogs"),
            egui::ViewportBuilder::default()
                .with_title("Logok keresése")
                .with_inner_size([300.0, 300.0]),
            |ctx, class| {
                assert!(
                    class == egui::ViewportClass::Immediate,
                    "This egui backend doesn't support multiple viewports"
                );

                egui::TopBottomPanel::top("DatePicker").show(ctx, |ui| {
                    ui.monospace(format!("Megtalált logok: {}/{}.", self.found, self.total));
                    ui.checkbox(&mut self.only_save_selected, "Csak a kijelölt board logjait mentse.");
                    
                    if ui.button("Mentés").clicked() {
                        self.save_logs();
                    }
                });

                egui::CentralPanel::default().show(ctx, |ui| {
                    for log in &self.logs {
                        ui.horizontal(|ui| {
                            for (i, lo) in log.logs.iter().enumerate() {
                                draw_result_box(ui, lo.found(), self.selected == i as u8);
                            }
                        });
                    } 
                });

                if ctx.input(|i| i.viewport().close_requested()) {
                    self.enabled = false;
                }
            },
        );
    }
}

fn draw_result_box(ui: &mut egui::Ui, result: bool, highlight: bool) -> egui::Response {
    let desired_size = egui::vec2(10.0, 10.0);

    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());

    let rect = if highlight { rect.expand(2.0) } else { rect };

    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        let rect = rect.expand(visuals.expansion);
        ui.painter().rect_filled(rect, 2.0, if result {egui::Color32::GREEN} else {egui::Color32::RED});
    }

    response
}