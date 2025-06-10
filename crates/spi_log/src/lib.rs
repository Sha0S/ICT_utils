#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

use anyhow::{bail, Context, Result};
use chrono::NaiveDateTime;
use log::{debug, error};
use roxmltree::Node;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub mod helpers;

macro_rules! error_and_bail {
    ($($arg:tt)+) => {
        error!($($arg)+);
        bail!($($arg)+);
    };
}

/*
    xml structure:

    VvExtDataExportXml
        -> DataModel
            -> Inspection
            -> Object ( Class="Panel" )
                -> Status
                    -> Inspection
                    -> Overall ?
                        ! Check behaviour of failed boards
                -> Object ( Class="Comp" Group="Fiducial" ) ?
                    ! Check behaviour if fiducials are not found
                -> Object ( Class="Board" )
                    -> Status
                        -> Inspection
                        -> Overall ?
                    -> Object ( Class="Comp" )
                        -> Status
                            -> Inspection
                            -> Overall ?
                        -> Object ( Class="Solder" )
                            -> Status
                                -> Inspection
                                -> Overall ?
                            -> Features
                                -> Feature
                                    -> Status
                                        -> Inspection
                                        -> Overall ?
                                    -> Values
                                        ! Solder.Bridge feature has no values block
                                        -> Value (Volume, Area, Height, DisplacementX, DisplacementY)
*/

#[derive(Debug, Clone)]
pub struct Panel {
    pub product: String, // DataModel -> Name
    pub variant: String, // DataModel -> Variant
    pub barcode: String, // DataModel -> Barcode

    pub inspection_start: NaiveDateTime, // Inspection -> InspectionStart
    pub inspection_end: NaiveDateTime,   // Inspection -> InspectionEnd
    pub inspection_aborted: bool,        // Inspection -> InspectionAborted

    pub inspection_failed: bool, // Status -> IsInspectionFailed
    pub is_failed: bool,         // Status -> IsFailed

    // Q: fiducials?

    pub boards: Vec<Board>,
}

#[derive(Debug, Clone)]
pub struct Board {
    pub barcode: String, // Object -> Barcode
    pub name: String,    // Object -> Name

    pub inspection_failed: bool, // Status -> IsInspectionFailed
    pub is_failed: bool,         // Status -> IsFailed

    pub components: Vec<Component>,
}

#[derive(Debug, Clone)]
pub struct Component {
    pub name: String,      // Object -> Name
    pub comp_type: String, // Object -> Type

    pub inspection_failed: bool, // Status -> IsInspectionFailed
    pub is_failed: bool,         // Status -> IsFailed

    pub pads: Vec<Pad>,
}

#[derive(Debug, Clone)]
pub struct Pad {
    pub name: String,     // Object -> Name
    pub pad_type: String, // Object -> Type

    pub inspection_failed: bool, // Status -> IsInspectionFailed
    pub is_failed: bool,         // Status -> IsFailed

    pub features: Vec<Feature>,
}

#[derive(Debug, Clone)]
pub struct Feature {
    pub name: String, // Object -> Name

    pub inspection_failed: bool, // Status -> IsInspectionFailed
    pub is_failed: bool,         // Status -> IsFailed

    pub values: Vec<Value>,
}

#[derive(Debug, Clone)]
pub struct Value {
    pub name: String,
    pub value: f32,
    pub unit: String,
    pub thresholds: Option<(i32, i32)>,
}

impl Panel {
    pub fn load<P: AsRef<Path> + std::fmt::Debug>(path: P) -> Result<Self> {
        debug!("Processing XML: {path:?}");

        let file = std::fs::read_to_string(path)?;
        let xml = roxmltree::Document::parse(&file)?;
        let root = xml.root_element();

        let datamodel = root
            .children()
            .find(|f| f.has_tag_name("DataModel"))
            .context("Could not find node 'DataModel'!")?;

        let product = datamodel
            .attribute("Name")
            .context("Missing attribute: 'Name'")?
            .to_string();
        let variant = datamodel
            .attribute("Variant")
            .context("Missing attribute: 'Variant'")?
            .to_string();
        let barcode = datamodel
            .attribute("Barcode")
            .context("Missing attribute: 'Barcode'")?
            .to_string();

        let inspection = datamodel
            .children()
            .find(|f| f.has_tag_name("Inspection"))
            .context("Could not find node 'Inspection' in 'DataModel'!")?;

        let inspection_start = NaiveDateTime::parse_from_str(
            inspection
                .attribute("InspectionStart")
                .context("Missing attribute: 'InspectionStart'")?,
            "%FT%T",
        )?;
        let inspection_end = NaiveDateTime::parse_from_str(
            inspection
                .attribute("InspectionEnd")
                .context("Missing attribute: 'InspectionEnd'")?,
            "%FT%T",
        )?;
        let inspection_aborted = inspection
            .attribute("InspectionAborted")
            .context("Missing attribute: 'InspectionAborted'")?
            == "true";

        let panel = datamodel
            .children()
            .find(|f| f.has_tag_name("Object"))
            .context("Could not find node 'Object' in 'DataModel'!")?;

        let mut is_failed = None;
        let mut inspection_failed = None;
        let mut boards = Vec::new();

        for panel_node in panel.children() {
            match panel_node.tag_name().name() {
                "Status" => {
                    for status_node in panel_node.children() {
                        match status_node.tag_name().name() {
                            "Overall" => {
                                is_failed = status_node.attribute("IsFailed");
                            }
                            "Inspection" => {
                                inspection_failed = status_node.attribute("IsInspectionFailed");
                            }
                            _ => {}
                        }
                    }
                }

                "Object" => {
                    if !panel_node.attribute("Class").is_some_and(|f| f == "Board") {
                        continue;
                    }
                    boards.push(Board::load(panel_node)?);
                }

                _ => {}
            }
        }

        let is_failed =
            is_failed.context("Status -> Overall -> IsFailed was not found!")? == "true";
        let inspection_failed = inspection_failed
            .context("Status -> Overall -> IsInspectionFailed was not found!")?
            == "true";

        Ok(Self {
            product,
            variant,
            barcode,
            inspection_start,
            inspection_end,
            inspection_aborted,
            inspection_failed,
            is_failed,
            boards,
        })
    }
}

impl Board {
    fn load(node: Node<'_, '_>) -> Result<Self> {
        if !node.attribute("Class").is_some_and(|f| f == "Board") {
            error_and_bail!("Node is not a Board node!");
        }

        let barcode = node
            .attribute("Barcode")
            .context("Missing attribute: 'Barcode'")?
            .to_string();
        let name = node
            .attribute("Name")
            .context("Missing attribute: 'Name'")?
            .to_string();

        let mut is_failed = None;
        let mut inspection_failed = None;
        let mut components = Vec::new();

        for child in node.children() {
            match child.tag_name().name() {
                "Status" => {
                    for status_node in child.children() {
                        match status_node.tag_name().name() {
                            "Overall" => {
                                is_failed = status_node.attribute("IsFailed");
                            }
                            "Inspection" => {
                                inspection_failed = status_node.attribute("IsInspectionFailed");
                            }
                            _ => {}
                        }
                    }

                    // break early if no components  failed
                    if inspection_failed.is_some_and(|f| f != "true") {
                        break;
                    }
                }
                "Object" => {
                    if !child.attribute("Class").is_some_and(|f| f == "Comp") {
                        continue;
                    }

                    components.push(Component::load(child)?);
                }
                _ => {}
            }
        }

        let is_failed =
            is_failed.context("Board -> Status -> Overall -> IsFailed was not found!")? == "true";
        let inspection_failed = inspection_failed
            .context("Board -> Status -> Overall -> IsInspectionFailed was not found!")?
            == "true";

        Ok(Board {
            barcode,
            name,
            inspection_failed,
            is_failed,
            components,
        })
    }

    pub fn generate_failed_components_list(&self) -> SPI_failed_board {
        let mut failed_components = Vec::new();

        for component in &self.components {
            if let Some(failed_comp) = component.generate_failed_component() {
                failed_components.push(failed_comp);
            }
        }

        SPI_failed_board{ name: self.name.clone(), failed_components }
    }
}

impl Component {
    fn load(node: Node<'_, '_>) -> Result<Self> {
        if !node.attribute("Class").is_some_and(|f| f == "Comp") {
            error_and_bail!("Node is not a Component node!");
        }

        let name = node
            .attribute("Name")
            .context("Attribute 'Name' missing from Component!")?
            .to_string();
        let comp_type = node
            .attribute("Type")
            .context("Attribute 'Type' missing from Component!")?
            .to_string();

        let mut pads = Vec::new();
        let mut is_failed = None;
        let mut inspection_failed = None;

        for child in node.children() {
            match child.tag_name().name() {
                "Status" => {
                    for status_node in child.children() {
                        match status_node.tag_name().name() {
                            "Overall" => {
                                is_failed = status_node.attribute("IsFailed");
                            }
                            "Inspection" => {
                                inspection_failed = status_node.attribute("IsInspectionFailed");
                            }
                            _ => {}
                        }
                    }

                    // break early if no pads failed
                    if inspection_failed.is_some_and(|f| f != "true") {
                        break;
                    }
                }

                "Object" => {
                    if !child.attribute("Class").is_some_and(|f| f == "Solder") {
                        continue;
                    }

                    pads.push(Pad::load(child)?);
                }

                _ => {}
            }
        }

        let is_failed =
            is_failed.context("Comp -> Status -> Overall -> IsFailed was not found!")? == "true";
        let inspection_failed = inspection_failed
            .context("Comp -> Status -> Overall -> IsInspectionFailed was not found!")?
            == "true";

        Ok(Component {
            name,
            comp_type,
            inspection_failed,
            is_failed,
            pads,
        })
    }

    fn generate_failed_component(&self) -> Option<SPI_failed_component> {
        if !self.inspection_failed { return None;}

        let mut failed_pads = Vec::new();

        for pad in &self.pads {
            if let Some(failed_pad) = pad.generate_failed_pad() {
                failed_pads.push(failed_pad);
            }
        }

        let pseudo = !self.is_failed;
        Some(SPI_failed_component { name: self.name.clone(), pseudo, failed_pads})
    }
}

impl Pad {
    fn load(node: Node<'_, '_>) -> Result<Self> {
        if !node.attribute("Class").is_some_and(|f| f == "Solder") {
            error_and_bail!("Node is not a Solder node!");
        }

        let name = node
            .attribute("Name")
            .context("Attribute 'Name' missing from Solder!")?
            .to_string();

        let name_striped = name.strip_prefix("Solder");

        let pad_type = node
            .attribute("Type")
            .context("Attribute 'Type' missing from Solder!")?
            .to_string();



        let mut features = Vec::new();
        let mut is_failed = None;
        let mut inspection_failed = None;

        for child in node.children() {
            match child.tag_name().name() {
                "Status" => {
                    for status_node in child.children() {
                        match status_node.tag_name().name() {
                            "Overall" => {
                                is_failed = status_node.attribute("IsFailed");
                            }
                            "Inspection" => {
                                inspection_failed = status_node.attribute("IsInspectionFailed");
                            }
                            _ => {}
                        }
                    }

                    // break early if no pads failed
                    if inspection_failed.is_some_and(|f| f != "true") {
                        break;
                    }
                }

                "Features" => {
                    for feature in child.children() {
                        if feature.tag_name().name() != "Feature" { continue; }
                        features.push(Feature::load(feature)?);
                    }
                }

                _ => {}
            }
        }

        let is_failed =
            is_failed.context("Comp -> Status -> Overall -> IsFailed was not found!")? == "true";
        let inspection_failed = inspection_failed
            .context("Comp -> Status -> Overall -> IsInspectionFailed was not found!")?
            == "true";

        Ok(Pad {
            name: if let Some(s) = name_striped { s.to_string() } else {name},
            pad_type,
            inspection_failed,
            is_failed,
            features,
        })
    }

    fn generate_failed_pad(&self) -> Option<SPI_failed_pad> {
        if !self.inspection_failed { return None;}

        let mut failed_features = Vec::new();

        for feature in &self.features {
            if let Some(failed_feature) = feature.generate_failed_feature() {
                failed_features.push(failed_feature);
            }
        }

        let pseudo = !self.is_failed;
        Some(SPI_failed_pad { name: self.name.clone(),pseudo, failed_features })
    }
}

impl Feature {
    fn load(node: Node<'_, '_>) -> Result<Self> {
        if node.tag_name().name() != "Feature" {
            error_and_bail!("Node is not a Feature node! {}", node.tag_name().name() );
        }

        let name = node
            .attribute("Name")
            .context("Attribute 'Name' missing from Feature!")?
            .to_string();

        let name_striped = name.strip_prefix("Solder.");

        let mut values = Vec::new();
        let mut is_failed = None;
        let mut inspection_failed = None;

        for child in node.children() {
            match child.tag_name().name() {
                "Status" => {
                    for status_node in child.children() {
                        match status_node.tag_name().name() {
                            "Overall" => {
                                is_failed = status_node.attribute("IsFailed");
                            }
                            "Inspection" => {
                                inspection_failed = status_node.attribute("IsInspectionFailed");
                            }
                            _ => {}
                        }
                    }

                    // break early if no measurements failed
                    if inspection_failed.is_some_and(|f| f != "true") {
                        break;
                    }
                }

                "Values" => {
                    for value_node in child.children() {
                        if value_node.is_text() {continue;}

                        let v_name = value_node
                            .attribute("Name")
                            .context("Attribute 'Name' missing from Value! {}")?
                            .to_string();

                        let value = value_node
                            .attribute("Value")
                            .context("Attribute 'Value' missing from Value!")?
                            .parse::<f32>()?;

                        let unit = value_node
                            .attribute("Unit")
                            .unwrap_or_default()
                            .to_string();

                        // "Template=Atom.Classification.Enhanced1DWith2Thresholds÷ThresholdLow=50÷ThresholdUpper=150÷WarningThresholdDelta=10÷CompareMode=0;between the thresholds"
                        // Name="DisplacementX" | "DisplacementY" ->
                        //          "Template=Atom.Classification.EnhancedRectangle2D÷ThresholdRectCenterX=0÷ThresholdRectCenterY=0÷ThresholdRectWidth=250÷ThresholdRectHeight=250÷WarningThresholdDelta=10÷CompareMode=0;inside"

                        let thresholds = if let Some(th_str) = value_node.attribute("Threshold") {
                            let th_parts = th_str.split('÷').collect::<Vec<&str>>();
                            let ul;
                            let ll;

                            match v_name.as_str() {
                                "DisplacementX" => {
                                    let ul_str = th_parts
                                        .iter().find(|f| f.starts_with("ThresholdRectWidth"))
                                        .context("ThresholdRectWidth missing!")?;
                                    let ul_val = ul_str
                                        .split_once('=')
                                        .context("ThresholdRectWidth: missing '=' sign!")?;
                                    ul = ul_val.1.parse::<i32>()?;
                                    ll = -ul;
                                }

                                "DisplacementY" => {
                                    let ul_str = th_parts
                                        .iter().find(|f| f.starts_with("ThresholdRectHeight"))
                                        .context("ThresholdRectHeight missing!")?;
                                    let ul_val = ul_str
                                        .split_once('=')
                                        .context("ThresholdRectHeight: missing '=' sign!")?;
                                    ul = ul_val.1.parse::<i32>()?;
                                    ll = -ul;
                                }

                                _ => {
                                    let ul_str = th_parts
                                        .iter().find(|f| f.starts_with("ThresholdUpper"))
                                        .context("ThresholdUpper missing!")?;
                                    let ul_val = ul_str
                                        .split_once('=')
                                        .context("ThresholdUpper: missing '=' sign!")?;
                                    ul = ul_val.1.parse::<i32>()?;

                                    
                                    let ll_str = th_parts
                                        .iter().find(|f| f.starts_with("ThresholdLow"))
                                        .context("ThresholdLow missing!")?;
                                    let ll_val = ll_str
                                        .split_once('=')
                                        .context("ThresholdLow: missing '=' sign!")?;
                                    ll = ll_val.1.parse::<i32>()?;
                                }
                            }

                            Some((ll, ul))
                        } else {
                            None
                        };

                        values.push(Value {
                            name: v_name,
                            value,
                            unit,
                            thresholds,
                        });
                    }
                }

                _ => {}
            }
        }

        let is_failed =
            is_failed.context("Comp -> Status -> Overall -> IsFailed was not found!")? == "true";
        let inspection_failed = inspection_failed
            .context("Comp -> Status -> Overall -> IsInspectionFailed was not found!")?
            == "true";

        Ok(Feature {
            name: if let Some(s) = name_striped { s.to_string() } else { name },
            inspection_failed,
            is_failed,
            values,
        })
    }

    fn generate_failed_feature(&self) -> Option<SPI_failed_feature> {
        if !self.inspection_failed { return None;}

        let mut failed_values = Vec::new();
        let pseudo = !self.is_failed;

        for value in &self.values {
            if let Some((ll,ul)) = value.thresholds {
                let v = value.value.round() as i32;
                if v <= ll || v >= ul {
                    failed_values.push(SPI_failed_value { name: value.name.clone(), value: value.value, limits: (ll,ul) });
                }
            }
        }

        Some(SPI_failed_feature { name: self.name.clone(),pseudo, failed_values })
    }
}


// Stores only the failed positions for a board

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SPI_failed_board {
    pub name: String,
    pub failed_components: Vec<SPI_failed_component>
}

impl SPI_failed_board {
    pub fn strip(&self) -> Self {
        SPI_failed_board { name: self.name.clone(), failed_components: Vec::new() }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SPI_failed_component {
    pub name: String,
    pub pseudo: bool,
    pub failed_pads: Vec<SPI_failed_pad>
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SPI_failed_pad {
    pub name: String,
    pub pseudo: bool,
    pub failed_features: Vec<SPI_failed_feature>
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SPI_failed_feature {
    pub name: String,
    pub pseudo: bool,
    pub failed_values: Vec<SPI_failed_value>
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SPI_failed_value {
    pub name: String,
    pub value: f32,
    pub limits: (i32, i32)
}



#[cfg(test)]
mod tests {
    use log::info;

    use crate::*;

    #[test]
    fn init_log() {
        env_logger::init();
    }

    #[test]
    fn load_log() {
        let spi_log = Panel::load(".\\test_files\\B828853_TOP.xml").unwrap();


        for board in &spi_log.boards {
            info!("{} - {} - {}", board.name, board.barcode, board.inspection_failed);
            let failures = board.generate_failed_components_list();
            for comp in &failures.failed_components {
                info!("\t{}", comp.name);
                for pad in &comp.failed_pads {
                    info!("\t\t{}", pad.name);
                    for feature in &pad.failed_features {
                        info!("\t\t\t{}", feature.name);
                        for value in &feature.failed_values {
                            info!("\t\t\t\t{} - {} < {} < {}", value.name, value.limits.0, value.value, value.limits.1);
                        }
                    }
                }
            }
        }
    }
}
