use crate::{SPI_failed_board, SPI_failed_value};
use chrono::NaiveDateTime;

// For reading boards back from SQL,
// where they are not combined into a Panel
#[derive(Debug, Clone)]
pub struct SingleBoard {
    pub barcode: String,
    pub result: bool, // true - pass, false - failed

    pub inspection_plan: String,
    pub variant: String,
    pub station: String,

    pub date_time: NaiveDateTime,

    pub failed_board_data: SPI_failed_board,
}

// Counting pseudo errors from SPI stations
// grouped by inspection_plan -> component -> pads -> feature
#[derive(Default, Debug)]
pub struct FailedCompCounter {
    pub inspection_plans: Vec<InspCounter>,
}

#[derive(Default, Debug)]
pub struct InspCounter {
    pub name: String,
    pub count_ok: u32,
    pub count_pseudo_nok: u32,
    pub count_nok: u32,
    pub components: Vec<CompCounter>,

    pub show: bool,
}

#[derive(Default, Debug)]
pub struct CompCounter {
    pub name: String,
    pub count_pseudo_nok: u32,
    pub count_nok: u32,
    pub pads: Vec<PadCounter>,

    pub show: bool,
}

#[derive(Default, Debug)]
pub struct PadCounter {
    pub name: String,
    pub count_pseudo_nok: u32,
    pub count_nok: u32,
    pub features: Vec<FeatureCounter>,

    pub show: bool,
}

#[derive(Default, Debug)]
pub struct FeatureCounter {
    pub name: String,
    pub count_pseudo_nok: u32,
    pub count_nok: u32,
    pub results: Vec<SPI_failed_value>,

    pub show: bool,
}

impl FailedCompCounter {
    pub fn generate(data: &[SingleBoard]) -> Self {
        let mut ret = Self::default();

        for board in data {
            // Find the inspection_plan (product) or generate a new one
            let inspection_plan = if let Some(i) = ret
                .inspection_plans
                .iter()
                .position(|f| f.name == board.inspection_plan)
            {
                &mut ret.inspection_plans[i]
            } else {
                ret.inspection_plans.push(InspCounter {
                    name: board.inspection_plan.clone(),
                    count_ok: 0,
                    count_pseudo_nok: 0,
                    count_nok: 0,
                    components: Vec::new(),
                    show: false,
                });
                ret.inspection_plans.last_mut().unwrap()
            };

            if !board.result {
                inspection_plan.count_nok += 1;
            } else {
                if board.failed_board_data.failed_components.is_empty() {
                    inspection_plan.count_ok += 1;
                    continue;
                } else {
                    inspection_plan.count_ok += 1;
                    inspection_plan.count_pseudo_nok += 1;
                }
            }

            for failed_comp in &board.failed_board_data.failed_components {
                let pos_and_name = format!("{}-{}", board.failed_board_data.name, failed_comp.name);

                // Find the component or generate a new one
                let comp = if let Some(i) = inspection_plan
                    .components
                    .iter()
                    .position(|f| f.name == pos_and_name)
                {
                    &mut inspection_plan.components[i]
                } else {
                    inspection_plan.components.push(CompCounter {
                        name: pos_and_name,
                        count_pseudo_nok: 0,
                        count_nok: 0,
                        pads: Vec::new(),
                        show: false,
                    });
                    inspection_plan.components.last_mut().unwrap()
                };

                if failed_comp.pseudo {
                    comp.count_pseudo_nok += 1;
                } else {
                    comp.count_nok += 1;
                }

                for failed_pad in &failed_comp.failed_pads {
                    // Find the pad or generate a new one
                    let pad: &mut PadCounter =
                        if let Some(i) = comp.pads.iter().position(|f| f.name == failed_pad.name) {
                            &mut comp.pads[i]
                        } else {
                            comp.pads.push(PadCounter {
                                name: failed_pad.name.clone(),
                                count_pseudo_nok: 0,
                                count_nok: 0,
                                features: Vec::new(),
                                show: false,
                            });
                            comp.pads.last_mut().unwrap()
                        };

                    if failed_pad.pseudo {
                        pad.count_pseudo_nok += 1;
                    } else {
                        pad.count_nok += 1;
                    }

                    for failed_feature in &failed_pad.failed_features {
                        // Find the feature or generate a new one
                        let feature: &mut FeatureCounter = if let Some(i) = pad
                            .features
                            .iter()
                            .position(|f| f.name == failed_feature.name)
                        {
                            &mut pad.features[i]
                        } else {
                            pad.features.push(FeatureCounter {
                                name: failed_feature.name.clone(),
                                count_pseudo_nok: 0,
                                count_nok: 0,
                                results: Vec::new(),
                                show: false,
                            });
                            pad.features.last_mut().unwrap()
                        };

                        if failed_feature.pseudo {
                            feature.count_pseudo_nok += 1;
                        } else {
                            feature.count_nok += 1;
                        }

                        feature
                            .results
                            .append(&mut failed_feature.failed_values.clone());
                    }
                }
            }
        }

        ret
    }

    pub fn sort(&mut self) {
        self.inspection_plans
            .sort_by(|a, b| b.count_pseudo_nok.cmp(&a.count_pseudo_nok));

        for ip in &mut self.inspection_plans {
            ip.components
                .sort_by(|a, b| b.count_pseudo_nok.cmp(&a.count_pseudo_nok));

            for comp in &mut ip.components {
                comp.pads
                    .sort_by(|a, b| b.count_pseudo_nok.cmp(&a.count_pseudo_nok));

                for pad in &mut comp.pads {
                    pad.features
                        .sort_by(|a, b| b.count_pseudo_nok.cmp(&a.count_pseudo_nok));
                }
            }
        }
    }
}
