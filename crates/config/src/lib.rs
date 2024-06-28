#![allow(non_snake_case)]

use std::{fs, path::PathBuf};

/* Product
'!' starts a comment
Product Name | Boards on panel | Log file directory | DMC patterns
*/

#[derive(Debug)]
pub struct Product {
    name: String,
    patterns: Vec<String>,
    boards_on_panel: u8,
    log_dir: PathBuf,
}

pub fn load_product_list(src: &str, load_all: bool) -> Vec<Product> {
    let mut list = Vec::new();

    if let Ok(fileb) = fs::read_to_string(src) {
        for full_line in fileb.lines() {
            if !full_line.is_empty() && !full_line.starts_with('!') {
                let line = &full_line[0..full_line.find('!').unwrap_or(full_line.len())];

                let parts: Vec<&str> = line.split('|').map(|f| f.trim()).collect();
                if parts.len() < 3 {
                    continue;
                }

                let boards_on_panel = parts[1].parse::<u8>().unwrap_or(1);
                let log_dir = PathBuf::from(parts[2]);

                if log_dir.try_exists().is_ok_and(|x| x) || load_all {
                    list.push(Product {
                        name: parts[0].to_owned(),
                        patterns: parts.iter().skip(3).map(|f| f.to_string()).collect(),
                        boards_on_panel,
                        log_dir,
                    });
                }
            }
        }
    } else {
        println!("ERR: source ({src}) not readable!");
    }

    list
}

impl Product {
    pub fn check_serial(&self, serial: &str) -> bool {
        for pattern in &self.patterns {
            if serial[13..].starts_with(pattern) {
                return true;
            }
        }

        false
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn get_bop(&self) -> u8 {
        self.boards_on_panel
    }

    pub fn get_log_dir(&self) -> &PathBuf {
        &self.log_dir
    }
}

/* Config */

#[derive(Default)]
pub struct Config {
    server: String,
    database: String,
    password: String,
    username: String,

    log_reader: String,
    MES_server: String,
}

impl Config {
    pub fn read(path: PathBuf) -> anyhow::Result<Config> {
        let mut c = Config::default();

        if let Ok(config) = ini::Ini::load_from_file(path.clone()) {
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
                        "ER: Missing fields from configuration file!",
                    ));
                }
            } else {
                return Err(anyhow::Error::msg("ER: Could not find [JVSERVER] field!"));
            }

            if let Some(app) = config.section(Some("APP")) {
                if let Some(viewer) = app.get("VIEWER") {
                    c.log_reader = viewer.to_owned();
                }

                if let Some(server) = app.get("MES_SERVER") {
                    c.MES_server = server.to_owned();
                }
            }
        } else {
            return Err(anyhow::Error::msg(format!(
                "ER: Could not read configuration file! [{}]",
                path.display()
            )));
        }

        Ok(c)
    }

    pub fn get_server(&self) -> &str {
        &self.server
    }

    pub fn get_database(&self) -> &str {
        &self.database
    }

    pub fn get_password(&self) -> &str {
        &self.password
    }

    pub fn get_username(&self) -> &str {
        &self.username
    }

    pub fn get_log_reader(&self) -> &str {
        &self.log_reader
    }

    pub fn get_MES_server(&self) -> &str {
        &self.MES_server
    }
}

/* Utillity */

pub fn load_gs_list(path: PathBuf) -> Vec<String> {
    let mut list = Vec::new();

    if let Ok(fileb) = fs::read_to_string(path) {
        list = fileb
            .lines()
            .filter(|f| !f.starts_with('!'))
            .map(|f| f.to_owned())
            .collect();
    }

    list
}

pub fn get_pos_from_logname(log_file_name: &str) -> u8 {
    let filename = log_file_name.split(&['/', '\\']).last().unwrap();
    let pos = filename.split_once('-').unwrap();
    pos.0.parse::<u8>().unwrap() - 1
}

pub fn increment_sn(start: &str, boards: u8) -> Vec<String> {
    // VLLDDDxxxxxxx*
    // x is 7 digits -> u32
    let mut ret = Vec::with_capacity(boards as usize);
    ret.push(start.to_string());

    let sn = &start[6..13].parse::<u32>().expect("ER: Parsing error");

    for i in 1..boards {
        let nsn = sn + i as u32;
        let mut next_sn = start.to_string();
        next_sn.replace_range(6..13, &format!("{:07}", nsn));
        ret.push(next_sn);
    }

    ret
}

pub fn generate_serials(serial: String, position: u8, max_pos: u8) -> Vec<String> {
    let mut ret = Vec::with_capacity(max_pos as usize);

    let sn = serial[6..13].parse::<u32>().expect("ER: Parsing error") - position as u32;
    for i in sn..sn + max_pos as u32 {
        let mut s = serial.clone();
        s.replace_range(6..13, &format!("{:07}", i));
        ret.push(s);
    }

    ret
}
