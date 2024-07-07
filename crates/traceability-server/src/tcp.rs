#![allow(non_snake_case)]

use anyhow::bail;
use log::{debug, error, info, warn};
use std::{
    io,
    path::PathBuf,
    sync::{mpsc::SyncSender, Arc, Mutex},
};
use tiberius::{Client, Query};
use tokio::{io::Interest, net::TcpStream};
use tokio_util::compat::TokioAsyncWriteCompatExt;

use ICT_config::*;

use crate::{AppMode, IconCollor, Message};

static CONFIG: &str = "config.ini";
static GOLDEN: &str = "golden_samples";

static LIMIT: i32 = 3;
static LIMIT_2: i32 = 6;

pub struct TcpServer {
    pub config: Config,
    mode: Arc<Mutex<AppMode>>,
    tx: SyncSender<Message>,

    last_mode: AppMode,
    user: Arc<Mutex<String>>,
    logs: Vec<String>,
    golden_samples: Vec<String>,
}

impl TcpServer {
    pub fn new(
        mode: Arc<Mutex<AppMode>>,
        tx: SyncSender<Message>,
        user: Arc<Mutex<String>>,
    ) -> Self {
        let config = match Config::read(PathBuf::from(CONFIG)) {
            Ok(c) => c,
            Err(e) => {
                error!("{e}");
                std::process::exit(0)
            }
        };

        if config.get_MES_server().is_empty() || config.get_station_name().is_empty() {
            error!("Missing fields from config file!");
            std::process::exit(0)
        }

        TcpServer {
            config,
            mode,
            tx,
            last_mode: AppMode::None,
            user,
            logs: Vec::new(),
            golden_samples: load_gs_list(PathBuf::from(GOLDEN)),
        }
    }

    fn push_mode(&mut self) {
        self.last_mode = *self.mode.lock().unwrap();
    }

    fn check_mode(&self) -> bool {
        *self.mode.lock().unwrap() == self.last_mode
    }

    fn push_log(&mut self, log: &str) {
        self.logs.push(log.to_owned());
    }

    fn extract_logs(&mut self) -> Vec<String> {
        let mut ret = Vec::new();

        std::mem::swap(&mut ret, &mut self.logs);

        ret
    }

    fn get_mode(&self) -> AppMode {
        *self.mode.lock().unwrap()
    }
}

pub async fn handle_client(server: &mut TcpServer, stream: TcpStream) {
    let response = loop {
        if let Ok(ready) = stream.ready(Interest::READABLE).await {
            if ready.is_readable() {
                let mut buf = [0; 1024];
                match stream.try_read(&mut buf) {
                    Ok(_) => {
                        let message = String::from_utf8_lossy(&buf).to_string();
                        info!("Message recieved: {message}");
                        break process_message(server, message).await;
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                        continue;
                    }
                    Err(e) => {
                        server.tx.send(Message::SetIcon(IconCollor::Red)).unwrap();
                        error!("Message read failed: {e}");
                        break format!("{e}");
                    }
                }
            }
        }
    };

    info!("Response: {response}");

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
                        server.tx.send(Message::SetIcon(IconCollor::Red)).unwrap();
                        error!("Message write failed: {e}");
                        break;
                    }
                }
            }
        }
    }
}

async fn process_message(server: &mut TcpServer, input: String) -> String {
    let tokens: Vec<&str> = input.split('|').map(|f| f.trim_end_matches('\0')).collect();
    debug!("Tokens: {:?}", tokens);
    match tokens[0] {
        "START" => {
            if tokens.len() < 3 {
                server
                    .tx
                    .send(Message::SetIcon(IconCollor::Yellow))
                    .unwrap();
                error!("Missing token after START!");
                String::from("ER: Missing token!")
            } else {
                match start_panel(server, tokens).await {
                    Ok(x) => {
                        server.tx.send(Message::SetIcon(IconCollor::Green)).unwrap();
                        x
                    }

                    Err(x) => {
                        server.tx.send(Message::SetIcon(IconCollor::Red)).unwrap();
                        error!("Failed to START panel: {x}");
                        format!("ER: {x}")
                    }
                }
            }
        }
        "LOG" => {
            if let Some(log) = tokens.get(1) {
                server.push_log(log);
                String::from("OK")
            } else {
                server
                    .tx
                    .send(Message::SetIcon(IconCollor::Yellow))
                    .unwrap();
                error!("Missing token after LOG!");
                String::from("ER: Missing log token!")
            }
        }
        "END" => match end_panel(server).await {
            Ok(x) => {
                server.tx.send(Message::SetIcon(IconCollor::Green)).unwrap();
                x
            }

            Err(x) => {
                server.tx.send(Message::SetIcon(IconCollor::Red)).unwrap();
                error!("Failed to END panel: {x}");
                format!("ER: {x}")
            }
        },
        _ => {
            server
                .tx
                .send(Message::SetIcon(IconCollor::Yellow))
                .unwrap();
            warn!("Unknown token recieved! {}", tokens[0]);
            String::from("ER: Unknown token recieved!")
        }
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

async fn start_panel(server: &mut TcpServer, tokens: Vec<&str>) -> anyhow::Result<String> {
    let dmc = tokens[1].to_string();
    let boards = tokens[2].parse::<u8>()?;
    let mode = server.get_mode();

    debug!("Starting new board: {dmc}");

    // A) Is it a golden sample

    if server.golden_samples.contains(&dmc) {
        server.push_mode();
        return Ok(String::from("GS"));
    }

    // B) traceability is disabled
    if mode != AppMode::Enabled {
        warn!("Mode is set to {mode:?}");
        server.push_mode();
        return Ok(String::from("OK: MES is disabled!"));
    }

    // C) Query dmc from the SQL db
    // Tiberius configuartion:

    let sql_server = server.config.get_server().to_owned();
    let sql_user = server.config.get_username().to_owned();
    let sql_pass = server.config.get_password().to_owned();

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
    let qtext = format!("USE [{}]", server.config.get_database());
    let query = Query::new(qtext);
    query.execute(&mut client).await?;

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
                server.push_mode();
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
                server.push_mode();
                Ok(format!("OK: {x} ({tested_total})"))
            }
        } else {
            bail!("Q#2 Parsing error.");
        }
    } else {
        server.push_mode();
        Ok(format!("OK: 0 ({tested_total})")) // Q#2 will return NONE, if the MB has no 'failed' results at all.
    }
}

async fn end_panel(server: &mut TcpServer) -> anyhow::Result<String> {
    let logs = server.extract_logs();
    let mode = server.get_mode();
    let mut note = String::new();

    if logs.is_empty() {
        error!("Log buffer is empty!");
        bail!("Log buffer is empty!");
    }

    if mode == AppMode::OffLine {
        warn!("Mode is set to {mode:?}");
        return Ok(String::from("OK: Off-line mode"));
    } else if mode == AppMode::Override {
        note = format!("Tested by: {}.", server.user.lock().unwrap().as_str());
    }

    let mut ict_logs = Vec::new();
    for log in logs {
        debug!("Parsing log: {log}");
        if let Ok(l) = ICT_log_file::LogFile::load(&PathBuf::from(&log)) {
            if l.is_ok() {
                ict_logs.push(l);
            } else {
                error!("Could not process log: {log}");
                bail!("Could not process log!")
            }
        } else {
            error!("Logfile parsing failed!");
            bail!("Logfile parsing failed!");
        }
    }

    if ict_logs.is_empty() {
        error!("ICT log buffer is empty!");
        bail!("ICT log buffer is empty!");
    }

    if !server.check_mode() {
        error!("Error processing panel!");
        bail!("Error processing panel!");
    }

    // Tiberius configuartion:

    let sql_server = server.config.get_server().to_owned();
    let sql_user = server.config.get_username().to_owned();
    let sql_pass = server.config.get_password().to_owned();

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
    let mut client = client_tmp?;

    // USE [DB]
    let qtext = format!("USE [{}]", server.config.get_database());
    let query = Query::new(qtext);
    query.execute(&mut client).await?;

    let station = server.config.get_station_name().to_owned();

    // Upload new results
    let mut qtext = String::from(
        "INSERT INTO [dbo].[SMT_Test] 
        ([Serial_NMBR], [Station], [Result], [Date_Time], [Log_File_Name], [SW_Version], [Notes])
        VALUES",
    );
    for log in ict_logs {
        let mut final_note = note.clone();
        if log.get_status() != 0 {
            let failed_tests = log.get_failed_tests().join(", ");
            final_note += &format!(" Failed: {}", failed_tests);
        }
        final_note.truncate(200);
        qtext += &format!(
            "('{}', '{}', '{}', '{}', '{}', '{}', '{}'),",
            log.get_DMC(),
            station,
            if log.get_status() == 0 {
                "Passed"
            } else {
                "Failed"
            },
            ICT_log_file::u64_to_time(log.get_time_end()),
            log.get_source().to_string_lossy(),
            log.get_SW_ver(),
            final_note
        );
    }
    qtext.pop(); // removes last ','

    let query = Query::new(qtext);
    query.execute(&mut client).await?;

    Ok(String::from("OK"))
}
