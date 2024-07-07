#![allow(non_snake_case)]

use std::{
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};

use chrono::DateTime;
use log::{debug, info};
use tokio::{net::TcpListener, time::sleep};
use tray_item::IconSource;
use winsafe::{co::ES, gui, prelude::*, AnyResult};

mod tcp;
use tcp::*;

mod tray;
use tray::*;

mod auth;
use auth::*;

const TIME_LIMIT: i64 = 10;

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

    // spawn TCP thread
    tokio::spawn(async move {
        let mut tcp_server = TcpServer::new(tcp_mode, tcp_tx.clone(), tcp_user);
        let MES_server = tcp_server.config.get_MES_server().to_owned();
        info!("Connecting to: {}", MES_server);
        let listener = TcpListener::bind(MES_server)
            .await
            .expect("ER: can't connect to socket!");

        tcp_tx.send(Message::Green).unwrap();

        loop {
            if let Ok((stream, _)) = listener.accept().await {
                handle_client(&mut tcp_server, stream).await;
            }
        }
    });

    let (mut tray, tray_ids) = init_tray(tx.clone());
    let mut active_user = None;
    let mut logout_timer: Option<DateTime<chrono::Local>> = None;

    loop {
        match rx.try_recv() {
            Ok(Message::Quit) => {
                info!("Stoping server due user request");
                break;
            }
            Ok(Message::Settings) => {}
            Ok(Message::Red) => {
                tray.set_icon(IconSource::Resource("red-icon")).unwrap();
            }
            Ok(Message::Yellow) => {
                tray.set_icon(IconSource::Resource("yellow-icon")).unwrap();
            }
            Ok(Message::Green) => tray.set_icon(IconSource::Resource("green-icon")).unwrap(),
            Ok(Message::LogIn) => {
                info!("Login started");
                if let Ok(res) = login() {
                    debug!("Login result: {res:?}");
                    logout_timer = Some(chrono::Local::now());
                    let level = res.level.clone();
                    *act_user_name.lock().unwrap() = res.name.clone();
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
                        info!("AppMode set to {m:?}");
                    }
                    AppMode::OffLine => {
                        if active_user.is_some() {
                            *mode.lock().unwrap() = m;
                            info!("AppMode set to {m:?}");
                        }
                    }
                    AppMode::Override => {
                        if let Some(user) = active_user.as_ref() {
                            if user.level != UserLevel::Tech {
                                *mode.lock().unwrap() = m;
                                info!("AppMode set to {m:?}");
                            }
                        }
                    }
                    _ => {
                        debug!("Setmode({m:?}) called");
                    }
                }
            }
            _ => {}
        }

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

        sleep(Duration::from_millis(500)).await;
    }

    Ok(())
}

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
                if user.name == self2.edit_name.text() && user.check_pw(self2.edit_pass.text()) {
                    *sel_2.lock().unwrap() = Some(i);
                    self2.wnd.hwnd().DestroyWindow()?;
                    break;
                }
            }
            Ok(())
        });
    }
}
