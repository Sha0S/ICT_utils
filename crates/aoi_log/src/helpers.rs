use chrono::NaiveDateTime;
use log::{debug, error, info};
use crate::Window;


// For reading boards back from SQL,
// where they are not combined into a Panel
#[derive(Debug, Default, Clone)]
pub struct SingleBoard {
    pub barcode: String,
    pub result: bool,           // true - pass, false - failed

    pub inspection_plan: String,
    pub variant: String,
    pub station: String,
    
    pub date_time: NaiveDateTime,
    pub operator: String,

    pub windows: Vec<Window>
}

// Counting pseudo errors from repair stations
// for statistics. Grouped by insection_plan -> macro -> package -> positions
#[derive(Default, Debug)]
pub struct ErrorCounter {
    pub total_pseudo: u32,
    pub total_error: u32,

    pub number_of_boards: u32,
    pub boards_with_errors: u32,

    pub inspection_plans: Vec<InspectionPlanCounter>
}

#[derive(Default, Debug)]
pub struct InspectionPlanCounter {
    pub name: String,
    pub total_pseudo: u32,
    pub total_error: u32,

    pub number_of_boards: u32,
    pub boards_with_errors: u32,

    pub macros: Vec<MacroCounter>
}

#[derive(Default, Debug)]
pub struct MacroCounter {
    pub name: String,
    pub total_pseudo: u32,
    pub total_error: u32,

    pub packages: Vec<PackageCounter>
}

#[derive(Default, Debug)]
pub struct PackageCounter {
    pub name: String,
    pub total_pseudo: u32,
    pub total_error: u32,

    pub positions: Vec<PositionCounter>
}

#[derive(Default, Debug)]
pub struct PositionCounter {
    pub name: String,
    pub total_pseudo: u32,
    pub total_error: u32,
}

impl ErrorCounter {
    fn get_or_insert(&mut self, plan: &str) -> &mut InspectionPlanCounter {
        if let Some(p) = &self.inspection_plans.iter().position(|f| f.name == plan) {
            &mut self.inspection_plans[*p]
        } else {
            self.inspection_plans.push(InspectionPlanCounter{ name: plan.to_string(), ..Default::default()});
            self.inspection_plans.last_mut().unwrap()
        }
    }

    fn update_counters(&mut self) {
        self.total_pseudo = 0;
        self.total_error = 0;

        for x in &mut self.inspection_plans {
            x.update_counters();
            self.total_pseudo += x.total_pseudo;
            self.total_error += x.total_error;
        }

        info!("Total pseudo: {}, total errors: {}", self.total_pseudo, self.total_error);
    }

    fn sort(&mut self) {
        self.inspection_plans.sort_by(|a,b| b.total_pseudo.cmp(&a.total_pseudo));
        for x in &mut self.inspection_plans {
            x.sort();
        }
    }

    pub fn generate(data: &[SingleBoard]) -> Self {
        info!("Generating pseudo error data for {} boards", data.len());

        let mut ret = ErrorCounter::default();

        for board in data {
            // filters out inspection results, we only need repair ones
            if !board.operator.is_empty() {

                ret.number_of_boards += 1;
                if !board.windows.is_empty() {
                    ret.boards_with_errors += 1;
                }

                let plan = ret.get_or_insert(&board.inspection_plan);

                plan.number_of_boards += 1;
                if !board.windows.is_empty() {
                    plan.boards_with_errors += 1;
                }

                for window in &board.windows {
                    let macro_name = format!("{}_{}", window.analysis_mode, window.analysis_sub_mode);
                    let macro_counter = plan.get_or_insert(&macro_name);
                    let package_counter = macro_counter.get_or_insert(&window.win_type);
                    let position_counter = package_counter.get_or_insert(&window.id);

                    match window.result {
                        crate::WindowResult::Fail => {
                            position_counter.total_error += 1;
                        },
                        crate::WindowResult::PseudoError => {
                            position_counter.total_pseudo += 1;
                        },
                        crate::WindowResult::Unknown | crate::WindowResult::Pass => {
                            error!("Recived illegal window result: {} - {:?}", window.id, window.result);
                        },
                    }
                }
            }
        }

        debug!("Updating counters");
        ret.update_counters();

        debug!("Sorting results");
        ret.sort();

        info!("Generation done!");
        ret
    }
}

impl InspectionPlanCounter {
    fn get_or_insert(&mut self, x: &str) -> &mut MacroCounter {
        if let Some(p) = &self.macros.iter().position(|f| f.name == x) {
            &mut self.macros[*p]
        } else {
            self.macros.push(MacroCounter{ name: x.to_string(), ..Default::default()});
            self.macros.last_mut().unwrap()
        }
    }

    fn update_counters(&mut self) {
        self.total_pseudo = 0;
        self.total_error = 0;

        for x in &mut self.macros {
            x.update_counters();
            self.total_pseudo += x.total_pseudo;
            self.total_error += x.total_error;
        }
    }

    fn sort(&mut self) {
        self.macros.sort_by(|a,b| b.total_pseudo.cmp(&a.total_pseudo));
        for x in &mut self.macros {
            x.sort();
        }
    }
}

impl MacroCounter {
    fn get_or_insert(&mut self, x: &str) -> &mut PackageCounter {
        if let Some(p) = &self.packages.iter().position(|f| f.name == x) {
            &mut self.packages[*p]
        } else {
            self.packages.push(PackageCounter{ name: x.to_string(), ..Default::default()});
            self.packages.last_mut().unwrap()
        }
    }

    fn update_counters(&mut self) {
        self.total_pseudo = 0;
        self.total_error = 0;

        for x in &mut self.packages {
            x.update_counters();
            self.total_pseudo += x.total_pseudo;
            self.total_error += x.total_error;
        }
    }

    fn sort(&mut self) {
        self.packages.sort_by(|a,b| b.total_pseudo.cmp(&a.total_pseudo));
        for x in &mut self.packages {
            x.sort();
        }
    }
}

impl PackageCounter {
    fn get_or_insert(&mut self, x: &str) -> &mut PositionCounter {
        if let Some(p) = &self.positions.iter().position(|f| f.name == x) {
            &mut self.positions[*p]
        } else {
            self.positions.push(PositionCounter{ name: x.to_string(), ..Default::default()});
            self.positions.last_mut().unwrap()
        }
    }

    fn update_counters(&mut self) {
        self.total_pseudo = 0;
        self.total_error = 0;

        for x in &self.positions {
            self.total_pseudo += x.total_pseudo;
            self.total_error += x.total_error;
        }
    }

    fn sort(&mut self) {
        self.positions.sort_by(|a,b| b.total_pseudo.cmp(&a.total_pseudo));
    }
}