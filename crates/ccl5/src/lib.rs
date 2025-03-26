#![allow(non_snake_case)]


use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, Result};
use chrono::NaiveDateTime;
use log::{debug, error};

macro_rules! error_and_bail {
    ($($arg:tt)+) => {
        error!($($arg)+);
        bail!($($arg)+);
    };
}

/*
SerialNum= V102508400582DB828853020
 -> ShortDMC: V102508400582D
 -> Barcode, Lead_DMC (?)
 -> Board: B828853 + SIDE (_TOP, _BOT)
Side=      Top
 -> Side (UPPERCASE! TOP or BOT)
LoginUser= a
 -> Operator
DateTime=  03/26/25 15:14:58
 -> FileDate + RowUpdated
BoardsPerPanel= 2
 -> Palette_size
TestStatus= PASS
 -> Result (PASS or FAIL)
 
+ FileName
*/

#[derive(Debug, Clone)]
pub struct Board {
    log: PathBuf,

    serial: String,
    side: String,
    boards_on_panel: u8,

    user: String,
    date_time: NaiveDateTime,
    result: String
}

impl Board {
    pub fn load<P: AsRef<Path> + Debug>(path: P) -> anyhow::Result<Board> {
        let log = PathBuf::from(P);
        let mut serial = String::new();
        let mut side = String::new();
        let mut boards_on_panel = 0;
        let mut user = String::new();
        let mut date_time = None;
        let mut result = String::new();

        let mut f = fs::read_to_string(&log)?;
        for line in f.lines() {
            if let Some((key, value_raw)) = line.split_once('=') {
                let value = value_raw.trim();
                match key {
                    "SerialNum" => {
                        serial = value.to_string();
                    }
                    "Side" => {
                        side = value.to_uppercase();
                    }
                    "LoginUser" => {
                        user = value.to_string();
                    }
                    "DateTime" => { // 03/26/25 15:14:58
                        if let Ok(dt) = NaiveDateTime::parse_from_str(value, "%m/%d/%y %H:%M:%S") {
                            date_time = Some(dt);
                        } else {
                            error_and_bail!("Error parsing DateTime field in log {:?}", log);
                        }
                    }
                    "BoardsPerPanel" => {
                        if let Ok(i) = vaule.parse::<u8>() {
                            boards_on_panel = i;
                        } else {
                            error_and_bail!("Error parsing BoardsPerPanel field in log {:?}", log);
                        }
                    }
                    "TestStatus" => {
                        result = value.to_string();
                    }

                    _ => {}
                }

            }
        }

        if serial.is_empty() || side.is_empty() || result.is_empty() {
            error_and_bail!("Found no SerialNum, Side or TestStatus  field in log {:?}", log);
        }


        if date_time.is_none() {
            error_and_bail!("Found no DateTime field in log {:?}", log);
        }
        let date_time = date_time.unwrap();

        Ok(Board{
            log,
            serial,
            side,
            boards_on_panel,
            user,
            date_time,
            result
        })
    }
}