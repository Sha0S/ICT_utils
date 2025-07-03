use log::info;
use std::path::Path;

use crate::{BResult, LogFile, TLimit, TType, Test};

impl LogFile {
    pub fn load_SPI_log(p: &Path) -> anyhow::Result<Vec<Self>> {
        info!("Loading SPi log file {}", p.display());

        let source = p.as_os_str().to_owned();

        let panel = SPI_log_file::Panel::load(p, false)?;

        let mut ret = Vec::new();

        for board in panel.boards {
            let mut tests = Vec::new();

            for comp in board.components {
                for pad in comp.pads {
                    for feature in pad.features {
                        // solderbridge has for example no "value", make a dummy test for it
                        if feature.values.is_empty() {
                            tests.push(Test {
                                name: format!("{}.{} - {}", comp.name, pad.name, feature.name),
                                ttype: TType::Unknown,
                                msg: String::new(),
                                result: if feature.inspection_failed {
                                    (BResult::Fail, 1.0)
                                } else {
                                    (BResult::Pass, 0.0)
                                },
                                limits: TLimit::None,
                            });
                        }

                        for value in feature.values {
                            if let Some(th) = value.thresholds {
                                tests.push(Test {
                                    name: format!("{}.{} - {}", comp.name, pad.name, value.name),
                                    ttype: TType::Unknown,
                                    msg: String::new(),
                                    result: (
                                        if feature.inspection_failed {
                                            BResult::Fail
                                        } else {
                                            BResult::Pass
                                        },
                                        value.value,
                                    ),
                                    limits: TLimit::Lim2(th.1 as f32, th.0 as f32),
                                });
                            }
                        }
                    }
                }
            }

            ret.push(LogFile {
                source: source.clone(),
                DMC: board.barcode,
                DMC_mb: panel.barcode.clone(),
                product_id: panel.product.clone(),
                index: board.name.parse().unwrap(),
                nest: None,
                result: !board.inspection_failed, // board.is_failed?
                status: if board.inspection_failed { 1 } else { 0 },
                status_str: String::new(),
                time_start: panel.inspection_start,
                time_end: panel.inspection_end,
                tests,
                report: String::new(),
                SW_version: String::new(),
                log_type: crate::LogFileType::SPI,
                mes_enabled: true,
            });
        }

        Ok(ret)
    }
}
