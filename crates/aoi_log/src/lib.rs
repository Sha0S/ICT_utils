#![allow(non_snake_case)]

use anyhow::{bail, Result};
use chrono::NaiveDateTime;
use log::{debug, error};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub mod helpers;
mod test;

macro_rules! error_and_bail {
    ($($arg:tt)+) => {
        error!($($arg)+);
        bail!($($arg)+);
    };
}

#[derive(Debug, Default, Clone)]
pub struct Panel {
    pub inspection_plan: String,
    pub variant: String,
    pub station: String,

    pub inspection_date_time: Option<NaiveDateTime>,
    pub repair: Option<Repair>,

    pub boards: Vec<Board>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Repair {
    pub date_time: NaiveDateTime,
    pub operator: String,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Board {
    pub barcode: String,
    pub result: bool, // true - pass, false - failed
    pub windows: Vec<Window>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub id: String,
    pub win_type: String,

    // might have to use the MacroName String
    pub analysis_mode: String,
    pub analysis_sub_mode: String,

    pub result: WindowResult,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum WindowResult {
    Pass,
    #[default]
    Fail,
    PseudoError,
    Unknown,
}

impl Panel {
    pub fn load_xml<P: AsRef<Path> + std::fmt::Debug>(path: P) -> Result<Self> {
        debug!("Processing XML: {path:?}");

        let mut ret = Panel::default();

        let file = std::fs::read_to_string(path)?;
        let xml = roxmltree::Document::parse(&file)?;

        let root = xml.root_element();
        let mut has_failed_boards = false;

        if let Some(ginfo) = root
            .children()
            .find(|f| f.has_tag_name("GlobalInformation"))
        {
            for sub_child in ginfo.children().filter(|f| f.is_element()) {
                match sub_child.tag_name().name() {
                    "Station" => {
                        if let Some(x) = sub_child.children().find(|f| f.has_tag_name("Name")) {
                            ret.station = x.text().unwrap_or_default().to_owned();
                            debug!("Station: {}", ret.station);
                        }
                    }
                    "Program" => {
                        if let Some(x) = sub_child
                            .children()
                            .find(|f| f.has_tag_name("InspectionPlanName"))
                        {
                            ret.inspection_plan = x.text().unwrap_or_default().to_owned();
                            debug!("InspectionPlan: {}", ret.inspection_plan);
                        }

                        if let Some(x) =
                            sub_child.children().find(|f| f.has_tag_name("VariantName"))
                        {
                            ret.variant = x.text().unwrap_or_default().to_owned();
                            debug!("Variant: {}", ret.variant);
                        }
                    }
                    "Inspection" => {
                        let date = if let Some(x) =
                            sub_child.children().find(|f| f.has_tag_name("Date"))
                        {
                            if let Some(y) = x.children().find(|f| f.has_tag_name("End")) {
                                y.text().unwrap_or_default()
                            } else {
                                error_and_bail!("Inspection block has no Date -> End field!");
                            }
                        } else {
                            error_and_bail!("Inspection block has no Date block!");
                        };
                        let time = if let Some(x) =
                            sub_child.children().find(|f| f.has_tag_name("Time"))
                        {
                            if let Some(y) = x.children().find(|f| f.has_tag_name("End")) {
                                y.text().unwrap_or_default()
                            } else {
                                error_and_bail!("Inspection block has no Time -> End field!");
                            }
                        } else {
                            error_and_bail!("Inspection block has no Time block!");
                        };

                        if !date.is_empty() && !time.is_empty() {
                            let t = format!("{date} {time}");
                            debug!("Raw time string: {t}");
                            ret.inspection_date_time =
                                Some(NaiveDateTime::parse_from_str(&t, "%Y%m%d %H%M%S")?);
                        }
                    }
                    "Repair" => {
                        let operator = if let Some(x) = sub_child
                            .children()
                            .find(|f| f.has_tag_name("OperatorName"))
                        {
                            x.text().unwrap_or_default().to_uppercase()
                        } else {
                            error_and_bail!("Repair block has no operator field!");
                        };

                        let date = if let Some(x) =
                            sub_child.children().find(|f| f.has_tag_name("Date"))
                        {
                            if let Some(y) = x.children().find(|f| f.has_tag_name("End")) {
                                y.text().unwrap_or_default()
                            } else {
                                error_and_bail!("Repair block has no Date -> End field!");
                            }
                        } else {
                            error_and_bail!("Repair block has no Date block!");
                        };
                        let time = if let Some(x) =
                            sub_child.children().find(|f| f.has_tag_name("Time"))
                        {
                            if let Some(y) = x.children().find(|f| f.has_tag_name("End")) {
                                y.text().unwrap_or_default()
                            } else {
                                error_and_bail!("Repair block has no Time -> End field!");
                            }
                        } else {
                            error_and_bail!("Repair block has no Time block!");
                        };

                        if !date.is_empty() && !time.is_empty() {
                            let t = format!("{date} {time}");
                            debug!("Raw time string: {t}");
                            let date = NaiveDateTime::parse_from_str(&t, "%Y%m%d %H%M%S")?;

                            ret.repair = Some(Repair {
                                date_time: date,
                                operator,
                            });
                        }
                    }
                    _ => (),
                }
            }
        } else {
            error_and_bail!("Could not find <GlobalInformation>!");
        }

        if ret.station.is_empty()
            || ret.inspection_plan.is_empty()
            || ret.inspection_date_time.is_none()
        {
            error_and_bail!("Missing mandatory <GlobalInformation> fields!");
        }

        // Iterating over <PCBInformation>, creating a Board for each <SinglePCB>

        if let Some(pcb_info) = root.children().find(|f| f.has_tag_name("PCBInformation")) {
            let count = pcb_info.children().filter(|f| f.is_element()).count();
            debug!("PCB count: {}", count);

            if count == 0 {
                error_and_bail!("PCBInformation has no sub-blocks!");
            }

            ret.boards = vec![Board::default(); count];

            for (i, child) in pcb_info
                .children()
                .filter(|f| f.tag_name().name() == "SinglePCB")
                .enumerate()
            {
                let mut serial = String::new();
                let mut result = String::new();

                for sub_child in child.children().filter(|f| f.is_element()) {
                    match sub_child.tag_name().name() {
                        "Barcode" => {
                            serial = sub_child.text().unwrap_or_default().to_owned();
                        }
                        "Result" => {
                            result = sub_child.text().unwrap_or_default().to_owned();
                        }
                        _ => {}
                    }
                }

                debug!("{i}: serial: {serial}, result: {result}");
                if !serial.is_empty() && !result.is_empty() {
                    if result != "PASS" {
                        has_failed_boards = true;
                    }
                    ret.boards[i].barcode = serial;
                    ret.boards[i].result = result == "PASS";
                } else {
                    error_and_bail!("SinglePCB sub-fields missing!");
                }
            }
        } else {
            error_and_bail!("Could not find <PCBInformation>!");
        }

        // Iterating over <ComponentInformation>, searching for failed Windows

        // Can skip this if the XML is for Inspection only, and all the PCB passed.
        if ret.repair.is_some() || has_failed_boards {
            debug!("Searching for failed Windows");
            if let Some(comp_info) = root
                .children()
                .find(|f| f.has_tag_name("ComponentInformation"))
            {
                for window in comp_info.children().filter(|f| f.is_element()) {
                    let mut win_id = String::new();
                    let mut win_type = String::new();
                    let mut pcb_number: usize = 0;
                    let mut result: WindowResult = WindowResult::Unknown;
                    let mut mode = String::new();
                    let mut sub_mode = String::new();

                    for sub_child in window.children().filter(|f| f.is_element()) {
                        match sub_child.tag_name().name() {
                            "WinID" => {
                                win_id = sub_child.text().unwrap_or_default().to_string();
                            }
                            "WinType" => {
                                win_type = sub_child.text().unwrap_or_default().to_string();
                            }
                            "PCBNumber" => {
                                pcb_number = sub_child.text().unwrap_or_default().parse()?;
                            }

                            // <MacroName> is Inspection station only, can use it to get Mode and Submode
                            // example:
                            //      - IRISO_9860B-40Z14_MENI_17_0 -> Mode: MENI, SubMode: 17
                            //      - R0402_3D_GENR_30_15 -> Mode: GENR, SubMode: 30
                            "MacroName" => {
                                let macro_text = sub_child.text().unwrap_or_default().to_string();
                                let mut macro_split = macro_text.split('_');

                                if let Some(sm) = macro_split.nth_back(1) {
                                    sub_mode = sm.to_owned();
                                } else {
                                    error_and_bail!(
                                        "Failed to get sub mode form MacroName: {}",
                                        macro_text
                                    );
                                }

                                if let Some(sm) = macro_split.next_back() {
                                    mode = sm.to_owned();
                                } else {
                                    error_and_bail!(
                                        "Failed to get mode form MacroName: {}",
                                        macro_text
                                    );
                                }
                            }

                            // <Result> block only exists on XMLs from the repair station
                            // Type = 2 is pseudo error, everything else is fail
                            "Result" => {
                                if let Some(t) =
                                    sub_child.children().find(|f| f.has_tag_name("Type"))
                                {
                                    let result_text = t.text().unwrap_or_default().to_string();
                                    if result_text == "2" {
                                        result = WindowResult::PseudoError
                                    } else {
                                        result = WindowResult::Fail
                                    }
                                }
                            }

                            // <Analysis> blocks contents change depending if it is from Inspection or Repair
                            // Repair: Mode and Submode are vaild, and used
                            // Inspection:
                            //      - Mode cannot be used, will have to read it from MacroName
                            //      - Submode is usable, but it can also be read from MacroName
                            //      - Result: 0 -> pass, anything else is fail
                            "Analysis" => {
                                if ret.repair.is_some() {
                                    for analysis_child in
                                        sub_child.children().filter(|f| f.is_element())
                                    {
                                        match analysis_child.tag_name().name() {
                                            "Mode" => {
                                                mode = analysis_child
                                                    .text()
                                                    .unwrap_or_default()
                                                    .to_string();
                                            }
                                            "SubMode" => {
                                                sub_mode = analysis_child
                                                    .text()
                                                    .unwrap_or_default()
                                                    .to_string();
                                            }
                                            _ => {}
                                        }
                                    }
                                } else if let Some(t) =
                                    sub_child.children().find(|f| f.has_tag_name("Result"))
                                {
                                    let result_text = t.text().unwrap_or_default().to_string();
                                    if result_text == "0" {
                                        result = WindowResult::Pass
                                    } else {
                                        result = WindowResult::Fail
                                    }
                                }
                            }
                            _ => {}
                        }
                    }

                    if result == WindowResult::Pass {
                        continue;
                    } else if result == WindowResult::Unknown {
                        error_and_bail!("Window result is Unknown!");
                    }

                    if win_id.is_empty()
                        || win_type.is_empty()
                        || mode.is_empty()
                        || sub_mode.is_empty()
                        || pcb_number == 0
                    {
                        error_and_bail!("Mandatory fields for Window are missing!");
                    }

                    debug!("Found failed window: PCB#{pcb_number}: {win_id}");
                    // pcb_number is base 1
                    pcb_number -= 1;

                    if let Some(board) = ret.boards.get_mut(pcb_number) {
                        board.windows.push(Window {
                            id: win_id,
                            win_type,
                            analysis_mode: mode,
                            analysis_sub_mode: sub_mode,
                            result,
                        });
                    } else {
                        error_and_bail!("Failed to get board number: {}", pcb_number);
                    }
                }
            }
        }

        debug!("Processing OK!");

        Ok(ret)
    }
}
