use std::path::Path;

use anyhow::bail;
use log::{debug, error, info};

use crate::{BResult, LogFile, TLimit, TType, Test};

impl LogFile {
    pub fn load_DCDC_FCT(p: &Path) -> anyhow::Result<Self> {
        info!("Loading DCDC FCT file {}", p.display());

        let source = p.as_os_str().to_owned();

        let mut DMC = "NoDMC".to_string();
        let mut product_id = "NoID".to_string();
        let mut result = false;
        let mut date = None;
        //let mut nest = 1;

        let mut tests = Vec::new();

        let file = std::fs::read_to_string(p)?;

        for line in file.lines() {
            if line.starts_with('*') {
                continue;
            }

            let tags: Vec<String> = line.split('|').map(|f| f.trim().to_string()).collect();

            // Header
            if tags.len() == 2 {
                match tags[0].as_str() {
                    "Tracking Number" => {
                        DMC = tags[1].clone();
                    }
                    "Product" => {
                        product_id = tags[1].clone();
                    }
                    "Test Result" => {
                        result = if tags[1] == "OK" { true } else { false };
                    }
                    "Date" => {
                        date = Some(chrono::NaiveDateTime::parse_from_str(
                            &tags[1],
                            "%d/%m/%y  %H:%M:%S",
                        ));
                    }
                    /*"Nest" => {
                        nest = tags[1].parse::<usize>().unwrap_or(1);
                    }*/
                    _ => {}
                }
            } else if tags.len() == 8 {
                if tags[3].is_empty() {
                    continue;
                }

                let mut div = 1.0;

                let ttype = match tags[3].as_str() {
                    "S" => TType::Time,
                    "V" => TType::Measurement,
                    "mV" => {
                        div = 1000.0;
                        TType::Measurement
                    }
                    "10mV" => {
                        div = 100.0;
                        TType::Measurement
                    }
                    "A" => TType::Current,
                    "Hz" => TType::Frequency,
                    "dec" | "bit" => TType::Unknown,
                    _ => {
                        debug!("Unknown ttype: {}", tags[3]);
                        TType::Unknown
                    }
                };

                let value = if let Ok(v) = tags[2].parse::<f32>() {
                    v / div
                } else {
                    error!("Could not parse value: {}", tags[2]);
                    continue;
                };

                let res = if tags[6] == "OK" {
                    BResult::Pass
                } else {
                    BResult::Fail
                };

                let limits = if tags[4].is_empty() || tags[5].is_empty() {
                    TLimit::None
                } else {
                    let min = tags[4].parse::<f32>();
                    let max = tags[5].parse::<f32>();

                    if min.is_err() || max.is_err() {
                        error!("Could not parse limits! {} - {}", tags[4], tags[5]);
                        TLimit::None
                    } else {
                        TLimit::Lim2(max.unwrap() / div, min.unwrap() / div)
                    }
                };

                tests.push(Test {
                    name: tags[1].to_string(),
                    ttype,
                    result: (res, value),
                    limits,
                });
            } else {
                debug!("Tag length invalid: {}\n\t{}", tags.len(), line);
            }
        }

        if date.is_none() {
            bail!("DateTime is missing!");
        }
        let date = date.unwrap();

        if date.is_err() {
            bail!("DateTime parsing error!");
        }
        let date = date.unwrap();

        Ok(Self {
            source,
            DMC: DMC.clone(),
            DMC_mb: DMC,
            product_id,
            index: 1,
            result,
            status: if result { 0 } else { 1 },
            status_str: "".to_string(),
            time_start: date,
            time_end: date,
            tests,
            report: String::new(),
            SW_version: String::new(),
            log_type: crate::LogFileType::FCT_DCDC,
            mes_enabled: true,
        })
    }
}
