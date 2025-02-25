#![allow(non_snake_case)]
#![allow(clippy::collapsible_match)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use std::{
    io::Write,
    net::TcpStream,
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};

use chrono::DateTime;
use log::{debug, error, info, warn};
use tokio::{net::TcpListener, time::sleep};
use tray_item::IconSource;
use winsafe::{co::ES, gui, prelude::*, AnyResult};

mod tcp;
use tcp::*;

mod tray;
use tray::*;

use ICT_auth::*;

const TIME_LIMIT: i64 = 5;

#[derive(Debug, Clone, Copy, PartialEq)]
enum AppMode {
    None,
    Enabled,
    OffLine,
    Override,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }

    env_logger::init();
    info!("Starting server");

    let (tx, rx) = mpsc::sync_channel(1);

    let mode = Arc::new(Mutex::new(AppMode::Enabled));
    let tcp_mode = mode.clone();
    let tcp_tx = tx.clone();

    let act_user_name: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let tcp_user = act_user_name.clone();
    let act_tcp_address: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let tcp_address = act_tcp_address.clone();

    // spawn TCP thread
    tokio::spawn(async move {
        let mut tcp_server = TcpServer::new(tcp_mode, tcp_tx.clone(), tcp_user);

        let MES_server = tcp_server.config.get_MES_server().to_owned();
        info!("Connecting to: {}", MES_server);

        *tcp_address.lock().unwrap() = MES_server.clone();

        let listener = match TcpListener::bind(MES_server).await {
            Ok(x) => x,
            Err(_) => {
                error!("ER: can't connect to socket!");
                tcp_tx.send(Message::FatalError).unwrap();
                panic!("ER: can't connect to socket!");
            }
        };

        if tcp_server.update_golden_samples().await.is_err() {
            error!("Failed to load the list of golden samples!");
            tcp_tx.send(Message::SetIcon(IconCollor::Red)).unwrap();
        } else {
            tcp_tx.send(Message::SetIcon(IconCollor::Green)).unwrap();
        }

        loop {
            if let Ok((stream, _)) = listener.accept().await {
                tcp_server.handle_client(stream).await;
            }
        }
    });

    let (mut tray, tray_ids) = init_tray(tx.clone());
    let mut active_user: Option<User> = None;
    let mut gs_user_name: String = String::new();
    let mut logout_timer: Option<DateTime<chrono::Local>> = None;
    let mut last_color = String::new();

    let timer_tx = tx.clone();
    tokio::spawn(async move {
        loop {
            timer_tx.send(Message::UpdateTimer).unwrap();
            sleep(Duration::from_secs(1)).await;
        }
    });

    // For Kaizen FCT, automatic scanning for logs
    if let Ok(config) = ICT_config::Config::read(ICT_config::CONFIG) {
        if config.get_station_name() == "Kaizen FCT" {
            let fct_timer_tx = tx.clone();
            tokio::spawn(async move {
                loop {
                    fct_timer_tx.send(Message::StartFctUpdate).unwrap();
                    sleep(Duration::from_secs(60)).await;
                }
            });
        }
    }

    loop {
        match rx.recv() {
            Ok(Message::Quit) => {
                info!("Stoping server due user request");
                break;
            }
            Ok(Message::FatalError) => {
                error!("Fatal error encountered, shuting down!");
                break;
            }
            Ok(Message::Settings) => {
                let _ = std::process::Command::new("auth_manager.exe").spawn();
            }
            Ok(Message::SetIcon(icon)) => {
                debug!("Icon change requested: {:?}", icon);

                let c_mode = *mode.lock().unwrap();

                let target_col = match icon {
                    IconCollor::Green => match c_mode {
                        AppMode::None | AppMode::Enabled => "green-icon",
                        AppMode::OffLine => "grey-icon",
                        AppMode::Override => "purple-icon",
                    },

                    IconCollor::Yellow => "yellow-icon",
                    IconCollor::Red => "red-icon",
                    IconCollor::Grey => "grey-icon",
                    IconCollor::Purple => "purple-icon",
                };

                if target_col == last_color {
                    continue;
                }
                if tray.set_icon(IconSource::Resource(target_col)).is_ok() {
                    debug!("Icon set to: {target_col}");
                    last_color = target_col.to_owned();
                } else {
                    warn!("Failed to change icon to: {target_col}");
                }
            }
            Ok(Message::UpdateTimer) => {
                if let Some(x) = logout_timer {
                    let time_elapsed = chrono::Local::now() - x;

                    if time_elapsed >= chrono::TimeDelta::minutes(TIME_LIMIT) {
                        tx.send(Message::LogOut).unwrap();
                    } else {
                        let seconds_left = 60 * TIME_LIMIT - time_elapsed.num_seconds();
                        tray.inner_mut()
                            .set_label(&format!("Logout in: {}s", seconds_left), tray_ids[0])
                            .unwrap();
                    }
                }
            }
            Ok(Message::LogInStart) => {
                info!("Login started");
                let login_tx = tx.clone();
                tokio::spawn(async move {
                    let res = login();
                    login_tx.send(Message::LogIn(res)).unwrap();
                });
            }
            Ok(Message::LogIn(user)) => {
                if let Ok(res) = user {
                    info!("Login result: {res:?}");
                    logout_timer = Some(chrono::Local::now());
                    let level = res.level;
                    *act_user_name.lock().unwrap() = res.name.clone();

                    if let Some(u) = active_user {
                        if u.name != res.name {
                            // if we log in as a diff user, then reset mode to enabled
                            tx.send(Message::SetMode(AppMode::Enabled)).unwrap();
                        }
                    }

                    active_user = Some(res);
                    update_tray_login(&mut tray, &tray_ids, level)
                }
            }
            Ok(Message::LogOut) => {
                if active_user.is_some() {
                    info!("Loging out");
                    active_user = None;
                    logout_timer = None;
                    act_user_name.lock().unwrap().clear();
                    tx.send(Message::SetMode(AppMode::Enabled)).unwrap();
                    update_tray_logout(&mut tray, &tray_ids);
                }
            }
            Ok(Message::SetMode(m)) => {
                debug!("AppMode requested: {m:?}");
                match m {
                    AppMode::Enabled => {
                        *mode.lock().unwrap() = m;
                        tx.send(Message::SetIcon(IconCollor::Green)).unwrap();
                        info!("AppMode set to {m:?}");
                    }
                    AppMode::OffLine => {
                        if active_user.is_some() {
                            *mode.lock().unwrap() = m;
                            tx.send(Message::SetIcon(IconCollor::Grey)).unwrap();
                            info!("AppMode set to {m:?}");
                        }
                    }
                    AppMode::Override => {
                        if let Some(user) = active_user.as_ref() {
                            if user.level > UserLevel::Technician {
                                *mode.lock().unwrap() = m;
                                tx.send(Message::SetIcon(IconCollor::Purple)).unwrap();
                                info!("AppMode set to {m:?}");
                            }
                        }
                    }
                    _ => {
                        debug!("Setmode({m:?}) called");
                    }
                }
            }
            Ok(Message::UpdateGSList) => {
                info!("Recieved user request to update GS list");
                let addr = act_tcp_address.lock().unwrap().clone();
                if send_tcp_message(addr, "UPDATE_GOLDEN_SAMPLES").is_err() {
                    let _ = tx.send(Message::SetIcon(IconCollor::Yellow));
                    error!("Failed to send TCP message!");
                }
            }
            Ok(Message::AddGS) => {
                info!("Recieved user request to add GS");
                if active_user.is_some() {
                    gs_user_name = active_user.as_ref().unwrap().name.clone();
                    let window_tx = tx.clone();
                    tokio::spawn(async move {
                        let res = input_gs();
                        println!("{:?}", res);
                        window_tx.send(Message::NewGS(res)).unwrap();
                    });
                }
            }
            Ok(Message::NewGS(gs_result)) => match gs_result {
                Ok(gs) => {
                    info!("New GS serial: {}", gs);
                    let addr = act_tcp_address.lock().unwrap().clone();
                    match send_tcp_message(addr, &format!("NEW_GS|{gs}|{gs_user_name}")) {
                        Ok(_) => info!("Sent TCP request to add new GS"),
                        Err(e) => error!("Failed to send TCP request to add new GS: {e}"),
                    }
                }
                Err(e) => {
                    warn!("Adding GS failed: {}", e);
                }
            },

            Ok(Message::StartFctUpdate) => {
                info!("Starting automatic FCT uploads");
                let addr = act_tcp_address.lock().unwrap().clone();
                if send_tcp_message(addr, "FCT_AUTO_UPDATE").is_err() {
                    let _ = tx.send(Message::SetIcon(IconCollor::Yellow));
                    error!("Failed to send TCP message!");
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn send_tcp_message(addr: String, message: &str) -> anyhow::Result<()> {
    let mut stream = TcpStream::connect(addr)?;
    stream.write_all(message.as_bytes())?;

    Ok(())
}

/*
MyLoginWindow
*/

fn login() -> AnyResult<User> {
    MyLoginWindow::new().run()
}

#[derive(Clone)]
pub struct MyLoginWindow {
    wnd: gui::WindowMain, // responsible for managing the window
    edit_name: gui::Edit,
    edit_pass: gui::Edit,
    btn_login: gui::Button, // a button

    users: Vec<User>,
    selected: Arc<Mutex<Option<usize>>>,
}

impl Default for MyLoginWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl MyLoginWindow {
    pub fn new() -> Self {
        let wnd = gui::WindowMain::new(
            // instantiate the window manager
            gui::WindowMainOpts {
                title: "Login".to_owned(),
                size: (300, 150),
                ..Default::default() // leave all other options as default
            },
        );

        let edit_name = gui::Edit::new(
            &wnd,
            gui::EditOpts {
                text: "Name".to_owned(),
                position: (20, 20),
                width: 260,
                ..Default::default()
            },
        );

        let edit_pass = gui::Edit::new(
            &wnd,
            gui::EditOpts {
                text: "Pass".to_owned(),
                position: (20, 50),
                width: 260,
                edit_style: ES::PASSWORD,
                ..Default::default()
            },
        );

        let btn_login = gui::Button::new(
            &wnd, // the window manager is the parent of our button
            gui::ButtonOpts {
                text: "OK".to_owned(),
                position: (20, 80),
                ..Default::default()
            },
        );

        let mut new_self = Self {
            wnd,
            edit_name,
            edit_pass,
            btn_login,
            users: load_user_list(),
            selected: Arc::new(Mutex::new(None)),
        };
        new_self.events(); // attach our events
        new_self
    }

    pub fn run(&self) -> AnyResult<User> {
        self.wnd.run_main(None)?; // simply let the window manager do the hard work

        if let Some(i) = *self.selected.lock().unwrap() {
            Ok(self.users[i].clone())
        } else {
            AnyResult::Err("Failed login".into())
        }
    }

    fn events(&mut self) {
        let sel_2 = self.selected.clone();
        let self2 = self.clone();
        self2.btn_login.on().bn_clicked(move || {
            // button click event
            for (i, user) in self2.users.iter().enumerate() {
                if user.name == self2.edit_name.text() && user.check_pw(&self2.edit_pass.text()) {
                    *sel_2.lock().unwrap() = Some(i);
                    self2.wnd.hwnd().DestroyWindow()?;
                    break;
                }
            }
            Ok(())
        });
    }
}

/*
StringInputWindow
*/

fn input_gs() -> AnyResult<String> {
    StringInputWindow::new().run()
}

#[derive(Clone)]
pub struct StringInputWindow {
    wnd: gui::WindowMain, // responsible for managing the window
    edit: gui::Edit,
    btn_ok: gui::Button,     // a button
    btn_cancel: gui::Button, // a button

    ret_text: Arc<Mutex<String>>,
}

impl Default for StringInputWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl StringInputWindow {
    pub fn new() -> Self {
        let wnd = gui::WindowMain::new(
            // instantiate the window manager
            gui::WindowMainOpts {
                title: "Input new GS serial".to_owned(),
                size: (300, 100),
                ..Default::default() // leave all other options as default
            },
        );

        let edit = gui::Edit::new(
            &wnd,
            gui::EditOpts {
                text: "serial".to_owned(),
                position: (20, 20),
                width: 260,
                ..Default::default()
            },
        );

        let btn_ok = gui::Button::new(
            &wnd, // the window manager is the parent of our button
            gui::ButtonOpts {
                text: "OK".to_owned(),
                position: (40, 60),
                ..Default::default()
            },
        );

        let btn_cancel = gui::Button::new(
            &wnd, // the window manager is the parent of our button
            gui::ButtonOpts {
                text: "Cancel".to_owned(),
                position: (150, 60),
                ..Default::default()
            },
        );

        let mut new_self = Self {
            wnd,
            edit,
            btn_ok,
            btn_cancel,
            ret_text: Arc::new(Mutex::new(String::new())),
        };
        new_self.events(); // attach our events
        new_self
    }

    pub fn run(&self) -> AnyResult<String> {
        self.wnd.run_main(None)?; // simply let the window manager do the hard work

        let ret: String = self.ret_text.lock().unwrap().clone();
        if !ret.is_empty() {
            Ok(ret)
        } else {
            AnyResult::Err("Input Canceled".into())
        }
    }

    fn events(&mut self) {
        let self2 = self.clone();
        self2.btn_ok.on().bn_clicked(move || {
            *self2.ret_text.lock().unwrap() = self2.edit.text();
            self2.wnd.hwnd().DestroyWindow()?;
            Ok(())
        });

        let self2 = self.clone();
        self2.btn_cancel.on().bn_clicked(move || {
            self2.wnd.hwnd().DestroyWindow()?;
            Ok(())
        });
    }
}
