#![allow(non_snake_case)]

use anyhow::{Context, Result};
use tiberius::{Client, Query};
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

pub struct SQL {
    db: String,
    config: tiberius::Config,
    client: Option<Client<Compat<TcpStream>>>,
}

impl SQL {
    pub fn new(ip: &str, db: &str, user: &str, pass: &str) -> Result<Self> {
        let mut config = tiberius::Config::new();
        config.host(ip);
        config.authentication(tiberius::AuthMethod::sql_server(user, pass));
        config.trust_cert();

        Ok(SQL { db: db.to_string(), config, client: None})
    }

    pub async fn check_connection(&mut self) -> bool {
        if let Some(client) = &mut self.client {
            client.execute("SELECT 1", &[]).await.is_ok()
        } else {
            false
        }
    }

    async fn connect(&mut self) -> Result<()> {
        let tcp = TcpStream::connect(self.config.get_addr()).await?;
        tcp.set_nodelay(true)?;
        let client = Client::connect(self.config.clone(), tcp.compat_write()).await?;

        self.client = Some(client);

        Ok(())
    }

    pub async fn create_connection(&mut self) -> Result<()> {

        let mut res = self.connect().await;

        let mut tries = 0;
        while res.is_err() && tries < 3 {
            res = self.connect().await;
            tries += 1;
        }

        res?;
        
        let mut client = self.client.as_mut().unwrap();

        // USE [DB]
        let qtext = format!("USE [{}]", self.db);
        let query = Query::new(qtext);
        query.execute(&mut client).await?;

        Ok(())
    }

    pub fn client(&mut self) -> Result<&mut Client<Compat<TcpStream>>> {
        self.client.as_mut().context("Client is not initialized!")
    }
}
