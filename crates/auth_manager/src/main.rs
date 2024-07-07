#![allow(non_snake_case)]

use eframe::egui;
use egui::Vec2;
use egui_extras::{Column, TableBuilder};
use ICT_auth::*;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> anyhow::Result<()> {
    env_logger::init(); // Log to stderr (if you run with `RUST_LOG=debug`).

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([340.0, 300.0]),
        ..Default::default()
    };

    _ = eframe::run_native(
        format!("ICT User Manager (v{VERSION})").as_str(),
        options,
        Box::new(|_| Box::<MyApp>::default()),
    );

    Ok(())
}

#[derive(Debug, Clone)]
struct NewUser {
    name: String,
    pass: String,
    pass2: String,
    level: UserLevel,
}

impl From<&NewUser> for User {
    fn from(val: &NewUser) -> Self {
        let mut ret = User::new(val.name.clone(), val.level);

        ret.create_hash(&val.pass);

        ret
    }
}

impl NewUser {
    fn all_ok(&self) -> bool {
        self.pass.len() > 4 && self.name.len() > 4 && self.pass_match()
    }

    fn pass_match(&self) -> bool {
        self.pass == self.pass2
    }

    fn clear(&mut self) {
        self.name.clear();
        self.pass.clear();
        self.pass2.clear();
        self.level = UserLevel::Technician;
    }
}

struct MyApp {
    users: Vec<User>,
    current_user: Option<User>,

    login_name: String,
    login_pass: String,

    new_user: NewUser,
}

impl Default for MyApp {
    fn default() -> Self {
        Self {
            users: load_user_list(),
            current_user: None,

            login_name: String::new(),
            login_pass: String::new(),

            new_user: NewUser {
                name: String::new(),
                pass: String::new(),
                pass2: String::new(),
                level: UserLevel::Technician,
            },
        }
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("Login").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.login_name).desired_width(200.0));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.login_pass)
                            .desired_width(200.0)
                            .password(true),
                    );
                });

                ui.vertical(|ui| {
                    let resp =
                        ui.add(egui::Button::new("Login").min_size(Vec2 { x: 50.0, y: 15.0 }));

                    if resp.clicked() {
                        for user in self.users.iter() {
                            if user.name == self.login_name && user.check_pw(&self.login_pass) {
                                println!("Login as: {}", user.name);
                                self.current_user = Some(user.clone());
                                self.login_name.clear();
                                self.login_pass.clear();
                            }
                        }
                    }

                    let resp2 =
                        ui.add(egui::Button::new("Logout").min_size(Vec2 { x: 50.0, y: 15.0 }));
                    if resp2.clicked() {
                        println!("Logout");
                        self.current_user = None;
                    }
                });

                ui.vertical(|ui| {
                    let resp =
                        ui.add(egui::Button::new("Save").min_size(Vec2 { x: 50.0, y: 15.0 }));
                    if resp.clicked() {
                        println!("Save");
                        save_user_list(&self.users);
                    }

                    let resp =
                        ui.add(egui::Button::new("Cancel").min_size(Vec2 { x: 50.0, y: 15.0 }));
                    if resp.clicked() {
                        println!("Cancel");
                        self.users = load_user_list();
                    }
                });
            });
        });

        if let Some(user) = &self.current_user {
            if user.level > UserLevel::Technician {
                egui::TopBottomPanel::bottom("Add_user").show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.new_user.name)
                                    .desired_width(200.0),
                            );
                            ui.add(
                                egui::TextEdit::singleline(&mut self.new_user.pass)
                                    .desired_width(200.0)
                                    .password(true),
                            );
                            ui.add(
                                egui::TextEdit::singleline(&mut self.new_user.pass2)
                                    .desired_width(200.0)
                                    .password(true),
                            );

                            if !self.new_user.pass_match() {
                                ui.label("W: Passwords don't match!");
                            }
                        });

                        ui.vertical(|ui| {
                            egui::ComboBox::from_id_source("Level")
                                .selected_text(format!("{:?}", self.new_user.level))
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut self.new_user.level,
                                        UserLevel::Admin,
                                        "Admin",
                                    );
                                    ui.selectable_value(
                                        &mut self.new_user.level,
                                        UserLevel::Engineer,
                                        "Engineer",
                                    );
                                    ui.selectable_value(
                                        &mut self.new_user.level,
                                        UserLevel::Technician,
                                        "Technician",
                                    );
                                });

                            let resp =
                            ui.add(egui::Button::new("Add").min_size(Vec2 { x: 50.0, y: 15.0 }));
                            if resp.clicked() && self.new_user.all_ok() {
                                println!("Adding new user");
                                self.users.push(User::from(&self.new_user));
                                self.new_user.clear();
                            }
                        });
                    });
                });
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            TableBuilder::new(ui)
                .striped(true)
                .column(Column::initial(40.0).resizable(false)) // ID
                .column(Column::initial(180.0).resizable(true)) // Name
                .column(Column::initial(100.0).resizable(false)) // Level
                .header(20.0, |mut header| {
                    header.col(|ui| {
                        ui.label("#");
                    });
                    header.col(|ui| {
                        ui.label("Név");
                    });
                    header.col(|ui| {
                        ui.label("Szint");
                    });
                })
                .body(|mut body| {
                    for (x, result) in self.users.iter().enumerate() {
                        body.row(14.0, |mut row| {
                            if let Some(x) = &self.current_user {
                                if x.name == result.name {
                                    row.set_selected(true);
                                }
                            }

                            row.col(|ui| {
                                ui.label(format!("{}", x + 1));
                            });
                            row.col(|ui| {
                                ui.label(&result.name);
                            });
                            row.col(|ui| {
                                ui.label(format!("{:?}", result.level));
                            });
                        });
                    }
                });
        });
    }
}
