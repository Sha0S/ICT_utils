#![allow(non_snake_case)]

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::bail;
use log::{debug, error};

pub const CONFIG: &str = "config.ini";
pub const PRODUCT_LIST: &str = "products";
pub const GOLDEN_LIST: &str = "golden_samples";

pub const LAST_DATE_FILE: &str = "last_date";
pub const LAST_DATE_FILE_BAK: &str = "last_date.bak";
pub const DMC_MIN_LENGTH: usize = 15;

/* Product
'!' starts a comment
Product Name | Boards on panel | Log file directory | DMC patterns
*/

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum TesterType {
    #[default]
    Ict,
    FctKaizen,
    FctDcdc,
}

#[derive(Debug, Default, Clone)]
pub struct Product {
    name: String,
    patterns: Vec<String>,
    boards_on_panel: u8,
    log_dir: PathBuf,
    modifiers: Vec<String>,
    tester_type: TesterType,
}

pub fn load_product_list<P: AsRef<Path> + std::fmt::Debug>(
    path: P,
    load_all: bool,
) -> Vec<Product> {
    let mut list = Vec::new();

    for line in filter_file(path) {
        let parts: Vec<&str> = line.split('|').map(|f| f.trim()).collect();
        if parts.len() < 3 {
            continue;
        }

        let boards_on_panel = parts[1].parse::<u8>().unwrap_or(1);
        let log_dir = PathBuf::from(parts[2]);

        let mut patterns = Vec::new();
        let mut modifiers = Vec::new();

        for token in parts.iter().skip(3) {
            if token.starts_with('#') {
                modifiers.push(token.to_string());
            } else {
                patterns.push(token.to_string())
            }
        }

        let tester_type = if modifiers.iter().any(|f| f == "#fct") {
            TesterType::FctKaizen
        } else if modifiers.iter().any(|f| f == "#fct_dcdc") {
            TesterType::FctDcdc
        } else {
            TesterType::Ict
        };

        if log_dir.try_exists().is_ok_and(|x| x) || load_all {
            list.push(Product {
                name: parts[0].to_owned(),
                patterns,
                boards_on_panel,
                log_dir,
                modifiers,
                tester_type,
            });
        }
    }

    list
}

pub fn get_product_for_serial<P: AsRef<Path> + std::fmt::Debug>(
    path: P,
    serial: &str,
) -> Option<Product> {
    if serial.len() < 20 {
        return None;
    }

    let list = load_product_list(path, true);

    list.into_iter()
        .find(|product| product.check_serial(serial))
}

impl Product {
    pub fn unknown() -> Self {
        Self {
            name: "Unknown product".to_string(),
            boards_on_panel: 1,
            ..Default::default()
        }
    }

    pub fn check_serial(&self, serial: &str) -> bool {
        if serial.len() < DMC_MIN_LENGTH {
            return false;
        }

        // Support for DCDC DMCs
        // Format: !YYDDDxxxx!********* (last 9 chars are version ID)
        // version ID starts at char #11
        if serial.starts_with('!') {
            for pattern in &self.patterns {
                if serial[11..].starts_with(pattern) {
                    return true;
                }
            }

            return false;
        }

        // VLLDDDxxxxxxx*
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

    pub fn get_tester_type(&self) -> TesterType {
        self.tester_type
    }

    pub fn get_pos_from_logname(&self, log_file_name: &str) -> Option<u8> {
        let filename = log_file_name.split(&['/', '\\']).last()?;
        let pos = filename.split_once('-')?;

        if let Ok(p) = pos.0.parse::<u8>() {
            if self.modifiers.iter().any(|f| f == "#inv") {
                Some(self.boards_on_panel - p)
            } else {
                Some(p - 1)
            }
        } else {
            None
        }
    }

    pub fn filter(&self, p: &Path) -> bool {
        for modifier in &self.modifiers {
            if let Some(sufix) = modifier.strip_prefix("#filter:") {
                if let Some(filename) = p.file_stem() {
                    if filename.to_string_lossy().contains(sufix) {
                        return true;
                    } else {
                        return false;
                    }
                }
            }
        }


        true
    }
}

/* Config */

#[derive(Debug, Copy, Clone)]
pub struct OverlayPos {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Default)]
pub struct Config {
    server: String,
    database: String,
    password: String,
    username: String,

    log_reader: String,
    MES_server: String,
    station_name: String,

    AOI_dir: String,
    AOI_chunks: usize,
    AOI_tminus: u64,
    AOI_deltat: u64,

    overlay_pos: Option<OverlayPos>,
}

impl Config {
    pub fn read<P: AsRef<Path>>(path: P) -> anyhow::Result<Config> {
        let path = path.as_ref();
        let mut c = Config::default();

        if let Ok(config) = ini::Ini::load_from_file(path) {
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

                if let Some(station) = app.get("STATION") {
                    c.station_name = station.to_owned();
                }
            }

            if let Some(app) = config.section(Some("AOI")) {
                if let Some(dir) = app.get("DIR") {
                    c.AOI_dir = dir.to_owned();
                }

                if let Some(chunks) = app.get("CHUNKS") {
                    c.AOI_chunks = chunks.parse().unwrap_or(10);
                }

                if let Some(chunks) = app.get("TMINUS") {
                    c.AOI_tminus = chunks.parse().unwrap_or(300);
                }

                if let Some(chunks) = app.get("DELTA_T") {
                    c.AOI_deltat = chunks.parse().unwrap_or(600);
                }

                if c.AOI_dir.is_empty() {
                    return Err(anyhow::Error::msg(
                        "ER: Missing [AOI] - [DIR] field from configuration file!",
                    ));
                }
            }

            if let Some(app) = config.section(Some("OVERLAY")) {
                let mut x: f32 = -1.0;
                let mut y: f32 = -1.0;
                let mut w: f32 = -1.0;
                let mut h: f32 = -1.0;

                if let Some(viewer) = app.get("X") {
                    x = viewer.parse().unwrap_or(-2.00);
                }

                if let Some(viewer) = app.get("Y") {
                    y = viewer.parse().unwrap_or(-2.00);
                }

                if let Some(viewer) = app.get("W") {
                    w = viewer.parse().unwrap_or(-2.00);
                }

                if let Some(viewer) = app.get("H") {
                    h = viewer.parse().unwrap_or(-2.00);
                }

                if x >= 0.0 && y >= 0.0 && w > 0.0 && h > 0.0 {
                    c.overlay_pos = Some(OverlayPos { x, y, w, h });
                    debug!("Overlay position is: {x}x{y} {w}x{h}");
                } else {
                    error!("Could not create overlay_pos: {x}x{y} {w}x{h}");
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

    pub fn get_station_name(&self) -> &str {
        &self.station_name
    }

    pub fn get_aoi_dir(&self) -> &str {
        &self.AOI_dir
    }

    pub fn get_aoi_chunks(&self) -> usize {
        self.AOI_chunks
    }

    pub fn get_aoi_deltat(&self) -> u64 {
        self.AOI_deltat
    }

    pub fn get_aoi_tminus(&self) -> u64 {
        self.AOI_tminus
    }

    pub fn get_overlay_pos(&self) -> Option<OverlayPos> {
        self.overlay_pos
    }
}

/* Utillity */

fn filter_file<P: AsRef<Path> + std::fmt::Debug>(path: P) -> Vec<String> {
    let mut list = Vec::new();

    if let Ok(fileb) = fs::read_to_string(&path) {
        for full_line in fileb.lines() {
            if !full_line.is_empty() && !full_line.starts_with('!') {
                let line = &full_line[0..full_line.find('!').unwrap_or(full_line.len())];
                list.push(line.trim().to_string());
            }
        }
    } else {
        log::error!("filter_file: source ({:?}) not readable!", path);
    }

    list
}

// DMCs can start with '!' sign (DCDC), so we can't use comments here
// but the GS list is auto generated from a SQL DB, so it shouldn't have any to begin with
pub fn load_gs_list<P: AsRef<Path> + std::fmt::Debug>(path: P) -> Vec<String> {
    let mut list = Vec::new();

    if let Ok(fileb) = fs::read_to_string(&path) {
        for full_line in fileb.lines() {
            if !full_line.is_empty() {
                list.push(full_line.trim().to_string());
            }
        }
    } else {
        log::error!("load_gs_list: source ({:?}) not readable!", path);
    }

    list
}

pub fn export_gs_list(gs: &Vec<String>) -> anyhow::Result<()> {
    let mut file = match fs::File::create(GOLDEN_LIST) {
        Err(e) => {
            bail!("{e}");
        }
        Ok(file) => file,
    };

    for line in gs {
        writeln!(file, "{}", line)?;
    }

    Ok(())
}

pub fn load_gs_list_for_product<P: AsRef<Path> + std::fmt::Debug>(
    path: P,
    product: &Product,
) -> Vec<String> {
    let all_gs = load_gs_list(path);
    let mut ret = Vec::new();

    for gs in all_gs {
        if product.check_serial(&gs) {
            ret.push(gs);
        }
    }

    ret
}

pub fn increment_sn(start: &str, boards: u8) -> Vec<String> {
    log::debug!("increment_sn: {start} number_of_boards: {boards}");
    let mut ret = Vec::with_capacity(boards as usize);
    ret.push(start.to_string());
    if boards < 2 || start.len() < DMC_MIN_LENGTH {
        return ret;
    }

    // Support for DCDC DMCs
    // Format: !YYDDDxxxx!********* (last 9 chars are version ID)
    // it only uses 4 digits, not 7! Start pos is the same.
    if start.starts_with('!') {
        if let Ok(sn) = &start[6..10].parse::<u32>() {
            for i in 1..boards {
                let nsn = sn + i as u32;
                let mut next_sn = start.to_string();
                next_sn.replace_range(6..10, &format!("{:04}", nsn));
                ret.push(next_sn);
            }
        } else {
            log::error!("increment_sn: DCDC DMC parsing error ({start})");
        }

        return ret;
    }

    // VLLDDDxxxxxxx*
    // x is 7 digits -> u32
    if let Ok(sn) = &start[6..13].parse::<u32>() {
        for i in 1..boards {
            let nsn = sn + i as u32;
            let mut next_sn = start.to_string();
            next_sn.replace_range(6..13, &format!("{:07}", nsn));
            ret.push(next_sn);
        }
    } else {
        log::error!("increment_sn: DMC parsing error ({start})");
    }

    ret
}

pub fn generate_serials(serial: &str, position: u8, max_pos: u8) -> Vec<String> {
    log::debug!("generate_serials: {serial}, pos: {position}, max: {max_pos}");
    let mut ret = Vec::with_capacity(max_pos as usize);

    if max_pos < 2 || serial.len() < DMC_MIN_LENGTH {
        ret.push(serial.to_string());
        return ret;
    }

    // Support for DCDC DMCs
    // Format: !YYDDDxxxx!********* (last 9 chars are version ID)
    // it only uses 4 digits, not 7! Start pos is the same.
    if serial.starts_with('!') {
        if let Ok(start) = serial[6..10].parse::<u32>() {
            let sn = start - position as u32;
            for i in sn..sn + max_pos as u32 {
                let mut s = serial.to_string();
                s.replace_range(6..10, &format!("{:04}", i));
                ret.push(s);
            }
        } else {
            ret.push(serial.to_string());
            log::error!("generate_serials: DCDC DMC parsing error ({serial})");
        }

        return ret;
    }

    // VLLDDDxxxxxxx*
    // x is 7 digits -> u32
    if let Ok(start) = serial[6..13].parse::<u32>() {
        let sn = start - position as u32;
        for i in sn..sn + max_pos as u32 {
            let mut s = serial.to_string();
            s.replace_range(6..13, &format!("{:07}", i));
            ret.push(s);
        }
    } else {
        ret.push(serial.to_string());
        log::error!("generate_serials: DMC parsing error ({serial})");
    }

    ret
}

pub fn generate_main_serial(serial: &str, position: u8) -> anyhow::Result<String> {
    log::debug!("generate_main_serials: {serial}, pos: {position}");

    if position < 2 {
        return Ok(serial.to_string());
    }

    if serial.len() < DMC_MIN_LENGTH {
        bail!("DMC is too short! {serial}");
    }

    // VLLDDDxxxxxxx*
    // x is 7 digits -> u32
    if let Ok(start) = serial[6..13].parse::<u32>() {
        let sn = start - position as u32 + 1;

        let mut s = serial.to_string();
        s.replace_range(6..13, &format!("{:07}", sn));
        return Ok(s);
    } else {
        log::error!("generate_main_serial: DMC parsing error ({serial})");
        bail!("generate_main_serial: DMC parsing error ({serial})");
    }
}

// Interop

pub fn query(serial: String) -> std::result::Result<std::process::Child, std::io::Error> {
    std::process::Command::new("query.exe").arg(serial).spawn()
}

// last_date subrutines
// LAST_TIME_FILE_BAK is a backup of the previous value, in case of a file corruption

pub fn get_last_date() -> anyhow::Result<chrono::DateTime<chrono::Local>> {
    // 1 - try to read LAST_TIME_FILE
    if let Ok(line) = fs::read_to_string(LAST_DATE_FILE) {
        if let Ok(last_date) = chrono::NaiveDateTime::parse_from_str(&line, "%Y-%m-%d %H:%M:%S") {
            match last_date.and_local_timezone(chrono::Local) {
                chrono::offset::LocalResult::Single(t) => return Ok(t),
                chrono::offset::LocalResult::Ambiguous(earliest, _) => return Ok(earliest),
                chrono::offset::LocalResult::None => {
                    error!("Error converting last_date! LocalResult::None!");
                }
            };
        } else {
            error!("Parsing last_date failed: {}", line);
        }
    } else {
        error!("Could not read {}!", LAST_DATE_FILE);
    }

    error!("Reading primary time source failed! Trying to read the backup.");

    // 2 - try to read LAST_DATE_FILE_BAK
    if let Ok(line) = fs::read_to_string(LAST_DATE_FILE_BAK) {
        if let Ok(last_date) = chrono::NaiveDateTime::parse_from_str(&line, "%Y-%m-%d %H:%M:%S") {
            match last_date.and_local_timezone(chrono::Local) {
                chrono::offset::LocalResult::Single(t) => return Ok(t),
                chrono::offset::LocalResult::Ambiguous(earliest, _) => return Ok(earliest),
                chrono::offset::LocalResult::None => {
                    error!("Error converting last_date! LocalResult::None!");
                }
            };
        } else {
            error!("Parsing last_date failed: {}", line);
        }
    } else {
        error!("Could not read {}!", LAST_DATE_FILE);
    }

    bail!("Could not read last_time!");
}

pub fn set_last_date(new: chrono::DateTime<chrono::Local>) -> anyhow::Result<()> {
    fs::copy(LAST_DATE_FILE, LAST_DATE_FILE_BAK)?;
    let output_string = new.format("%Y-%m-%d %H:%M:%S").to_string();
    fs::write(LAST_DATE_FILE, output_string)?;

    Ok(())
}
