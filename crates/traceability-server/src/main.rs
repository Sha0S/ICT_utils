#![allow(non_snake_case)]

use anyhow::bail;
use std::{
    io,
    path::PathBuf,
    sync::{
        mpsc::{self, SyncSender},
        Arc, Mutex,
    },
};
use tiberius::{Client, Query};
use tokio::{
    io::Interest,
    net::{TcpListener, TcpStream},
};
use tokio_util::compat::TokioAsyncWriteCompatExt;
use tray_item::{IconSource, TrayItem};

use ICT_config::*;

static CONFIG: &str = "config.ini";
static GOLDEN: &str = "golden_samples";

static LIMIT: i32 = 3;
static LIMIT_2: i32 = 6;

enum Message {
    Quit,
    Login,

    Green,
    Yellow,
    Red,
}

struct App {
    config: Config,
    golden_samples: Vec<String>,
}

impl Default for App {
    fn default() -> Self {
        let config = match Config::read(PathBuf::from(CONFIG)) {
            Ok(c) => c,
            Err(e) => {
                println!("{e}");
                std::process::exit(0)
            }
        };

        App {
            config,
            golden_samples: load_gs_list(PathBuf::from(GOLDEN)),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let server = Arc::new(Mutex::new(App::default()));
    let (tx, rx) = mpsc::sync_channel(1);

    let tcp_server = server.clone();
    let tcp_tx = tx.clone();

    // spawn TCP thread
    tokio::spawn(async move {
        let MES_server = tcp_server
            .lock()
            .unwrap()
            .config
            .get_MES_server()
            .to_owned();
        println!("Connecting to: {}", MES_server);
        let listener = TcpListener::bind(MES_server)
            .await
            .expect("ERR: can't connect to socket!");

        tcp_tx.send(Message::Green).unwrap();

        loop {
            if let Ok((stream, _)) = listener.accept().await {
                handle_client(tcp_server.clone(), tcp_tx.clone(), stream).await;
            }
        }
    });

    // UI thread - to_do()

    let mut tray =
        TrayItem::new("ICT Traceability Server", IconSource::Resource("red-icon")).unwrap();

    tray.add_label("ICT Traceability Server").unwrap();

    tray.inner_mut().add_separator().unwrap();

    let login_tx = tx.clone();
    tray.add_menu_item("Login", move || {
        login_tx.send(Message::Login).unwrap();
    })
    .unwrap();

    tray.inner_mut().add_separator().unwrap();

    let quit_tx = tx.clone();
    tray.add_menu_item("Quit", move || {
        quit_tx.send(Message::Quit).unwrap();
    })
    .unwrap();

    loop {
        match rx.recv() {
            Ok(Message::Quit) => {
                println!("Quit");
                break;
            }
            Ok(Message::Red) => {
                tray.set_icon(IconSource::Resource("red-icon")).unwrap();
            }
            Ok(Message::Yellow) => {
                tray.set_icon(IconSource::Resource("yellow-icon")).unwrap();
            }
            Ok(Message::Green) => tray.set_icon(IconSource::Resource("green-icon")).unwrap(),
            Ok(Message::Login) => {
                // to_do
            }
            _ => {}
        }
    }

    Ok(())
}

async fn handle_client(server: Arc<Mutex<App>>, tx: SyncSender<Message>, stream: TcpStream) {
    let response = loop {
        if let Ok(ready) = stream.ready(Interest::READABLE).await {
            if ready.is_readable() {
                let mut buf = [0; 1024];
                match stream.try_read(&mut buf) {
                    Ok(_) => {
                        let message = String::from_utf8_lossy(&buf).to_string();
                        println!("{message}");
                        break process_message(server, tx.clone(), message).await;
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        continue;
                    }
                    Err(e) => {
                        tx.send(Message::Red).unwrap();
                        break format!("{e}");
                    }
                }
            }
        }
    };

    println!("Response: {response}");

    loop {
        if let Ok(ready) = stream.ready(Interest::WRITABLE).await {
            if ready.is_writable() {
                match stream.try_write(response.as_bytes()) {
                    Ok(_) => {
                        break;
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        continue;
                    }
                    Err(e) => {
                        tx.send(Message::Red).unwrap();
                        println!("ERR: {e}");
                        break;
                    }
                }
            }
        }
    }
}

async fn process_message(
    server: Arc<Mutex<App>>,
    tx: SyncSender<Message>,
    input: String,
) -> String {
    let tokens: Vec<&str> = input.split('|').map(|f| f.trim_end_matches('\0')).collect();
    println!("Tokens: {:?}", tokens);
    match tokens[0] {
        "START" => {
            if tokens.len() < 3 {
                tx.send(Message::Yellow).unwrap();
                String::from("Err: Missing token!")
            } else {
                match start_board(server, tokens).await {
                    Ok(x) => {
                        tx.send(Message::Green).unwrap();
                        x
                    }

                    Err(x) => {
                        tx.send(Message::Red).unwrap();
                        format!("ERR: {x}")
                    }
                }
            }
        }
        "END" => {
            todo!()
        }
        _ => {
            tx.send(Message::Red).unwrap();
            String::from("ERR: Unknown token recieved!") }
    }
}

async fn connect(
    tib_config: tiberius::Config,
) -> anyhow::Result<tiberius::Client<tokio_util::compat::Compat<TcpStream>>> {
    let tcp = TcpStream::connect(tib_config.get_addr()).await?;
    tcp.set_nodelay(true)?;
    let client = Client::connect(tib_config, tcp.compat_write()).await?;

    Ok(client)
}

async fn start_board(server: Arc<Mutex<App>>, tokens: Vec<&str>) -> anyhow::Result<String> {
    let dmc = tokens[1].to_string();
    let boards = tokens[2].parse::<u8>()?;

    println!("Starting new board: {dmc}");

    // A) Is it a golden sample

    if server.lock().unwrap().golden_samples.contains(&dmc) {
        return Ok(String::from("GS"));
    }

    // Tiberius configuartion:

    let sql_server = server.lock().unwrap().config.get_server().to_owned();
    let sql_user = server.lock().unwrap().config.get_username().to_owned();
    let sql_pass = server.lock().unwrap().config.get_password().to_owned();

    let mut tib_config = tiberius::Config::new();
    tib_config.host(sql_server);
    tib_config.authentication(tiberius::AuthMethod::sql_server(sql_user, sql_pass));
    tib_config.trust_cert(); // Most likely not needed.

    let mut client_tmp = connect(tib_config.clone()).await;
    let mut tries = 0;
    while client_tmp.is_err() && tries < 3 {
        client_tmp = connect(tib_config.clone()).await;
        tries += 1;
    }

    if client_tmp.is_err() {
        bail!("Connection to DB failed!")
    }
    let mut client = client_tmp.unwrap();

    // USE [DB]
    let qtext = format!("USE [{}]", server.lock().unwrap().config.get_database());
    let query = Query::new(qtext);
    query.execute(&mut client).await.unwrap();

    // QUERY #1:

    let qtext = format!(
        "SELECT COUNT(*) FROM [dbo].[SMT_Test] WHERE [Serial_NMBR] = '{}'",
        dmc
    );

    let query = Query::new(qtext);
    let result = query.query(&mut client).await?;

    let tested_total;
    if let Some(row) = result.into_row().await? {
        if let Some(x) = row.get::<i32, usize>(0) {
            tested_total = x;
            if tested_total < LIMIT {
                return Ok(format!("OK: {tested_total}"));
            } else if tested_total >= LIMIT_2 {
                return Ok(format!("NK: {tested_total}"));
            }
        } else {
            bail!("Q#1 Parsing error.");
        }
    } else {
        bail!("Q#1 result is none.");
    }

    // Check each panel if LIMIT <= tested_total < LIMIT_2
    // No single board should have 'failed' LIMIT times
    // QUERY #2:

    let targets: Vec<String> = increment_sn(&dmc, boards)
        .iter()
        .map(|f| format!("'{f}'"))
        .collect();
    let targets_string = targets.join(", ");

    let qtext = format!(
        "SELECT COUNT(*) AS Fails
        FROM [dbo].[SMT_Test]
        WHERE [Serial_NMBR] IN ({})
        AND [Result] = 'Failed'
        GROUP BY [Serial_NMBR]
		ORDER BY Fails DESC;",
        targets_string
    );

    let query = Query::new(qtext);
    let result = query.query(&mut client).await?;

    if let Some(row) = result.into_row().await? {
        if let Some(x) = row.get::<i32, usize>(0) {
            if x >= LIMIT {
                Ok(format!("NK: {x} ({tested_total})"))
            } else {
                Ok(format!("OK: {x} ({tested_total})"))
            }
        } else {
            bail!("Q#2 Parsing error.");
        }
    } else {
        Ok(format!("OK: 0 ({tested_total})")) // Q#2 will return NONE, if the MB has no 'failed' results at all.
    }
}
