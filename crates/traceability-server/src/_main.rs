#![allow(non_snake_case)]

use egui::{Vec2, ViewportCommand};
use log::{debug, info};
use std::{
    sync::{
        mpsc::{self, SyncSender},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::net::TcpListener;
use tray_item::IconSource;

mod tcp;
use tcp::*;

mod tray;
use tray::*;

mod auth;
use auth::*;

#[derive(Debug, Clone, Copy, PartialEq)]
enum AppMode {
    Enabled,
    OffLine,
}

#[derive(Debug, Clone, Copy)]
enum AwMode {
    None,
    Login,
    Admin,
}

struct AuthWindow {
    tx: SyncSender<Message>,
    auth_mode: Arc<Mutex<AppMode>>,
    win_mode: Arc<Mutex<AwMode>>,
    context: Arc<Mutex<Option<egui::Context>>>,

    users: Vec<User>,
    user_name: String,
    user_pass: String,
}

impl AuthWindow {
    fn new() -> Self {
        let (tx, rx) = mpsc::sync_channel(1);

        let mode = Arc::new(Mutex::new(AppMode::Enabled));
        let tcp_mode = mode.clone();
        let tcp_tx = tx.clone();

        // spawn TCP thread
        tokio::spawn(async move {
            let mut tcp_server = TcpServer::new(tcp_mode, tcp_tx.clone());
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
        let context: Arc<Mutex<Option<egui::Context>>> = Arc::new(Mutex::new(None));
        let tray_context = context.clone();

        let app_mode = mode.clone();
        let aw_mode = Arc::new(Mutex::new(AwMode::None));
        let tray_aw_mode = aw_mode.clone();
        

        tokio::spawn(async move {
            loop {
                match rx.recv() {
                    Ok(Message::Quit) => {
                        info!("Stoping server due user request");
                        if let Some(x) = tray_context.lock().unwrap().as_ref() {
                            x.send_viewport_cmd(ViewportCommand::Close);
                        }
                        break;
                    }
                    Ok(Message::Settings) => {
                        *tray_aw_mode.lock().unwrap() = AwMode::Admin;
                        if let Some(x) = tray_context.lock().unwrap().as_ref() {
                            x.request_repaint();
                        }
                    }
                    Ok(Message::Red) => {
                        tray.set_icon(IconSource::Resource("red-icon")).unwrap();
                    }
                    Ok(Message::Yellow) => {
                        tray.set_icon(IconSource::Resource("yellow-icon")).unwrap();
                    }
                    Ok(Message::Green) => {
                        tray.set_icon(IconSource::Resource("green-icon")).unwrap()
                    }
                    Ok(Message::LogIn) => {
                        info!("Login started");
                        *tray_aw_mode.lock().unwrap() = AwMode::Login;
                        if let Some(x) = tray_context.lock().unwrap().as_ref() {
                            x.request_repaint();
                        }
                    }
                    Ok(Message::LogInS(x)) => {
                        tray.inner_mut().set_label(&format!("Hi, {}!", x), tray_ids[0]).unwrap();
                    }
                    Ok(Message::LogOut) => {
                        info!("Logged out");
                        *app_mode.lock().unwrap() = AppMode::Enabled;

                        *tray_aw_mode.lock().unwrap() = AwMode::None;
                        if let Some(x) = tray_context.lock().unwrap().as_ref() {
                            x.request_repaint();
                        }
                    }
                    _ => {}
                }
            }
        });

        Self {
            tx,
            auth_mode: mode,
            win_mode: aw_mode,
            context,

            users: load_user_list(),
            user_name: String::new(),
            user_pass: String::new(),
        }
    }
}

impl eframe::App for AuthWindow {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        //ctx.request_repaint_after(Duration::from_secs(1));

        *self.context.lock().unwrap() = Some(ctx.clone());

        let mode = *self.win_mode.lock().unwrap();
        match mode {
            AwMode::None => {}
            AwMode::Login => {
                egui::CentralPanel::default().show(&ctx, |ui| {
                    ui.text_edit_singleline(&mut self.user_name);
                    ui.add(egui::TextEdit::singleline(&mut self.user_pass).password(true));

                    if ui.button("Login").clicked() {
                        debug!("Login as {}", self.user_name);
                        for user in &self.users {
                            if user.name == self.user_name && user.check_pw(self.user_pass.clone()) {
                                debug!("Login Success!");
                                self.tx.send(Message::LogInS(self.user_name.clone())).unwrap();
                                
                                break;
                            }
                        }
                    }

                });
            }
            AwMode::Admin => {}
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }

    env_logger::init();
    info!("Starting server");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
        .with_inner_size(Vec2{x: 200.0, y: 100.0})
        .with_position((3200.0, 1200.0))
        .with_close_button(false),
        ..Default::default()
    };

    _ = eframe::run_native(
        "Traceability",
        options,
        Box::new(|_| Box::new(AuthWindow::new())),
    );

    Ok(())
}
