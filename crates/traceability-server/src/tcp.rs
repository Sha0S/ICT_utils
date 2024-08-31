#![allow(non_snake_case)]

use anyhow::bail;
use log::{debug, error, info, warn};
use std::{
    io,
    path::PathBuf,
    sync::{mpsc::SyncSender, Arc, Mutex},
};
use tiberius::{Client, Query};
use tokio::{
    io::{AsyncWriteExt, Interest},
    net::TcpStream,
};
use tokio_stream::StreamExt;
use tokio_util::compat::TokioAsyncWriteCompatExt;

use ICT_config::*;

use crate::{AppMode, IconCollor, Message};

static CONFIG: &str = "config.ini";

static LIMIT: i32 = 3;
static LIMIT_2: i32 = 6;

pub struct TcpServer {
    pub config: Config,
    mode: Arc<Mutex<AppMode>>,
    tx: SyncSender<Message>,
    client: Option<Client<tokio_util::compat::Compat<TcpStream>>>,

    last_mode: AppMode,
    last_dmc: String,
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
            client: None,
            last_mode: AppMode::None,
            last_dmc: String::new(),
            user,
            logs: Vec::new(),
            golden_samples: Vec::new(),
        }
    }

    pub async fn handle_client(&mut self, stream: TcpStream) {
        let response = loop {
            if let Ok(ready) = stream.ready(Interest::READABLE).await {
                if ready.is_readable() {
                    let mut buf = [0; 1024];
                    match stream.try_read(&mut buf) {
                        Ok(_) => {
                            let message =
                                String::from_utf8_lossy(&buf).trim_matches('\0').to_string();
                            info!("Message recieved: {message}");
                            break self.process_message(message).await;
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                            continue;
                        }
                        Err(e) => {
                            self.tx.send(Message::SetIcon(IconCollor::Red)).unwrap();
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
                            self.tx.send(Message::SetIcon(IconCollor::Red)).unwrap();
                            error!("Message write failed: {e}");
                            break;
                        }
                    }
                }
            }
        }
    }

    async fn process_message(&mut self, input: String) -> String {
        let tokens: Vec<&str> = input.split('|').map(|f| f.trim_end_matches('\0')).collect();
        debug!("Tokens: {:?}", tokens);
        match tokens[0] {
            "START" => {
                if tokens.len() < 3 {
                    self.tx.send(Message::SetIcon(IconCollor::Yellow)).unwrap();
                    error!("Missing token after START!");
                    String::from("ER: Missing token!")
                } else {
                    match self.start_panel(tokens).await {
                        Ok(x) => {
                            self.tx.send(Message::SetIcon(IconCollor::Green)).unwrap();
                            x
                        }

                        Err(x) => {
                            self.tx.send(Message::SetIcon(IconCollor::Red)).unwrap();
                            error!("Failed to START panel: {x}");
                            format!("ER: {x}")
                        }
                    }
                }
            }
            "LOG" => {
                if let Some(log) = tokens.get(1) {
                    self.push_log(log);
                    String::from("OK")
                } else {
                    self.tx.send(Message::SetIcon(IconCollor::Yellow)).unwrap();
                    error!("Missing token after LOG!");
                    String::from("ER: Missing log token!")
                }
            }
            "END" => match self.end_panel().await {
                Ok(x) => {
                    self.tx.send(Message::SetIcon(IconCollor::Green)).unwrap();
                    debug!("END return value: {}", x);
                    x
                }

                Err(x) => {
                    self.tx.send(Message::SetIcon(IconCollor::Red)).unwrap();
                    error!("Failed to END panel: {x}");
                    format!("ER: {x}")
                }
            },
            "UPDATE_GOLDEN_SAMPLES" => {
                info!("Recieved request to update golden samples.");

                match self.update_golden_samples().await {
                    Ok(x) => x,

                    Err(x) => {
                        error!("Failed to PING: {x}");
                        format!("ER: {x}")
                    }
                }
            }
            "NEW_GS" => {
                info!("Recieved request to add golden sample.");
                if tokens.len() == 3 {
                    let serial = tokens[1];
                    let user = tokens[2];
                    debug!("Serial recieved: {}", serial);

                    match self.add_golden_sample(serial, user).await {
                        Ok(x) => x,

                        Err(x) => {
                            error!("Failed to add serial: {x}");
                            format!("ER: {x}")
                        }
                    }
                } else {
                    error!("Missing tokens!");
                    String::from("Missing token!")
                }
            }
            "PING" => {
                info!("PING token recieved! Tokens: {:?}", tokens);

                match self.ping_other_servers(tokens).await {
                    Ok(x) => x,

                    Err(x) => {
                        error!("Failed to PING: {x}");
                        format!("ER: {x}")
                    }
                }
            }
            "TEST" => {
                info!("TEST token recieved! Tokens: {:?}", tokens);
                format!("TEST token recieved! Tokens: {:?}", tokens)
            }
            _ => {
                self.tx.send(Message::SetIcon(IconCollor::Yellow)).unwrap();
                warn!("Unknown token recieved! {}", tokens[0]);
                String::from("ER: Unknown token recieved!")
            }
        }
    }

    async fn connect(&mut self) -> anyhow::Result<String> {
        loop {
            match self.client.as_mut() {
                None => {
                    let sql_server = self.config.get_server().to_owned();
                    let sql_user = self.config.get_username().to_owned();
                    let sql_pass = self.config.get_password().to_owned();

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

                    let mut client_tmp = client_tmp?;

                    let qtext = format!("USE [{}]", self.config.get_database());
                    debug!("USE DB: {}", qtext);
                    client_tmp.execute(qtext, &[]).await?;

                    self.client = Some(client_tmp)
                }
                Some(client) => match client.execute("SELECT 1", &[]).await {
                    Ok(_) => {
                        break;
                    }
                    Err(_) => {
                        warn!("Connection to DB lost, reconnecting!");
                        self.client = None;
                    }
                },
            }
        }

        Ok("Connection is OK!".to_string())
    }

    async fn start_panel(&mut self, tokens: Vec<&str>) -> anyhow::Result<String> {
        let dmc = tokens[1].to_string();
        let boards = tokens[2].parse::<u8>()?;
        let mode = self.get_mode();

        self.logs.clear();
        debug!("Starting new board: {dmc}");

        // A) Is it a golden sample

        if self.golden_samples.contains(&dmc) {
            self.push_mode(dmc);
            return Ok(String::from("GS"));
        }

        // B) traceability is disabled
        if mode != AppMode::Enabled {
            warn!("Mode is set to {mode:?}");
            self.push_mode(dmc);
            return Ok(String::from("OK: MES is disabled!"));
        }

        // Connect to the DB:
        self.connect().await?;
        let client = self.client.as_mut().unwrap();

        // QUERY #1:

        let qtext = format!(
            "SELECT COUNT(*) FROM [dbo].[SMT_Test] WHERE [Serial_NMBR] = '{}'",
            dmc
        );

        let query = Query::new(qtext);
        let result = query.query(client).await?;

        let tested_total;
        if let Some(row) = result.into_row().await? {
            if let Some(x) = row.get::<i32, usize>(0) {
                tested_total = x;
                if tested_total < LIMIT {
                    self.push_mode(dmc);
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
        let result = query.query(client).await?;

        if let Some(row) = result.into_row().await? {
            if let Some(x) = row.get::<i32, usize>(0) {
                if x >= LIMIT {
                    Ok(format!("NK: {x} ({tested_total})"))
                } else {
                    self.push_mode(dmc);
                    Ok(format!("OK: {x} ({tested_total})"))
                }
            } else {
                bail!("Q#2 Parsing error.");
            }
        } else {
            self.push_mode(dmc);
            Ok(format!("OK: 0 ({tested_total})")) // Q#2 will return NONE, if the MB has no 'failed' results at all.
        }
    }

    async fn end_panel(&mut self) -> anyhow::Result<String> {
        let logs = self.extract_logs();
        let mode = self.get_mode();
        let mut note = String::new();

        if logs.is_empty() {
            error!("Log buffer is empty!");
            bail!("Log buffer is empty!");
        }

        if mode == AppMode::OffLine {
            warn!("Mode is set to {mode:?}");
            return Ok(String::from("OK: Off-line mode"));
        } else if mode == AppMode::Override {
            note = format!("Tested by: {}. ", self.user.lock().unwrap().as_str());
        }

        let mut ict_logs = Vec::new();
        let mut t_max_u64: u64 = 0;
        for log in logs {
            debug!("Parsing log: {log}");
            if let Ok(l) = ICT_log_file::LogFile::load(&PathBuf::from(&log)) {
                if l.is_ok() {
                    t_max_u64 = t_max_u64.max(l.get_time_end());
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

        debug!("T_max: {}", t_max_u64);

        if ict_logs.is_empty() {
            error!("ICT log buffer is empty!");
            bail!("ICT log buffer is empty!");
        }

        let dmc = ict_logs[0].get_main_DMC();

        if !self.check_mode(dmc) {
            error!("Error processing panel!");
            bail!("Error processing panel!");
        }

        // Connect to the DB:
        self.connect().await?;
        let client = self.client.as_mut().unwrap();

        let station = self.config.get_station_name().to_owned();

        // Upload new results
        let mut qtext = String::from(
            "INSERT INTO [dbo].[SMT_Test] 
            ([Serial_NMBR], [Station], [Result], [Date_Time], [Log_File_Name], [SW_Version], [Notes])
            VALUES",
        );

        let t_max = ICT_log_file::u64_to_time(t_max_u64);
        for log in ict_logs {
            let mut final_note = note.clone();
            if log.get_status() != 0 {
                let failed_tests = log.get_failed_tests().join(", ");
                final_note += &format!("Failed: {}", failed_tests);
            }
            final_note.truncate(200);

            let log_path = format!("{}", log.get_source().to_string_lossy());
            let striped_log_path = if &log_path[1..2] == ":" {
                &log_path[2..]
            } else {
                &log_path
            };

            qtext += &format!(
                "('{}', '{}', '{}', '{}', '{}', '{}', '{}'),",
                log.get_DMC(),
                station,
                if log.get_status() == 0 {
                    "Passed"
                } else {
                    "Failed"
                },
                t_max,
                striped_log_path,
                log.get_SW_ver(),
                final_note
            );
        }
        qtext.pop(); // removes last ','

        debug!("Upload: {}", qtext);
        let query = Query::new(qtext);
        query.execute(client).await?;

        debug!("Upload OK");

        Ok(String::from("OK"))
    }

    async fn ping_other_servers(&mut self, tokens: Vec<&str>) -> anyhow::Result<String> {
        for station in self.config.get_other_stations() {
            info!("PING to station: {station}");
            let mut stream = TcpStream::connect(station).await?;
            stream
                .write_all(format!("TEST|{}", tokens[1]).as_bytes())
                .await?;
            stream.shutdown().await?;
        }

        Ok("PING succesfull".to_string())
    }

    pub async fn update_golden_samples(&mut self) -> anyhow::Result<String> {
        // Connect to the DB:
        self.connect().await?;
        let client = self.client.as_mut().unwrap();

        // Query golden samples
        if let Ok(mut result) = client
            .query("SELECT [Serial_NMBR] FROM [dbo].[SMT_ICT_GS]", &[])
            .await
        {
            self.golden_samples.clear();
            while let Some(row) = result.next().await {
                let row = row.unwrap();
                match row {
                    tiberius::QueryItem::Row(x) => {
                        self.golden_samples
                            .push(x.get::<&str, usize>(0).unwrap().to_owned());
                    }
                    tiberius::QueryItem::Metadata(_) => (),
                }
            }
        } else {
            bail!("Found no golden samples!");
        }

        ICT_config::export_gs_list(&self.golden_samples)?;

        Ok("OK: Golden samples updated succesfully".to_string())
    }

    async fn add_golden_sample(&mut self, serial: &str, user: &str) -> anyhow::Result<String> {
        // Connect to the DB:
        self.connect().await?;
        let client = self.client.as_mut().unwrap();

        let product = match ICT_config::get_product_for_serial(ICT_config::PRODUCT_LIST, serial) {
            Some(prod) => prod.get_name().to_string(),
            None => String::new(),
        };

        let date = chrono::Utc::now();

        let mut query = Query::new(
            "INSERT INTO [dbo].[SMT_ICT_GS]
            ([Serial_NMBR], [Product], [Date_Time], [Notes])
            VALUES (@P1, @P2, @P3, @P4);",
        );
        query.bind(serial);
        query.bind(product);
        query.bind(date);
        query.bind(user);

        match query.execute(client).await {
            Ok(_) => {
                self.golden_samples.push(serial.to_string());
                ICT_config::export_gs_list(&self.golden_samples)?;
                Ok("Upload succesfull".to_string())
            }
            Err(e) => bail!("{e}"),
        }
    }

    fn push_mode(&mut self, dmc: String) {
        self.last_mode = *self.mode.lock().unwrap();
        self.last_dmc = dmc;
    }

    fn check_mode(&self, dmc: &str) -> bool {
        *self.mode.lock().unwrap() == self.last_mode && self.last_dmc == dmc
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

async fn connect(
    tib_config: tiberius::Config,
) -> anyhow::Result<tiberius::Client<tokio_util::compat::Compat<TcpStream>>> {
    let tcp = TcpStream::connect(tib_config.get_addr()).await?;
    tcp.set_nodelay(true)?;
    let client = Client::connect(tib_config, tcp.compat_write()).await?;

    Ok(client)
}
