#![allow(dead_code)]
#![allow(non_snake_case)]

use std::collections::HashSet;
use std::ffi::OsString;
use std::io;
use std::ops::AddAssign;
use std::path::{Path, PathBuf};

use chrono::{Datelike, NaiveDateTime, Timelike};
use ICT_config::{get_product_for_serial, load_gs_list_for_product, Product};

mod keysight_log;

// Removes the index from the testname.
// For example: "17%c617" -> "c617"
fn strip_index(s: &str) -> &str {
    let mut chars = s.chars();

    let mut c = chars.next();
    while c.is_some() {
        if c.unwrap() == '%' {
            break;
        }
        c = chars.next();
    }

    chars.as_str()
}

// YYMMDDhhmmss => YY.MM.DD. hh:mm:ss
pub fn u64_to_string(mut x: u64) -> String {
    let YY = x / u64::pow(10, 10);
    x %= u64::pow(10, 10);

    let MM = x / u64::pow(10, 8);
    x %= u64::pow(10, 8);

    let DD = x / u64::pow(10, 6);
    x %= u64::pow(10, 6);

    let hh = x / u64::pow(10, 4);
    x %= u64::pow(10, 4);

    let mm = x / u64::pow(10, 2);
    x %= u64::pow(10, 2);

    format!(
        "{:02.0}.{:02.0}.{:02.0}. {:02.0}:{:02.0}:{:02.0}",
        YY, MM, DD, hh, mm, x
    )
}

pub fn u64_to_time(mut x: u64) -> chrono::NaiveDateTime {
    let year: u64 = x / u64::pow(10, 10) + 2000;
    x %= u64::pow(10, 10);

    let month = x / u64::pow(10, 8);
    x %= u64::pow(10, 8);

    let day: u64 = x / u64::pow(10, 6);
    x %= u64::pow(10, 6);

    let hour = x / u64::pow(10, 4);
    x %= u64::pow(10, 4);

    let min = x / u64::pow(10, 2);
    x %= u64::pow(10, 2);

    let date = chrono::NaiveDate::from_ymd_opt(year as i32, month as u32, day as u32).unwrap();
    let time = chrono::NaiveTime::from_hms_opt(hour as u32, min as u32, x as u32).unwrap();

    date.and_time(time)
}

fn time_to_u64<T: chrono::Datelike + Timelike>(t: T) -> u64 {
    (t.year() as u64 - 2000) * u64::pow(10, 10)
        + t.month() as u64 * u64::pow(10, 8)
        + t.day() as u64 * u64::pow(10, 6)
        + t.hour() as u64 * u64::pow(10, 4)
        + t.minute() as u64 * u64::pow(10, 2)
        + t.second() as u64
}

fn local_time_to_u64(t: chrono::DateTime<chrono::Local>) -> u64 {
    (t.year() as u64 - 2000) * u64::pow(10, 10)
        + t.month() as u64 * u64::pow(10, 8)
        + t.day() as u64 * u64::pow(10, 6)
        + t.hour() as u64 * u64::pow(10, 4)
        + t.minute() as u64 * u64::pow(10, 2)
        + t.second() as u64
}

#[derive(Clone, Copy, PartialEq)]
pub enum ExportMode {
    All,
    FailuresOnly,
    Manual,
}

pub struct ExportSettings {
    pub vertical: bool,
    pub only_failed_panels: bool,
    pub only_final_logs: bool,
    pub mode: ExportMode,
    pub list: String,
}

impl Default for ExportSettings {
    fn default() -> Self {
        Self {
            vertical: false,
            only_failed_panels: false,
            only_final_logs: false,
            mode: ExportMode::All,
            list: String::new(),
        }
    }
}

pub type TResult = (BResult, f32);
type TList = (String, TType);

// OK - NOK
#[derive(Default, Debug, Clone, Copy)]
pub struct Yield(pub u16, pub u16);
impl AddAssign for Yield {
    fn add_assign(&mut self, x: Self) {
        *self = Yield(self.0 + x.0, self.1 + x.1);
    }
}

// Returns Yield as a precentage (OK/(OK+NOK))*100
impl Yield {
    pub fn precentage(self) -> f32 {
        (self.0 as f32 * 100.0) / (self.0 as f32 + self.1 as f32)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TLimit {
    None,
    Lim2(f32, f32),      // UL - LL
    Lim3(f32, f32, f32), // Nom - UL - LL
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TType {
    Pin,
    Shorts,
    Jumper,
    Fuse,
    Resistor,
    Capacitor,
    Inductor,
    Diode,
    Zener,
    NFet,
    PFet,
    Npn,
    Pnp,
    Pot,
    Switch,
    Testjet,
    Digital,
    Measurement,
    Current,
    BoundaryS,
    Time,
    Frequency,
    Temperature,
    Precentage,
    Degrees,
    Unknown,
}

// conversion for FCT logs
impl From<&str> for TType {
    fn from(value: &str) -> Self {
        match value {
            "Ohm" => TType::Resistor,
            "V" | "Vrms" => TType::Measurement,
            "mA" | "A" => TType::Current,
            "Hz" | "HZ" | "kHZ" | "KHZ" => TType::Frequency,
            "%" => TType::Precentage,
            "°" => TType::Degrees,
            "°C" | "¢C" => TType::Temperature,
            _ => TType::Unknown,
        }
    }
}

impl From<keysight_log::AnalogTest> for TType {
    fn from(value: keysight_log::AnalogTest) -> Self {
        match value {
            keysight_log::AnalogTest::Cap => TType::Capacitor,
            keysight_log::AnalogTest::Diode => TType::Diode,
            keysight_log::AnalogTest::Fuse => TType::Fuse,
            keysight_log::AnalogTest::Inductor => TType::Inductor,
            keysight_log::AnalogTest::Jumper => TType::Jumper,
            keysight_log::AnalogTest::Measurement => TType::Measurement,
            keysight_log::AnalogTest::NFet => TType::NFet,
            keysight_log::AnalogTest::PFet => TType::PFet,
            keysight_log::AnalogTest::Npn => TType::Npn,
            keysight_log::AnalogTest::Pnp => TType::Pnp,
            keysight_log::AnalogTest::Pot => TType::Pot,
            keysight_log::AnalogTest::Res => TType::Resistor,
            keysight_log::AnalogTest::Switch => TType::Switch,
            keysight_log::AnalogTest::Zener => TType::Zener,
            keysight_log::AnalogTest::Error => TType::Unknown,
        }
    }
}

impl TType {
    fn print(&self) -> String {
        match self {
            TType::Pin => "Pin".to_string(),
            TType::Shorts => "Shorts".to_string(),
            TType::Jumper => "Jumper".to_string(),
            TType::Fuse => "Fuse".to_string(),
            TType::Resistor => "Resistor".to_string(),
            TType::Capacitor => "Capacitor".to_string(),
            TType::Inductor => "Inductor".to_string(),
            TType::Diode => "Diode".to_string(),
            TType::Zener => "Zener".to_string(),
            TType::Testjet => "Testjet".to_string(),
            TType::Digital => "Digital".to_string(),
            TType::Measurement => "Measurement".to_string(),
            TType::Current => "Current".to_string(),
            TType::BoundaryS => "Boundary Scan".to_string(),
            TType::Unknown => "Unknown".to_string(),
            TType::NFet => "N-FET".to_string(),
            TType::PFet => "P-FET".to_string(),
            TType::Npn => "NPN".to_string(),
            TType::Pnp => "PNP".to_string(),
            TType::Pot => "Pot".to_string(),
            TType::Switch => "Switch".to_string(),
            TType::Time => "Time".to_string(),
            TType::Frequency => "Frequency".to_string(),
            TType::Temperature => "Temperature".to_string(),
            TType::Precentage => "Precentage".to_string(),
            TType::Degrees => "Degrees".to_string(),
        }
    }

    pub fn unit(&self) -> String {
        match self {
            TType::Pin | TType::Shorts => "Result".to_string(),
            TType::Jumper | TType::Fuse | TType::Resistor => "Ω".to_string(),
            TType::Capacitor => "F".to_string(),
            TType::Inductor => "H".to_string(),
            TType::Diode | TType::Zener => "V".to_string(),
            TType::NFet | TType::PFet | TType::Npn | TType::Pnp => "V".to_string(),
            TType::Pot | TType::Switch => "Ω".to_string(),
            TType::Testjet => "Result".to_string(),
            TType::Digital => "Result".to_string(),
            TType::Measurement => "V".to_string(),
            TType::Current => "A".to_string(),
            TType::BoundaryS => "Result".to_string(),
            TType::Unknown => "Result".to_string(),
            TType::Time => "s".to_string(),
            TType::Frequency => "Hz".to_string(),
            TType::Temperature => "°C".to_string(),
            TType::Precentage => "%".to_string(),
            TType::Degrees => "°".to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BResult {
    Pass,
    Fail,
    Unknown,
}

impl From<BResult> for bool {
    fn from(val: BResult) -> Self {
        matches!(val, BResult::Pass)
    }
}

impl From<bool> for BResult {
    fn from(value: bool) -> Self {
        if value {
            return BResult::Pass;
        }

        BResult::Fail
    }
}

impl From<i32> for BResult {
    fn from(value: i32) -> Self {
        if value == 0 {
            BResult::Pass
        } else {
            BResult::Fail
        }
    }
}

impl From<&str> for BResult {
    fn from(value: &str) -> Self {
        if matches!(value, "0" | "00") {
            return BResult::Pass;
        }

        BResult::Fail
    }
}

pub const DARK_GOLD: egui::Color32 = egui::Color32::from_rgb(235, 195, 0);

impl BResult {
    pub fn print(&self) -> String {
        match self {
            BResult::Pass => String::from("Pass"),
            BResult::Fail => String::from("Fail"),
            BResult::Unknown => String::from("NA"),
        }
    }

    pub fn into_color(self) -> egui::Color32 {
        match self {
            BResult::Pass => egui::Color32::GREEN,
            BResult::Fail => egui::Color32::RED,
            BResult::Unknown => egui::Color32::YELLOW,
        }
    }

    pub fn into_dark_color(self) -> egui::Color32 {
        match self {
            BResult::Pass => egui::Color32::DARK_GREEN,
            BResult::Fail => egui::Color32::RED,
            BResult::Unknown => egui::Color32::BLACK,
        }
    }
}

pub struct FailureList {
    pub test_id: usize,
    pub name: String,
    pub total: usize,
    pub failed: Vec<(String, u64)>,
    pub by_index: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct Test {
    name: String,
    ttype: TType,

    result: TResult,
    limits: TLimit,
}

impl Test {
    fn clear(&mut self) {
        self.name = String::new();
        self.ttype = TType::Unknown;
        self.result = (BResult::Unknown, 0.0);
        self.limits = TLimit::None;
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn get_ttype(&self) -> TType {
        self.ttype
    }

    pub fn get_result(&self) -> TResult {
        self.result
    }

    pub fn get_limits(&self) -> TLimit {
        self.limits
    }
}

#[derive(Debug)]
pub struct LogFile {
    source: OsString,
    DMC: String,
    DMC_mb: String,
    product_id: String,
    index: usize,

    result: bool,
    status: i32,
    status_str: String,

    time_start: u64,
    time_end: u64,

    tests: Vec<Test>,
    report: String,
    SW_version: String,
}

impl LogFile {
    pub fn load(p: &Path) -> io::Result<Self> {
        if p.extension().is_some_and(|f| f == "csv") {
            LogFile::load_FCT(p)
        } else {
            LogFile::load_ICT(p)
        }
    }

    pub fn load_FCT(p: &Path) -> io::Result<Self> {
        println!("INFO: Loading FCT file {}", p.display());
        let source = p.as_os_str().to_owned();

        let file_ANSI = std::fs::read(p)?;
        let decoded = encoding_rs::WINDOWS_1252.decode(&file_ANSI);

        if decoded.2 {
            println!("ERROR: Conversion had errors");
        }

        let lines = decoded.0.lines();

        let mut DMC = None;
        //let mut DMC_mb = None;
        //let mut product_id = None;
        //let mut SW_version = None;
        let mut result = None;
        let mut status = None;

        let mut time_start = None;
        let mut time_start_u64: u64 = 0;
        let mut testing_time: u64 = 0;

        let mut tests = Vec::new();

        for line in lines {
            let tokens: Vec<&str> = line.split(';').collect();
            if tokens.len() < 2 {
                //println!("ERROR: To few tokens! ({})", line);
                continue;
            }

            match tokens[0] {
                "SerialNumber" => DMC = Some(tokens[1].to_string()),
                /*"MainSerial" => DMC_mb = Some(tokens[1].to_string()),
                "Part Type" => {
                    if let Some((t, sw)) = tokens[1].split_once('-') {
                        product_id = Some(t.to_string());
                        SW_version = Some(sw.to_string());
                    }
                }*/
                "Start Time" => {
                    if let Ok(time) =
                        chrono::NaiveDateTime::parse_from_str(tokens[1], "%Y.%m.%d. %H:%M")
                    {
                        time_start = Some(time);
                        time_start_u64 = time_to_u64(time);
                    } else {
                        println!("Time conversion error!");
                    }
                }
                "Testing time(sec)" => {
                    if let Ok(dt) = tokens[1].parse() {
                        testing_time = dt;
                        tests.push(Test {
                            name: "Testing time".to_string(),
                            ttype: TType::Time,
                            result: (BResult::Pass, dt as f32),
                            limits: TLimit::None,
                        });
                    }
                }
                "Result" => result = Some(tokens[1].to_string()),
                "Error Code" => {
                    if let Ok(s) = tokens[1].parse() {
                        status = Some(s);
                    }
                }
                _ => {
                    if tokens.len() != 6 {
                        println!("Tokens: {tokens:?}");
                        continue;
                    }
                    if tokens[0] == "StepName" {
                        continue;
                    }

                    if let Ok(mut meas) = tokens[2].parse::<f32>() {
                        if tokens[4] == "mA" {
                            meas /= 1000.0;
                        }
                        if tokens[4] == "kHZ" || tokens[4] == "kHz" {
                            meas *= 1000.0;
                        }

                        let limits = if let Ok(mut min) = tokens[1].parse::<f32>() {
                            if let Ok(mut max) = tokens[3].parse::<f32>() {
                                if tokens[4] == "mA" {
                                    min /= 1000.0;
                                    max /= 1000.0;
                                }
                                if tokens[4] == "kHZ" || tokens[4] == "kHz" {
                                    min *= 1000.0;
                                    max *= 1000.0;
                                }

                                TLimit::Lim2(min, max)
                            } else {
                                TLimit::None
                            }
                        } else {
                            TLimit::None
                        };

                        let result = (
                            if tokens[5] == "Passed" {
                                BResult::Pass
                            } else {
                                BResult::Fail
                            },
                            meas,
                        );

                        tests.push(Test {
                            name: tokens[0].to_string(),
                            ttype: TType::from(tokens[4]),
                            result,
                            limits,
                        });
                    }
                }
            }
        }

        if tests.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "Logfile conatined no tests!",
            ));
        }

        let time_end: u64 = if let Some(start) = time_start {
            if testing_time > 0 {
                let end_time = start + std::time::Duration::from_secs(testing_time);
                time_to_u64(end_time)
            } else {
                time_start_u64
            }
        } else {
            0
        };

        let result = LogFile {
            source,
            DMC: DMC.clone().unwrap_or_default(),
            DMC_mb: DMC.unwrap_or_default(), //DMC_mb.unwrap_or_default(),
            product_id: "Kaized CMD".to_string(), //product_id.unwrap_or_default(),
            index: 1,
            result: result.is_some_and(|f| f == "Passed"),
            status: status.unwrap_or_default(),
            status_str: String::new(),
            time_start: time_start_u64,
            time_end,
            tests,
            report: String::new(),
            SW_version: String::new(), //SW_version.unwrap_or_default(),
        };

        //println!("Result: {result:?}");

        Ok(result)
    }

    pub fn load_ICT(p: &Path) -> io::Result<Self> {
        println!("INFO: Loading (v2) file {}", p.display());
        let source = p.as_os_str().to_owned();

        let mut product_id = String::from("NoID");
        //let mut revision_id = String::new(); // ! New, needs to be implemented in the program

        let mut DMC = String::from("NoDMC");
        let mut DMC_mb = String::from("NoMB");
        let mut index = 1;
        let mut time_start: u64 = 0;
        let mut time_end: u64 = 0;
        let mut status = 0;

        let mut tests: Vec<Test> = Vec::new();
        let mut report: Vec<String> = Vec::new();
        let mut failed_nodes: Vec<String> = Vec::new();
        let mut failed_pins: Vec<String> = Vec::new();

        // pre-populate pins test
        tests.push(Test {
            name: "pins".to_owned(),
            ttype: TType::Pin,
            result: (BResult::Unknown, 0.0),
            limits: TLimit::None,
        });
        //

        // Variables for user defined blocks:
        let mut PS_counter = 0;
        let mut SW_version = String::new();
        //

        let tree = keysight_log::parse_file(p)?;
        let mut batch_node: Option<&keysight_log::TreeNode> = None;
        let mut btest_node: Option<&keysight_log::TreeNode> = None;

        if let Some(batch) = tree.last() {
            // {@BATCH|UUT type|UUT type rev|fixture id|testhead number|testhead type|process step|batch id|
            //      operator id|controller|testplan id|testplan rev|parent panel type|parent panel type rev (| version label)}
            if let keysight_log::KeysightPrefix::Batch(
                p_id,
                _, //r_id,
                _,
                _,
                _,
                _,
                _,
                _,
                _,
                _,
                _,
                _,
                _,
                _,
            ) = &batch.data
            {
                product_id = p_id.clone();
                //revision_id = r_id.clone();
                batch_node = Some(batch);
            } else {
                eprintln!("W: No BATCH field found!");
            }
        }

        if let Some(btest) = {
            if let Some(x) = batch_node {
                x.branches.last()
            } else {
                tree.last()
            }
        } {
            // {@BTEST|board id|test status|start datetime|duration|multiple test|log level|log set|learning|
            // known good|end datetime|status qualifier|board number|parent panel id}
            if let keysight_log::KeysightPrefix::BTest(
                b_id,
                b_status,
                t_start,
                _,
                _,
                _,
                _,
                _,
                _,
                t_end,
                _,
                b_index,
                mb_id,
            ) = &btest.data
            {
                DMC = b_id.clone();

                if let Some(mb) = mb_id {
                    DMC_mb = mb.clone();
                } else {
                    DMC_mb = DMC.clone();
                }

                status = *b_status;
                time_start = *t_start;
                time_end = *t_end;
                index = *b_index as usize;
                btest_node = Some(btest);
            } else {
                eprintln!("W: No BTEST field found!");
            }
        }

        let test_nodes = if let Some(x) = btest_node {
            &x.branches
        } else {
            &tree
        };

        for test in test_nodes {
            match &test.data {
                // I haven't encountered any analog fields outside of a BLOCK, so this might be not needed.
                keysight_log::KeysightPrefix::Analog(analog, status, result, sub_name) => {
                    if let Some(name) = sub_name {
                        let limits = match test.branches.first() {
                            Some(lim) => match lim.data {
                                keysight_log::KeysightPrefix::Lim2(max, min) => {
                                    TLimit::Lim2(max, min)
                                }
                                keysight_log::KeysightPrefix::Lim3(nom, max, min) => {
                                    TLimit::Lim3(nom, max, min)
                                }
                                _ => {
                                    eprintln!(
                                        "ERR: Analog test limit parsing error!\n\t{:?}",
                                        lim.data
                                    );
                                    TLimit::None
                                }
                            },
                            None => TLimit::None,
                        };

                        for subfield in test.branches.iter().skip(1) {
                            match &subfield.data {
                                keysight_log::KeysightPrefix::Report(rpt) => {
                                    report.push(rpt.clone());
                                }
                                _ => {
                                    eprintln!("ERR: Unhandled subfield!\n\t{:?}", subfield.data)
                                }
                            }
                        }

                        tests.push(Test {
                            name: strip_index(name).to_string(),
                            ttype: TType::from(*analog),
                            result: (BResult::from(*status), *result),
                            limits,
                        })
                    } else {
                        eprintln!(
                            "ERR: Analog test outside of a BLOCK and without name!\n\t{:?}",
                            test.data
                        );
                    }
                }
                keysight_log::KeysightPrefix::AlarmId(_, _) => todo!(),
                keysight_log::KeysightPrefix::Alarm(_, _, _, _, _, _, _, _, _) => todo!(),
                keysight_log::KeysightPrefix::Array(_, _, _, _) => todo!(),
                keysight_log::KeysightPrefix::Block(b_name, _) => {
                    let block_name = strip_index(b_name).to_string();
                    let mut digital_tp: Option<usize> = None;
                    let mut boundary_tp: Option<usize> = None;

                    for sub_test in &test.branches {
                        match &sub_test.data {
                            keysight_log::KeysightPrefix::Analog(
                                analog,
                                status,
                                result,
                                sub_name,
                            ) => {
                                let limits = match sub_test.branches.first() {
                                    Some(lim) => match lim.data {
                                        keysight_log::KeysightPrefix::Lim2(max, min) => {
                                            TLimit::Lim2(max, min)
                                        }
                                        keysight_log::KeysightPrefix::Lim3(nom, max, min) => {
                                            TLimit::Lim3(nom, max, min)
                                        }
                                        _ => {
                                            eprintln!(
                                                "ERR: Analog test limit parsing error!\n\t{:?}",
                                                lim.data
                                            );
                                            TLimit::None
                                        }
                                    },
                                    None => TLimit::None,
                                };

                                for subfield in sub_test.branches.iter().skip(1) {
                                    match &subfield.data {
                                        keysight_log::KeysightPrefix::Report(rpt) => {
                                            report.push(rpt.clone());
                                        }
                                        _ => {
                                            eprintln!(
                                                "ERR: Unhandled subfield!\n\t{:?}",
                                                subfield.data
                                            )
                                        }
                                    }
                                }

                                let mut name = block_name.clone();
                                if let Some(sn) = &sub_name {
                                    name = format!("{}%{}", name, sn);
                                }

                                tests.push(Test {
                                    name,
                                    ttype: TType::from(*analog),
                                    result: (BResult::from(*status), *result),
                                    limits,
                                })
                            }
                            keysight_log::KeysightPrefix::Digital(status, _, _, _, sub_name) => {
                                // subrecords: DPIN - ToDo!

                                for subfield in sub_test.branches.iter() {
                                    match &subfield.data {
                                        keysight_log::KeysightPrefix::Report(rpt) => {
                                            report.push(rpt.clone());
                                        }
                                        _ => {
                                            eprintln!(
                                                "ERR: Unhandled subfield!\n\t{:?}",
                                                subfield.data
                                            )
                                        }
                                    }
                                }

                                if let Some(dt) = digital_tp {
                                    if *status != 0 {
                                        tests[dt].result = (BResult::from(*status), *status as f32);
                                    }
                                } else {
                                    digital_tp = Some(tests.len());
                                    tests.push(Test {
                                        name: strip_index(sub_name).to_string(),
                                        ttype: TType::Digital,
                                        result: (BResult::from(*status), *status as f32),
                                        limits: TLimit::None,
                                    });
                                }
                            }
                            keysight_log::KeysightPrefix::TJet(status, _, sub_name) => {
                                for subfield in sub_test.branches.iter() {
                                    match &subfield.data {
                                        keysight_log::KeysightPrefix::Report(rpt) => {
                                            report.push(rpt.clone());
                                        }
                                        keysight_log::KeysightPrefix::DPin(_, pins) => {
                                            let mut tmp: Vec<String> =
                                                pins.iter().map(|f| f.0.clone()).collect();
                                            failed_nodes.append(&mut tmp);
                                        }
                                        _ => {
                                            eprintln!(
                                                "ERR: Unhandled subfield!\n\t{:?}",
                                                subfield.data
                                            )
                                        }
                                    }
                                }

                                let name = format!("{}%{}", block_name, strip_index(sub_name));
                                tests.push(Test {
                                    name,
                                    ttype: TType::Testjet,
                                    result: (BResult::from(*status), *status as f32),
                                    limits: TLimit::None,
                                })
                            }
                            keysight_log::KeysightPrefix::Boundary(sub_name, status, _, _) => {
                                // Subrecords: BS-O, BS-S - ToDo

                                for subfield in sub_test.branches.iter() {
                                    match &subfield.data {
                                        keysight_log::KeysightPrefix::Report(rpt) => {
                                            report.push(rpt.clone());
                                        }
                                        _ => {
                                            eprintln!(
                                                "ERR: Unhandled subfield!\n\t{:?}",
                                                subfield.data
                                            )
                                        }
                                    }
                                }

                                if let Some(dt) = boundary_tp {
                                    if *status != 0 {
                                        tests[dt].result = (BResult::from(*status), *status as f32);
                                    }
                                } else {
                                    boundary_tp = Some(tests.len());
                                    tests.push(Test {
                                        name: strip_index(sub_name).to_string(),
                                        ttype: TType::BoundaryS,
                                        result: (BResult::from(*status), *status as f32),
                                        limits: TLimit::None,
                                    })
                                }
                            }
                            keysight_log::KeysightPrefix::Report(rpt) => {
                                report.push(rpt.clone());
                            }
                            keysight_log::KeysightPrefix::UserDefined(s) => {
                                eprintln!("ERR: Not implemented USER DEFINED block!\n\t{:?}", s);
                            }
                            keysight_log::KeysightPrefix::Error(s) => {
                                eprintln!("ERR: KeysightPrefix::Error found!\n\t{:?}", s);
                            }
                            _ => {
                                eprintln!(
                                    "ERR: Found a invalid field nested in BLOCK!\n\t{:?}",
                                    sub_test.data
                                );
                            }
                        }
                    }
                }

                // Boundary exists in BLOCK and as a solo filed if it fails.
                keysight_log::KeysightPrefix::Boundary(test_name, status, _, _) => {
                    // Subrecords: BS-O, BS-S - ToDo

                    for subfield in test.branches.iter() {
                        match &subfield.data {
                            keysight_log::KeysightPrefix::Report(rpt) => {
                                report.push(rpt.clone());
                            }
                            _ => {
                                eprintln!("ERR: Unhandled subfield!\n\t{:?}", subfield.data)
                            }
                        }
                    }

                    tests.push(Test {
                        name: strip_index(test_name).to_string(),
                        ttype: TType::BoundaryS,
                        result: (BResult::from(*status), *status as f32),
                        limits: TLimit::None,
                    })
                }

                // Digital tests can be present as a BLOCK member, or solo.
                keysight_log::KeysightPrefix::Digital(status, _, _, _, test_name) => {
                    for subfield in test.branches.iter() {
                        match &subfield.data {
                            keysight_log::KeysightPrefix::DPin(_, pins) => {
                                let mut tmp: Vec<String> =
                                    pins.iter().map(|f| f.0.clone()).collect();
                                failed_nodes.append(&mut tmp);
                            }
                            keysight_log::KeysightPrefix::Report(rpt) => {
                                report.push(rpt.clone());
                            }
                            _ => {
                                eprintln!("ERR: Unhandled subfield!\n\t{:?}", subfield.data)
                            }
                        }
                    }

                    tests.push(Test {
                        name: strip_index(test_name).to_string(),
                        ttype: TType::Digital,
                        result: (BResult::from(*status), *status as f32),
                        limits: TLimit::None,
                    })
                }
                keysight_log::KeysightPrefix::Pins(_, status, _) => {
                    // Subrecord: Pin - ToDo
                    for subfield in test.branches.iter() {
                        match &subfield.data {
                            keysight_log::KeysightPrefix::Report(rpt) => {
                                report.push(rpt.clone());
                            }
                            keysight_log::KeysightPrefix::Pin(pin) => {
                                failed_pins.append(&mut pin.clone());
                            }
                            _ => {
                                eprintln!("ERR: Unhandled subfield!\n\t{:?}", subfield.data)
                            }
                        }
                    }

                    tests[0].result = (BResult::from(*status), *status as f32);
                }
                keysight_log::KeysightPrefix::Report(rpt) => {
                    report.push(rpt.clone());
                }

                // I haven't encountered any testjet fields outside of a BLOCK, so this might be not needed.
                keysight_log::KeysightPrefix::TJet(status, _, test_name) => {
                    // subrecords: DPIN - ToDo!
                    for subfield in test.branches.iter() {
                        match &subfield.data {
                            keysight_log::KeysightPrefix::Report(rpt) => {
                                report.push(rpt.clone());
                            }
                            _ => {
                                eprintln!("ERR: Unhandled subfield!\n\t{:?}", subfield.data)
                            }
                        }
                    }

                    tests.push(Test {
                        name: strip_index(test_name).to_string(),
                        ttype: TType::Testjet,
                        result: (BResult::from(*status), *status as f32),
                        limits: TLimit::None,
                    })
                }
                keysight_log::KeysightPrefix::Shorts(mut status, s1, s2, s3, _) => {
                    // Sometimes, failed shorts tests are marked as passed at the 'test status' field.
                    // So we check the next 3 fields too, they all have to be '000'
                    if *s1 > 0 || *s2 > 0 || *s3 > 0 {
                        status = 1;
                    }

                    for subfield in test.branches.iter() {
                        match &subfield.data {
                            keysight_log::KeysightPrefix::Report(rpt) => {
                                report.push(rpt.clone());
                            }
                            keysight_log::KeysightPrefix::ShortsSrc(_, _, node) => {
                                failed_nodes.push(node.clone());
                                for sub2 in &subfield.branches {
                                    match &sub2.data {
                                        keysight_log::KeysightPrefix::Report(rpt) => {
                                            report.push(rpt.clone());
                                        }
                                        keysight_log::KeysightPrefix::ShortsDest(dst) => {
                                            let mut tmp: Vec<String> =
                                                dst.iter().map(|d| d.0.clone()).collect();
                                            failed_nodes.append(&mut tmp);
                                        }
                                        _ => {
                                            eprintln!("ERR: Unhandled subfield!\n\t{:?}", sub2.data)
                                        }
                                    }
                                }
                            }
                            keysight_log::KeysightPrefix::ShortsOpen(src, dst, _) => {
                                failed_nodes.push(src.clone());
                                failed_nodes.push(dst.clone());

                                for sub2 in &subfield.branches {
                                    match &sub2.data {
                                        keysight_log::KeysightPrefix::Report(rpt) => {
                                            report.push(rpt.clone());
                                        }
                                        _ => {
                                            eprintln!("ERR: Unhandled subfield!\n\t{:?}", sub2.data)
                                        }
                                    }
                                }
                            }
                            _ => {
                                eprintln!("ERR: Unhandled subfield!\n\t{:?}", subfield.data)
                            }
                        }
                    }

                    tests.push(Test {
                        name: String::from("shorts"),
                        ttype: TType::Shorts,
                        result: (BResult::from(status), status as f32),
                        limits: TLimit::None,
                    })
                }
                keysight_log::KeysightPrefix::UserDefined(s) => match s[0].as_str() {
                    "@Programming_time" => {
                        if s.len() < 2 {
                            eprintln!("ERR: Parsing error at @Programming_time!\n\t{:?}", s);
                            continue;
                        }

                        if let Some(t) = s[1].strip_suffix("msec") {
                            if let Ok(ts) = t.parse::<i32>() {
                                tests.push(Test {
                                    name: String::from("Programming_time"),
                                    ttype: TType::Unknown,
                                    result: (BResult::Pass, ts as f32 / 1000.0),
                                    limits: TLimit::None,
                                })
                            } else {
                                eprintln!("ERR: Parsing error at @Programming_time!\n\t{:?}", s);
                            }
                        } else {
                            eprintln!("ERR: Parsing error at @Programming_time!\n\t{:?}", s);
                        }
                    }
                    "@PS_info" => {
                        if s.len() < 3 {
                            eprintln!("ERR: Parsing error at @PS_info!\n\t{:?}", s);
                            continue;
                        }

                        let voltage;
                        let current;

                        if let Some(t) = s[1].strip_suffix('V') {
                            if let Ok(ts) = t.parse::<f32>() {
                                voltage = ts;
                            } else {
                                eprintln!("ERR: Parsing error at @PS_info!\n\t{:?}", s);
                                continue;
                            }
                        } else {
                            eprintln!("ERR: Parsing error at @PS_info!\n\t{:?}", s);
                            continue;
                        }

                        if let Some(t) = s[2].strip_suffix('A') {
                            if let Ok(ts) = t.parse::<f32>() {
                                current = ts;
                            } else {
                                eprintln!("ERR: Parsing error at @PS_info!\n\t{:?}", s);
                                continue;
                            }
                        } else {
                            eprintln!("ERR: Parsing error at @PS_info!\n\t{:?}", s);
                            continue;
                        }

                        println!("{} - {}", voltage, current);
                        PS_counter += 1;
                        tests.push(Test {
                            name: format!("PS_Info_{PS_counter}%Voltage"),
                            ttype: TType::Measurement,
                            result: (BResult::Pass, voltage),
                            limits: TLimit::None,
                        });
                        tests.push(Test {
                            name: format!("PS_Info_{PS_counter}%Current"),
                            ttype: TType::Current,
                            result: (BResult::Pass, current),
                            limits: TLimit::None,
                        });
                    }
                    x if x.starts_with("@MySW") => {
                        if let Some(x) = s.get(1) {
                            SW_version = x.to_string();
                        }
                    }
                    _ => {
                        eprintln!("ERR: Not implemented USER DEFINED block!\n\t{:?}", s);
                    }
                },
                keysight_log::KeysightPrefix::Error(s) => {
                    eprintln!("ERR: KeysightPrefix::Error found!\n\t{:?}", s);
                }
                _ => {
                    eprintln!(
                        "ERR: Found a invalid field nested in BTEST!\n\t{:?}",
                        test.data
                    );
                }
            }
        }

        // Check for the case, when the status is set as failed, but we found no failing tests.
        if status != 0 && !tests.iter().any(|f| f.result.0 == BResult::Fail) {
            // Push in a dummy failed test
            tests.push(Test {
                name: format!(
                    "Status_code:{}_-_{}",
                    status,
                    keysight_log::status_to_str(status)
                ),
                ttype: TType::Unknown,
                result: (BResult::Fail, 0.0),
                limits: TLimit::None,
            });
        }

        if time_start == 0 {
            if let Ok(x) = p.metadata() {
                time_start = local_time_to_u64(x.modified().unwrap().into());
            }
        }

        if time_end == 0 {
            time_end = time_start;
        }

        Ok(LogFile {
            source,
            DMC,
            DMC_mb,
            product_id,
            index,
            result: status == 0,
            status,
            status_str: keysight_log::status_to_str(status),
            time_start,
            time_end,
            tests,
            report: report.join("\n"),
            SW_version,
        })
    }

    pub fn is_ok(&self) -> bool {
        !self.tests.is_empty() && self.DMC != "NoDMC" && self.DMC_mb != "NoMB"
    }

    pub fn has_report(&self) -> bool {
        !self.report.is_empty()
    }

    pub fn get_source(&self) -> &OsString {
        &self.source
    }

    pub fn get_status(&self) -> i32 {
        self.status
    }

    pub fn get_status_str(&self) -> &str {
        &self.status_str
    }

    pub fn get_product_id(&self) -> &str {
        &self.product_id
    }

    pub fn get_DMC(&self) -> &str {
        &self.DMC
    }

    pub fn get_main_DMC(&self) -> &str {
        &self.DMC_mb
    }

    pub fn get_time_start(&self) -> u64 {
        self.time_start
    }

    pub fn get_time_end(&self) -> u64 {
        self.time_end
    }

    pub fn get_report(&self) -> &str {
        &self.report
    }

    pub fn get_SW_ver(&self) -> &str {
        &self.SW_version
    }

    pub fn get_tests(&self) -> &Vec<Test> {
        &self.tests
    }

    pub fn get_failed_tests(&self) -> Vec<String> {
        let mut ret = Vec::new();

        if self.status != 0 {
            for test in self.tests.iter() {
                if test.result.0 == BResult::Fail {
                    ret.push(test.name.clone());
                }
            }
        }

        ret
    }
}

struct Log {
    source: OsString,
    time_s: u64,
    time_e: u64,
    result: BResult, // Could use a bool too, as it can't be Unknown

    results: Vec<TResult>,
    limits: Vec<TLimit>,

    report: String,
}

impl Log {
    fn new(log: LogFile) -> Self {
        let mut results: Vec<TResult> = Vec::new();
        let mut limits: Vec<TLimit> = Vec::new();

        for t in log.tests {
            results.push(t.result);
            limits.push(t.limits);
        }

        Self {
            source: log.source,
            time_s: log.time_start,
            time_e: log.time_end,
            result: log.result.into(),
            results,
            limits,
            report: log.report,
        }
    }

    fn get_failed_test_list(&self) -> Vec<usize> {
        let mut ret = Vec::new();

        for res in self.results.iter().enumerate() {
            if res.1 .0 == BResult::Fail {
                ret.push(res.0);
            }
        }

        ret
    }
}

struct Board {
    DMC: String,
    logs: Vec<Log>,
    index: usize, // Number on the multiboard, goes from 1 to 20
}

impl Board {
    fn new(index: usize) -> Self {
        Self {
            DMC: String::new(),
            logs: Vec::new(),
            index,
        }
    }

    fn push(&mut self, log: LogFile) -> bool {
        // a) Board is empty
        if self.DMC.is_empty() {
            self.DMC = log.DMC.to_owned();
            self.logs.push(Log::new(log));
        // b) Board is NOT empty
        } else {
            self.logs.push(Log::new(log));
        }

        true
    }

    fn update(&mut self) {
        self.logs.sort_by_key(|k| k.time_s);
    }

    fn all_ok(&self) -> bool {
        for l in &self.logs {
            if l.result == BResult::Fail {
                return false;
            }
        }
        true
    }

    fn get_reports(&self) -> String {
        //let mut ret: Vec<String> = vec![format!("{} - {}", self.index, self.DMC)];
        let mut ret: Vec<String> = Vec::new();

        for (i, log) in self.logs.iter().enumerate() {
            if log.result == BResult::Pass {
                ret.push(format!("Log #{i} - {}: Pass\n", u64_to_string(log.time_e)));
            } else {
                ret.push(format!("Log #{i} - {}: Fail\n", u64_to_string(log.time_e)));

                if log.report.is_empty() {
                    ret.push(String::from("No report field found in log!\n"));
                    ret.push(String::from("Enable it in testplan!\n"));
                } else {
                    ret.push(log.report.clone());
                }
            }

            ret.push("\n".to_string());
        }

        ret.join("\n")
    }

    fn export_to_col(
        &self,
        sheet: &mut rust_xlsxwriter::Worksheet,
        mut c: u16,
        only_failure: bool,
        only_final: bool,
        export_list: &[usize],
        num_format: &rust_xlsxwriter::Format,
    ) -> u16 {
        if self.logs.is_empty() {
            return c;
        }

        if only_failure && self.all_ok() {
            return c;
        }

        if only_final && only_failure && self.logs.last().is_some_and(|x| x.result == BResult::Pass)
        {
            return c;
        }

        let format_with_wrap = rust_xlsxwriter::Format::new().set_text_wrap();

        let log_slice = {
            if only_final {
                &self.logs[self.logs.len() - 1..]
            } else {
                &self.logs[..]
            }
        };

        for l in log_slice {
            if only_failure && l.result == BResult::Pass {
                continue;
            }

            // DMC in a merged 2x2 range
            let _ = sheet.merge_range(0, c, 1, c + 1, &self.DMC, &format_with_wrap);

            // Log result and time of test
            let _ = sheet.write(2, c, l.result.print());
            let _ = sheet.write_with_format(2, c + 1, u64_to_string(l.time_s), &format_with_wrap);

            let _ = sheet.set_column_width(c, 8);
            let _ = sheet.set_column_width(c + 1, 14);

            // Print measurement results
            for (i, t) in export_list.iter().enumerate() {
                if let Some(res) = l.results.get(*t) {
                    if res.0 != BResult::Unknown {
                        let _ = sheet.write(3 + i as u32, c, res.0.print());
                        let _ =
                            sheet.write_number_with_format(3 + i as u32, c + 1, res.1, num_format);
                    }
                }
            }
            c += 2;
        }

        c
    }

    fn export_to_line(
        &self,
        sheet: &mut rust_xlsxwriter::Worksheet,
        mut l: u32,
        only_failure: bool,
        only_final: bool,
        export_list: &[usize],
        num_format: &rust_xlsxwriter::Format,
    ) -> u32 {
        if self.logs.is_empty() {
            return l;
        }

        if only_failure && self.all_ok() {
            return l;
        }

        if only_final && only_failure && self.logs.last().is_some_and(|x| x.result == BResult::Pass)
        {
            return l;
        }

        let log_slice = {
            if only_final {
                &self.logs[self.logs.len() - 1..]
            } else {
                &self.logs[..]
            }
        };

        for log in log_slice {
            if only_failure && log.result == BResult::Pass {
                continue;
            }

            // DMC
            let _ = sheet.write(l, 0, &self.DMC);

            // Log result and time of test
            let _ = sheet.write(l, 2, log.result.print());
            let _ = sheet.write(l, 1, u64_to_string(log.time_s));

            // Print measurement results
            for (i, t) in export_list.iter().enumerate() {
                if let Some(res) = log.results.get(*t) {
                    if res.0 != BResult::Unknown {
                        let c = i as u16 * 2 + 3;
                        let _ = sheet.write(l, c, res.0.print());
                        let _ = sheet.write_number_with_format(l, c + 1, res.1, num_format);
                    }
                }
            }
            l += 1;
        }

        l
    }
}

#[derive(Clone, Debug)]
pub struct MbResult {
    pub start: u64,
    pub end: u64,
    pub result: BResult,
    pub panels: Vec<BResult>,
}
struct MultiBoard {
    DMC: String,
    golden_sample: bool,
    boards: Vec<Board>,

    // ( Start time, Multiboard test result, <Result of the individual boards>)
    results: Vec<MbResult>,
}

impl MultiBoard {
    fn new() -> Self {
        Self {
            DMC: String::new(),
            golden_sample: false,
            boards: Vec::new(),
            results: Vec::new(),
            //first_res: BResult::Unknown,
            //final_res: BResult::Unknown
        }
    }

    // Q: should we check for the DMC of the board? If the main DMC and index is matching then it should be OK.
    fn push(&mut self, log: LogFile) -> bool {
        if self.DMC.is_empty() {
            self.DMC = log.DMC_mb.to_owned();
        }

        while self.boards.len() < log.index {
            self.boards.push(Board::new(self.boards.len() + 1))
        }

        self.boards[log.index - 1].push(log)
    }

    fn set_gs(&mut self) {
        self.golden_sample = true;
    }

    // Generating stats for self, and reporting single-board stats.
    fn update(&mut self) -> (Yield, Yield, Yield) {
        let mut sb_first_yield = Yield(0, 0);
        let mut sb_final_yield = Yield(0, 0);
        let mut sb_total_yield = Yield(0, 0);

        for sb in &mut self.boards {
            sb.update();
        }

        self.update_results();

        for result in &self.results {
            for r in &result.panels {
                if *r == BResult::Pass {
                    sb_total_yield.0 += 1;
                } else if *r == BResult::Fail {
                    sb_total_yield.1 += 1;
                }
            }
        }

        if let Some(x) = self.results.first() {
            for r in &x.panels {
                if *r == BResult::Pass {
                    sb_first_yield.0 += 1;
                } else if *r == BResult::Fail {
                    sb_first_yield.1 += 1;
                } else {
                    //println!("First is Unknown!");
                }
            }
        }

        if let Some(x) = self.results.last() {
            for r in &x.panels {
                if *r == BResult::Pass {
                    sb_final_yield.0 += 1;
                } else if *r == BResult::Fail {
                    sb_final_yield.1 += 1;
                } else {
                    //println!("Last is Unknown!");
                }
            }
        }

        (sb_first_yield, sb_final_yield, sb_total_yield)
    }

    fn update_results(&mut self) {
        self.results.clear();

        for b in &self.boards {
            'forlog: for l in &b.logs {
                // 1 - check if there is a results with matching "time"
                for r in &mut self.results {
                    if r.start == l.time_s {
                        // write the BResult in to r.2.index
                        r.panels[b.index - 1] = l.result;

                        // if time_e is higher than the saved end time, then overwrite it
                        if r.end < l.time_e {
                            r.end = l.time_e;
                        }
                        continue 'forlog;
                    }
                }
                // 2 - if not then make one
                let mut new_res = MbResult {
                    start: l.time_s,
                    end: l.time_e,
                    result: BResult::Unknown,
                    panels: vec![BResult::Unknown; self.boards.len()],
                };
                new_res.panels[b.index - 1] = l.result;
                self.results.push(new_res);
            }
        }

        // At the end we have to update the 2nd field of the results.
        for res in &mut self.results {
            let mut all_ok = true;
            let mut has_unknown = false;
            for r in &res.panels {
                match r {
                    BResult::Unknown => has_unknown = true,
                    BResult::Fail => all_ok = false,
                    _ => (),
                }
            }

            if !all_ok {
                res.result = BResult::Fail;
            } else if has_unknown {
                res.result = BResult::Unknown;
            } else {
                res.result = BResult::Pass
            }
        }

        // Sort results by time.
        self.results.sort_by_key(|k| k.start);
    }

    fn get_results(&self) -> &Vec<MbResult> {
        &self.results
    }

    fn get_failures(&self, setting: FlSettings) -> Vec<(usize, usize, String, u64)> {
        let mut failures: Vec<(usize, usize, String, u64)> = Vec::new(); // (test number, board index, DMC, time)

        for b in &self.boards {
            if b.logs.is_empty() {
                continue;
            }

            let logs = match setting {
                FlSettings::All => &b.logs,
                FlSettings::AfterRetest => &b.logs[b.logs.len() - 1..],
                FlSettings::FirstPass => &b.logs[..1],
            };

            for l in logs {
                if l.result == BResult::Pass {
                    continue;
                }
                for (i, r) in l.results.iter().enumerate() {
                    if r.0 == BResult::Fail {
                        failures.push((i, b.index, b.DMC.clone(), l.time_s));
                    }
                }
            }
        }

        failures
    }

    // Get the measurments for test "testid". Vec<(time, index, result, limits)>
    fn get_stats_for_test(&self, testid: usize) -> Vec<(u64, usize, TResult, TLimit)> {
        let mut resultlist: Vec<(u64, usize, TResult, TLimit)> = Vec::new();

        for sb in &self.boards {
            let index = sb.index;
            for l in &sb.logs {
                let time = l.time_s;
                if let Some(result) = l.results.get(testid) {
                    resultlist.push((time, index, *result, l.limits[testid]))
                }
            }
        }

        resultlist
    }
}

pub struct LogFileHandler {
    // Statistics:
    pp_multiboard: usize, // Panels Per Multiboard (1-20), can only be determined once everything is loaded. Might not need it.

    mb_first_yield: Yield,
    sb_first_yield: Yield,
    mb_final_yield: Yield,
    sb_final_yield: Yield,
    mb_total_yield: Yield,
    sb_total_yield: Yield,

    product_id: String, // Product identifier
    product: Option<Product>,
    golden_samples: Vec<String>,

    testlist: Vec<TList>,
    multiboards: Vec<MultiBoard>,

    sourcelist: HashSet<OsString>,
}

#[derive(Default)]
pub struct HourlyYield {
    pub panels: Yield,
    pub panels_with_gs: Yield,
    pub boards: Yield,
    pub boards_with_gs: Yield,
}

pub type HourlyStats = (u64, HourlyYield, Vec<(BResult, u64, String, bool)>); // (time, [(OK, NOK), (OK, NOK with gs)], Vec<Results>)
pub type MbStats = (String, Vec<MbResult>, bool); // (DMC, Vec<(time, Multiboard result, Vec<Board results>)>, golden_sample)

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum FlSettings {
    FirstPass,
    All,
    AfterRetest,
}

impl Default for LogFileHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl LogFileHandler {
    pub fn new() -> Self {
        LogFileHandler {
            pp_multiboard: 0,
            mb_first_yield: Yield(0, 0),
            sb_first_yield: Yield(0, 0),
            mb_final_yield: Yield(0, 0),
            sb_final_yield: Yield(0, 0),
            mb_total_yield: Yield(0, 0),
            sb_total_yield: Yield(0, 0),
            product_id: String::new(),
            product: None,
            golden_samples: Vec::new(),
            testlist: Vec::new(),
            multiboards: Vec::new(),
            sourcelist: HashSet::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.multiboards.is_empty()
    }

    pub fn push_from_file(&mut self, p: &Path) -> bool {
        //println!("INFO: Pushing file {} into log-stack", p.display());
        if let Ok(log) = LogFile::load(p) {
            self.push(log)
        } else {
            false
        }
    }

    pub fn push(&mut self, mut log: LogFile) -> bool {
        println!("\tProcessing logfile: {:?}", log.source);

        if self.sourcelist.contains(&log.source) {
            println!("\t\tW: Logfile already loaded!");
            return false;
        }

        self.sourcelist.insert(log.source.clone());

        if self.product_id.is_empty() {
            println!("\t\tINFO: Initializing as {}", log.product_id);
            self.product_id = log.product_id.to_owned();

            if let Some(product) = get_product_for_serial(ICT_config::PRODUCT_LIST, &log.DMC_mb) {
                self.golden_samples = load_gs_list_for_product(ICT_config::GOLDEN_LIST, &product);
                self.product = Some(product);
            }

            println!("\t\t\tProduct is: {:?}", self.product);
            println!("\t\t\tGolden samples: {:?}", self.golden_samples);

            // Create testlist
            for t in log.tests.iter() {
                self.testlist.push((t.name.to_owned(), t.ttype));
            }

            self.multiboards.push(MultiBoard::new());

            if self.golden_samples.contains(&log.DMC_mb) {
                self.multiboards[0].set_gs();
            }

            self.multiboards[0].push(log)
        } else {
            // Check if it is for the same type.
            // Mismatched types are not supported. (And ATM I see no reason to do that.)
            if self.product_id != log.product_id {
                println!(
                    "\t\tERR: Product type mismatch detected! {} =/= {}\n\t\t {:?}",
                    self.product_id, log.product_id, log.source
                );
                return false;
            }

            /*
                ToDo: Check for version (D5?)
                Need to add version info to logfile, and product_list.
            */

            // If the testlist is missing any entries, add them
            for test in &log.tests {
                if !self.testlist.iter().any(|e| e.0 == test.name) {
                    println!(
                        "\t\tW: Test {} was missing from testlist. Adding.",
                        test.name
                    );
                    self.testlist.push((test.name.clone(), test.ttype));
                }
            }

            // log.tests is always shorter or = than the testlist
            log.tests.resize(
                self.testlist.len(),
                Test {
                    name: String::new(),
                    ttype: TType::Unknown,
                    result: (BResult::Unknown, 0.0),
                    limits: TLimit::None,
                },
            );

            let len = log.tests.len(); // log.tests is always shorter than the testlist
            let mut buffer_i: Vec<usize> = Vec::new();

            // Get diff
            let mut q = 0;

            for i in 0..len {
                if self.testlist[i].0 != log.tests[i].name {
                    if !log.tests[i].name.is_empty() {
                        q += 1;
                        println!(
                            "\t\tW: Test mismatch: {} =/= {}",
                            self.testlist[i].0, log.tests[i].name
                        );
                    }
                    buffer_i.push(i);
                }
            }

            if q > 0 {
                print!(
                    "\t\tFound {} ({}) mismatches, re-ordering... ",
                    q,
                    buffer_i.len()
                );
                let mut tmp: Vec<Test> = Vec::new();
                for i in &buffer_i {
                    tmp.push(log.tests[*i].clone());
                    log.tests[*i].clear();
                }

                for i in &buffer_i {
                    for t in &tmp {
                        if self.testlist[*i].0 == t.name {
                            log.tests[*i] = t.clone();
                        }
                    }
                }

                println!("Done!");
            }

            // Check if the MultiBoard already exists.
            for mb in self.multiboards.iter_mut() {
                if mb.DMC == log.DMC_mb {
                    return mb.push(log);
                }
            }

            // If it does not, then make a new one
            let mut mb = MultiBoard::new();

            if self.golden_samples.contains(&log.DMC_mb) {
                mb.set_gs();
            }

            let rv = mb.push(log);
            self.multiboards.push(mb);
            rv
        }
    }

    pub fn update(&mut self) {
        println!("INFO: Update started...");
        let mut mbres: Vec<(Yield, Yield, Yield)> = Vec::new();

        self.pp_multiboard = 1;
        self.mb_first_yield = Yield(0, 0);
        self.mb_final_yield = Yield(0, 0);
        self.mb_total_yield = Yield(0, 0);

        for b in self.multiboards.iter_mut() {
            mbres.push(b.update());

            if self.pp_multiboard < b.boards.len() {
                self.pp_multiboard = b.boards.len();
            }

            for result in &b.results {
                if result.result == BResult::Pass {
                    self.mb_total_yield.0 += 1;
                } else if result.result == BResult::Fail {
                    self.mb_total_yield.1 += 1;
                }
            }

            if let Some(x) = b.results.first() {
                if x.result == BResult::Pass {
                    self.mb_first_yield.0 += 1;
                } else if x.result == BResult::Fail {
                    self.mb_first_yield.1 += 1;
                }
            }

            if let Some(x) = b.results.last() {
                if x.result == BResult::Pass {
                    self.mb_final_yield.0 += 1;
                } else if x.result == BResult::Fail {
                    self.mb_final_yield.1 += 1;
                }
            }
        }

        self.sb_first_yield = Yield(0, 0);
        self.sb_final_yield = Yield(0, 0);
        self.sb_total_yield = Yield(0, 0);

        for b in mbres {
            self.sb_first_yield += b.0;
            self.sb_final_yield += b.1;
            self.sb_total_yield += b.2;
        }

        println!(
            "INFO: Update done! Result: {:?} - {:?} - {:?}",
            self.sb_first_yield, self.sb_final_yield, self.sb_total_yield
        );
        println!(
            "INFO: Update done! Result: {:?} - {:?} - {:?}",
            self.mb_first_yield, self.mb_final_yield, self.mb_total_yield
        );
    }

    pub fn clear(&mut self) {
        //self.pp_multiboard = 0;
        self.product_id.clear();
        self.product = None;
        self.golden_samples.clear();
        self.testlist.clear();
        self.multiboards.clear();
        self.sourcelist.clear();
    }

    pub fn get_yields(&self) -> [Yield; 3] {
        [
            self.sb_first_yield,
            self.sb_final_yield,
            self.sb_total_yield,
        ]
    }

    pub fn get_mb_yields(&self) -> [Yield; 3] {
        [
            self.mb_first_yield,
            self.mb_final_yield,
            self.mb_total_yield,
        ]
    }

    pub fn get_testlist(&self) -> &Vec<TList> {
        &self.testlist
    }

    // (DMC, time, result, failed test list)
    pub fn get_failed_boards(&self) -> Vec<(String, u64, BResult, Vec<String>)> {
        let mut ret = Vec::new();

        for mb in &self.multiboards {
            for board in &mb.boards {
                if !board.all_ok() {
                    for log in &board.logs {
                        let failed_ids = log.get_failed_test_list();
                        let mut failed_tests = Vec::new();
                        for fail in failed_ids {
                            if let Some(x) = self.testlist.get(fail) {
                                failed_tests.push(x.0.clone());
                            }
                        }
                        ret.push((board.DMC.clone(), log.time_e, log.result, failed_tests));
                    }
                }
            }
        }

        ret
    }

    pub fn get_failures(&self, setting: FlSettings) -> Vec<FailureList> {
        let mut failure_list: Vec<FailureList> = Vec::new();

        for mb in &self.multiboards {
            'failfor: for failure in mb.get_failures(setting) {
                // Check if already present
                for fl in &mut failure_list {
                    if fl.test_id == failure.0 {
                        fl.total += 1;
                        fl.failed.push((failure.2, failure.3));
                        fl.by_index[failure.1 - 1] += 1;
                        continue 'failfor;
                    }
                }
                // If not make a new one
                let mut new_fail = FailureList {
                    test_id: failure.0,
                    name: self.testlist[failure.0].0.clone(),
                    total: 1,
                    failed: vec![(failure.2, failure.3)],
                    by_index: vec![0; self.pp_multiboard],
                };

                new_fail.by_index[failure.1 - 1] += 1;
                failure_list.push(new_fail);
            }
        }

        failure_list.sort_by_key(|k| k.total);
        failure_list.reverse();

        /*for fail in &failure_list {
            println!("Test no {}, named {} failed {} times.", fail.test_id, fail.name, fail.total);
        } */

        failure_list
    }

    pub fn get_hourly_mb_stats(&self) -> Vec<HourlyStats> {
        // Vec<(time in yymmddhh, total ok, total nok, Vec<(result, mmss)> )>
        // Time is in format 231222154801 by default YYMMDDHHMMSS
        // We don't care about the last 4 digit, so we can div by 10^4

        let mut ret: Vec<HourlyStats> = Vec::new();

        for mb in &self.multiboards {
            'resfor: for res in &mb.results {
                let time = res.end / u64::pow(10, 4);
                let time_2 = res.end % u64::pow(10, 4);

                //println!("{} - {} - {}", res.0, time, time_2);

                // check if a entry for "time" exists
                for r in &mut ret {
                    if r.0 == time {
                        if res.result == BResult::Pass {
                            r.1.panels_with_gs.0 += 1;
                            r.1.boards_with_gs.0 += self.pp_multiboard as u16;

                            if !mb.golden_sample {
                                r.1.panels.0 += 1;
                                r.1.boards.0 += self.pp_multiboard as u16;
                            }
                        } else {
                            let failed_boards =
                                res.panels.iter().filter(|f| **f == BResult::Fail).count() as u16;

                            r.1.panels_with_gs.1 += 1;
                            r.1.boards_with_gs.1 += failed_boards;

                            if !mb.golden_sample {
                                r.1.panels.1 += 1;
                                r.1.boards.1 += failed_boards;
                            }
                        }

                        r.2.push((res.result, time_2, mb.DMC.clone(), mb.golden_sample));

                        continue 'resfor;
                    }
                }

                let mut hourly = HourlyYield::default();
                if res.result == BResult::Pass {
                    hourly.panels_with_gs.0 += 1;
                    hourly.boards_with_gs.0 += self.pp_multiboard as u16;

                    if !mb.golden_sample {
                        hourly.panels.0 += 1;
                        hourly.boards.0 += self.pp_multiboard as u16;
                    }
                } else {
                    let failed_boards =
                        res.panels.iter().filter(|f| **f == BResult::Fail).count() as u16;

                    hourly.panels_with_gs.1 += 1;
                    hourly.boards_with_gs.1 += failed_boards;

                    if !mb.golden_sample {
                        hourly.panels.1 += 1;
                        hourly.boards.1 += failed_boards;
                    }
                }

                ret.push((
                    time,
                    hourly,
                    vec![(res.result, time_2, mb.DMC.clone(), mb.golden_sample)],
                ));
            }
        }

        ret.sort_by_key(|k| k.0);

        for r in &mut ret {
            r.2.sort_by_key(|k| k.1);
        }

        ret
    }

    // Returns the result of eaxh mb. Format: (DMC, Vec<(test_time, mb_result, Vec<board_result>)>)
    pub fn get_mb_results(&self) -> Vec<MbStats> {
        let mut ret: Vec<MbStats> = Vec::new();

        for mb in &self.multiboards {
            ret.push((mb.DMC.clone(), mb.get_results().clone(), mb.golden_sample));
        }

        ret.sort_by_key(|k| k.1.last().unwrap().start);
        ret
    }

    // Get the measurments for test "testid". (TType,Vec<(time, index, result, limits)>) The Vec is sorted by time.
    // Could pass the DMC too
    pub fn get_stats_for_test(&self, testid: usize) -> (TType, Vec<(u64, usize, TResult, TLimit)>) {
        let mut resultlist: Vec<(u64, usize, TResult, TLimit)> = Vec::new();

        if testid > self.testlist.len() {
            println!(
                "ERR: Test ID is out of bounds! {} > {}",
                testid,
                self.testlist.len()
            );
            return (TType::Unknown, resultlist);
        }

        for mb in &self.multiboards {
            resultlist.append(&mut mb.get_stats_for_test(testid));
        }

        resultlist.sort_by_key(|k| k.0);

        for res in resultlist.iter_mut() {
            res.0 = NaiveDateTime::parse_from_str(&format!("{}", res.0), "%y%m%d%H%M%S")
                .unwrap()
                .and_utc()
                .timestamp() as u64;
        }

        (self.testlist[testid].1, resultlist)
    }

    pub fn get_tests_w_limit_changes(&self) -> Option<Vec<(usize, String)>> {
        let mut ret: Vec<(usize, String)> = Vec::new();

        'outerloop: for (i, (tname, ttype)) in self.testlist.iter().enumerate() {
            match ttype {
                // These tests have no "limit" by default, skip them
                TType::BoundaryS => continue,
                TType::Digital => continue,
                TType::Pin => continue,
                TType::Shorts => continue,
                TType::Testjet => continue,
                TType::Unknown => {
                    //println!("ERR: TType::Unknown in the final testlist at #{i}, name {tname}");
                }
                _ => {
                    let mut limit: Option<&TLimit> = None;
                    for mb in &self.multiboards {
                        for sb in &mb.boards {
                            for log in &sb.logs {
                                if let Some(test) = log.limits.get(i) {
                                    if *test == TLimit::None {
                                        continue;
                                    }
                                    if limit.is_none() {
                                        limit = Some(test)
                                    } else if *limit.unwrap() != *test {
                                        println!(
                                            "INFO: Test {tname} has limit changes in the sample"
                                        );
                                        ret.push((i, tname.clone()));
                                        continue 'outerloop;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if ret.is_empty() {
            None
        } else {
            Some(ret)
        }
    }
    // We don't check here if the limits have changed.
    // Can't properly show than in a spreadsheet anyway.
    // We will notify the user about it in the UI only.
    fn generate_limit_list(&self) -> Option<Vec<TLimit>> {
        let mut ret: Vec<TLimit> = Vec::new();

        'outerloop: for i in 0..self.testlist.len() {
            for mb in &self.multiboards {
                for sb in &mb.boards {
                    for log in &sb.logs {
                        if let Some(limit) = log.limits.get(i) {
                            if log.results[i].0 != BResult::Unknown {
                                ret.push(*limit);
                                continue 'outerloop;
                            }
                        }
                    }
                }
            }

            // if none of the boards has a result for the test, then push None
            ret.push(TLimit::None);
        }

        if ret.is_empty() {
            None
        } else {
            Some(ret)
        }
    }

    fn get_export_list(&self, settings: &ExportSettings) -> Vec<usize> {
        let mut ret: Vec<usize> = Vec::new();

        match settings.mode {
            ExportMode::All => {
                ret = (0..self.testlist.len()).collect();
            }
            ExportMode::FailuresOnly => {
                for id in self.get_failures(FlSettings::All) {
                    ret.push(id.test_id);
                }
            }
            ExportMode::Manual => {
                for part in settings.list.split(' ') {
                    for (i, (t, _)) in self.testlist.iter().enumerate() {
                        if *t == part {
                            ret.push(i);
                            break;
                        }
                    }
                }
            }
        }

        ret
    }

    pub fn export(&self, path: PathBuf, settings: &ExportSettings) {
        let mut book = rust_xlsxwriter::Workbook::new();
        let sheet = book.add_worksheet();
        let sci_format = rust_xlsxwriter::Format::new().set_num_format("0.00E+00");

        if settings.vertical {
            // Create header
            let _ = sheet.write(0, 0, &self.product_id);
            let _ = sheet.write(2, 0, "DMC");
            let _ = sheet.set_column_width(0, 32);
            let _ = sheet.write(2, 1, "Test time");
            let _ = sheet.set_column_width(1, 18);
            let _ = sheet.write(2, 2, "Log result");
            let _ = sheet.write(0, 2, "Test name:");
            let _ = sheet.write(1, 2, "Test limits:");
            let _ = sheet.set_column_width(2, 10);

            // Generate list of teststeps to be exported
            let export_list = self.get_export_list(settings);

            // Print testlist
            for (i, t) in export_list.iter().enumerate() {
                let c: u16 = (i * 2 + 3).try_into().unwrap();
                let _ = sheet.write_with_format(
                    0,
                    c,
                    &self.testlist[*t].0,
                    &rust_xlsxwriter::Format::new().set_text_wrap(),
                );
                let _ = sheet.set_column_width(c, 16);
                let _ = sheet.write(0, c + 1, self.testlist[*t].1.print());
                let _ = sheet.set_column_width(c + 1, 10);
                let _ = sheet.write(2, c, "Result");
                let _ = sheet.write(2, c + 1, "Value");
            }

            // Print limits. Nominal value is skiped.
            // It does not check if the limit changed.
            if let Some(limits) = self.generate_limit_list() {
                for (i, t) in export_list.iter().enumerate() {
                    let c: u16 = (i * 2 + 3).try_into().unwrap();
                    // Lim2 (f32,f32),     // UL - LL
                    // Lim3 (f32,f32,f32)  // Nom - UL - LL
                    match limits[*t] {
                        TLimit::Lim3(_, ul, ll) => {
                            let _ = sheet.write_number_with_format(1, c, ll, &sci_format);
                            let _ = sheet.write_number_with_format(1, c + 1, ul, &sci_format);
                        }
                        TLimit::Lim2(ul, ll) => {
                            let _ = sheet.write_number_with_format(1, c, ll, &sci_format);
                            let _ = sheet.write_number_with_format(1, c + 1, ul, &sci_format);
                        }
                        TLimit::None => {}
                    }
                }
            }

            // Print test results
            let mut l: u32 = 3;
            for mb in &self.multiboards {
                for b in &mb.boards {
                    l = b.export_to_line(
                        sheet,
                        l,
                        settings.only_failed_panels,
                        settings.only_final_logs,
                        &export_list,
                        &sci_format,
                    );
                }
            }
        } else {
            // Create header
            let _ = sheet.write(0, 0, &self.product_id);
            let _ = sheet.write(2, 0, "Test name");
            let _ = sheet.set_column_width(0, 22);

            let _ = sheet.write(2, 1, "Test type");
            let _ = sheet.set_column_width(4, 16);

            let _ = sheet.write(1, 3, "Test limits");

            let _ = sheet.write(2, 2, "MIN");
            let _ = sheet.set_column_width(2, 10);
            let _ = sheet.write(2, 3, "Nom");
            let _ = sheet.set_column_width(3, 10);
            let _ = sheet.write(2, 4, "MAX");
            let _ = sheet.set_column_width(4, 10);

            // Generate list of teststeps to be exported
            let export_list = self.get_export_list(settings);

            // Print testlist
            for (i, t) in export_list.iter().enumerate() {
                let l: u32 = (i + 3).try_into().unwrap();
                let _ = sheet.write(l, 0, &self.testlist[*t].0);
                let _ = sheet.write(l, 1, &self.testlist[*t].1.print());
            }

            // Print limits
            // It does not check if the limit changed.
            if let Some(limits) = self.generate_limit_list() {
                for (i, t) in export_list.iter().enumerate() {
                    let l: u32 = (i + 3).try_into().unwrap();
                    // Lim2 (f32,f32),     // UL - LL
                    // Lim3 (f32,f32,f32)  // Nom - UL - LL
                    match limits[*t] {
                        TLimit::Lim3(nom, ul, ll) => {
                            let _ = sheet.write_number_with_format(l, 2, ll, &sci_format);
                            let _ = sheet.write_number_with_format(l, 3, nom, &sci_format);
                            let _ = sheet.write_number_with_format(l, 4, ul, &sci_format);
                        }
                        TLimit::Lim2(ul, ll) => {
                            let _ = sheet.write_number_with_format(l, 2, ll, &sci_format);
                            let _ = sheet.write_number_with_format(l, 4, ul, &sci_format);
                        }
                        TLimit::None => {}
                    }
                }
            }

            // Print test results
            let mut c: u16 = 5;
            for mb in &self.multiboards {
                for b in &mb.boards {
                    c = b.export_to_col(
                        sheet,
                        c,
                        settings.only_failed_panels,
                        settings.only_final_logs,
                        &export_list,
                        &sci_format,
                    );
                }
            }
        }

        let _ = book.save(path);
    }

    fn get_mb_w_DMC(&self, DMC: &str) -> Option<&MultiBoard> {
        for mb in self.multiboards.iter() {
            for sb in &mb.boards {
                if sb.DMC == DMC {
                    return Some(mb);
                }
            }
        }

        println!("Found none as {DMC}");
        None
    }

    fn get_sb_w_DMC(&self, DMC: &str) -> Option<&Board> {
        for mb in self.multiboards.iter() {
            for sb in &mb.boards {
                if sb.DMC == DMC {
                    return Some(sb);
                }
            }
        }

        println!("Found none as {DMC}");
        None
    }

    /*pub fn get_report_for_MB(&self, DMC: &str) -> Option<Vec<String>> {
        if let Some(board) = self.get_mb_w_DMC(DMC) {
            return Some(board.get_reports());
        }

        None
    }*/

    pub fn get_report_for_SB(&self, DMC: &str) -> Option<String> {
        if let Some(board) = self.get_sb_w_DMC(DMC) {
            return Some(board.get_reports());
        }

        None
    }

    pub fn get_report_for_SB_w_index(&self, DMC: &str, index: usize) -> Option<String> {
        if let Some(mb) = self.get_mb_w_DMC(DMC) {
            if let Some(board) = mb.boards.get(index) {
                return Some(board.get_reports());
            }
        }

        None
    }

    pub fn get_report_for_SB_NOK(&self, DMC: &str) -> Option<String> {
        if let Some(mb) = self.get_mb_w_DMC(DMC) {
            for sb in mb.boards.iter() {
                if !sb.all_ok() {
                    return Some(sb.get_reports());
                }
            }
        }

        None
    }

    pub fn get_product_id(&self) -> String {
        self.product_id.clone()
    }
}
