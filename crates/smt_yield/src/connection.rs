use anyhow::bail;
use log::{debug, error};
use tiberius::Client;
use tiberius::Query;
use tokio::net::TcpStream;
use tokio_util::compat::Compat;
use tokio_util::compat::TokioAsyncWriteCompatExt;

use anyhow::Result;

macro_rules! error_and_bail {
    ($($arg:tt)+) => {
        error!($($arg)+);
        bail!($($arg)+);
    };
}

#[derive(Default, Debug)]
pub struct Config {
    server: String,
    database: String,
    password: String,
    username: String,
}

impl Config {
    pub fn load() -> anyhow::Result<Config> {
        let mut c = Config::default();

        if let Ok(config) = ini::Ini::load_from_file("config.ini") {
            if let Some(jvserver) = config.section(Some("JVSERVER")) {
                // mandatory fields:
                if let Some(server) = jvserver.get("SERVER") {
                    c.server = server.to_owned();
                }
                if let Some(password) = jvserver.get("PASSWORD") {
                    c.password = password.to_owned();
                }
                if let Some(username) = jvserver.get("USERNAME") {
                    c.username = username.to_owned();
                }
                if let Some(database) = jvserver.get("DATABASE") {
                    c.database = database.to_owned();
                }

                if c.server.is_empty()
                    || c.password.is_empty()
                    || c.username.is_empty()
                    || c.database.is_empty()
                {
                    return Err(anyhow::Error::msg(
                        "ER: Missing [JVSERVER] fields from configuration file!",
                    ));
                }
            } else {
                return Err(anyhow::Error::msg("ER: Could not find [JVSERVER] field!"));
            }
        } else {
            return Err(anyhow::Error::msg(
                "ER: Could not read configuration file! [.\\config.ini]",
            ));
        }

        Ok(c)
    }
}

async fn connect(tib_config: tiberius::Config) -> anyhow::Result<Client<Compat<TcpStream>>> {
    let tcp = TcpStream::connect(tib_config.get_addr()).await?;
    tcp.set_nodelay(true)?;
    let client = Client::connect(tib_config, tcp.compat_write()).await?;

    Ok(client)
}

pub async fn create_connection(config: &Config) -> Result<Client<Compat<TcpStream>>> {
    // Tiberius configuartion:

    let sql_server = config.server.to_owned();
    let sql_user = config.username.to_owned();
    let sql_pass = config.password.to_owned();

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
        error_and_bail!("Connection to DB failed!");
    }
    let mut client = client_tmp?;

    // USE [DB]
    let qtext = format!("USE [{}]", config.database);
    debug!("USE DB: {}", qtext);
    let query = Query::new(qtext);
    query.execute(&mut client).await?;

    Ok(client)
}
