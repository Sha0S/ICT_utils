use std::path::Path;
use log::debug;
use serde::Deserialize;


#[derive(Deserialize, Debug)]
pub struct Config {
    // SQL server config
    pub sql_ip: String,
    pub sql_db: String,
    pub sql_pass: String,
    pub sql_user: String,

    pub serial_port: String,

    // Products
    pub product_list: Vec<Product>
}

#[derive(Deserialize, Debug, Clone)]
pub struct Product {
    pub name: String,
    pub serial_ids: Vec<String>,
    pub uses_fct: bool,
    pub boards_per_frame: u8,
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let file = std::fs::read_to_string(path)?;
        let ret = serde_json::from_str(&file)?;
        debug!("Config: {:?}", ret);
        Ok(ret)
    }

    pub fn get_product(&self, serial: &str) -> Option<Product> {

        for product in &self.product_list {
            if product.check_serial(serial) {
                return Some(product.clone());
            }
        }

        None
    }
}

impl Product {
    pub fn check_serial(&self, serial: &str) -> bool {
        if serial.len() < 15 {
            return false;
        }

        // VLLDDDxxxxxxx*
        for pattern in &self.serial_ids {
            if serial[13..].starts_with(pattern) {
                return true;
            }
        }

        false
    }
}