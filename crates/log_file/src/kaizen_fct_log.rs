use std::path::Path;

use anyhow::bail;
use log::{debug, error, info};

use crate::{BResult, LogFile, LogFileType, TLimit, TType, Test};

impl LogFile {
    pub fn load_Kaizen_FCT(p: &Path) -> anyhow::Result<Self> {
        info!("Loading FCT file {}", p.display());
        let source = p.as_os_str().to_owned();

        let file_ANSI = std::fs::read(p)?;
        let decoded = encoding_rs::WINDOWS_1252.decode(&file_ANSI);

        if decoded.2 {
            error!("Conversion had errors");
        }

        let lines = decoded.0.lines();

        let mut DMC = None;
        //let mut DMC_mb = None;
        //let mut product_id = None;
        let mut SW_version = String::new();
        let mut result = None;
        let mut status = None;

        let mut time_start = None;

        let mut testing_time: u64 = 0;

        let mut mes_enabled = false;

        let mut tests = Vec::new();

        for line in lines {
            let tokens: Vec<&str> = line.split(';').collect();
            if tokens.len() < 2 {
                //println!("ERROR: To few tokens! ({})", line);
                continue;
            }

            match tokens[0] {
                "SerialNumber" => DMC = Some(tokens[1].to_string()),
                /*"MainSerial" => DMC_mb = Some(tokens[1].to_string()),*/
                "Part Type" => {
                    SW_version = tokens[1].to_string();
                }
                "MES" => {
                    if tokens[1] == "1" {
                        mes_enabled = true;
                    }
                }
                "Start Time" => {
                    if let Ok(time) =
                        chrono::NaiveDateTime::parse_from_str(tokens[1], "%Y.%m.%d. %H:%M")
                    {
                        time_start = Some(time);
                    } else {
                        error!("Time conversion error!");
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
                        debug!("Tokens: {tokens:?}");
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

                                TLimit::Lim2(max, min)
                            } else {
                                TLimit::None
                            }
                        } else {
                            TLimit::None
                        };

                        let result = (
                            if tokens[5] == "Passed" || tokens[5] == "Info" {
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
            bail!("Logfile conatined no tests!",);
        }

        if time_start.is_none() {
            bail!("Logfile conatined no start time!",);
        }

        let time_start = time_start.unwrap();
        let time_end = time_start + std::time::Duration::from_secs(testing_time);

        // Generate report text for failed boards
        let result = result.is_some_and(|f| f == "Passed");
        let mut report = String::new();
        if !result {
            let mut lines = Vec::new();
            for test in &tests {
                if test.result.0 != BResult::Pass {
                    lines.push(format!("{} HAS FAILED", test.name));
                    lines.push(format!("Measured: {:+1.4E}", test.result.1));

                    if let TLimit::Lim2(ul, ll) = test.limits {
                        lines.push(format!("High Limit: {:+1.4E}", ul));
                        lines.push(format!("Low Limit: {:+1.4E}", ll));
                    }

                    if test.ttype != TType::Unknown {
                        lines.push(format!(
                            "{} test with unit {}",
                            test.ttype.print(),
                            test.ttype.unit()
                        ));
                    }

                    lines.push("\n----------------------------------------\n".to_string());
                }
            }

            report = lines.join("\n");
        }

        let result = LogFile {
            source,
            DMC: DMC.clone().unwrap_or_default(),
            DMC_mb: DMC.unwrap_or_default(), //DMC_mb.unwrap_or_default(),
            product_id: "Kaized CMD".to_string(), //product_id.unwrap_or_default(),
            index: 1,
            result,
            status: status.unwrap_or_default(),
            status_str: String::new(),
            time_start,
            time_end,
            tests,
            report,
            SW_version,
            log_type: LogFileType::FCT_Kaizen,
            mes_enabled,
        };

        //println!("Result: {result:?}");

        Ok(result)
    }
}
