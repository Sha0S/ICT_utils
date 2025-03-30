use crate::{Window, WindowResult};
use chrono::NaiveDateTime;
use log::{debug, error, info};

// For reading boards back from SQL,
// where they are not combined into a Panel
#[derive(Debug, Default, Clone)]
pub struct SingleBoard {
    pub barcode: String,
    pub result: bool, // true - pass, false - failed

    pub inspection_plan: String,
    pub variant: String,
    pub station: String,

    pub date_time: NaiveDateTime,
    pub operator: String,

    pub windows: Vec<Window>,
}

// Counts pseudo errors for statistics, v2

#[derive(Debug)]
pub struct PseudoErrC {
    pub inspection_plans: Vec<String>,
    pub total_pseudo: Vec<u32>,
    pub total_boards: Vec<u32>,
    pub macros: Vec<MacroErrC>,
}

#[derive(Debug)]
pub struct MacroErrC {
    pub name: String,
    pub total_pseudo: Vec<u32>,
    pub show: bool,
    pub packages: Vec<PackageErrC>,
}

#[derive(Debug)]
pub struct PackageErrC {
    pub name: String,
    pub total_pseudo: Vec<u32>,
    pub show: bool,
    pub positions: Vec<PositionErrC>,
}

#[derive(Debug)]
pub struct PositionErrC {
    pub name: String,
    pub total_pseudo: Vec<u32>,
}

impl PseudoErrC {
    pub fn generate(board_data: &[SingleBoard]) -> Self {
        let mut inspection_plans: Vec<String> = Vec::new();

        // 1 - gather the inspection plans
        for board in board_data {
            if !inspection_plans.contains(&board.inspection_plan) {
                inspection_plans.push(board.inspection_plan.clone());
            }
        }

        let mut total_pseudo = vec![0; inspection_plans.len()];
        let mut total_boards = vec![0; inspection_plans.len()];

        // 2 - iterate over the boards, and search for faulty windows
        let mut macros = Vec::new();

        for board in board_data {
            if board.operator.is_empty() {
                continue;
            } // ignore logs not from the repair station
            if board.windows.is_empty() {
                continue;
            } // ignore logs not containing faults

            let inspection_id = inspection_plans
                .iter()
                .position(|f| *f == board.inspection_plan)
                .unwrap(); // can't fail
            total_boards[inspection_id] += 1;

            for window in &board.windows {
                if window.result != WindowResult::PseudoError {
                    continue;
                } // ignore everything, that is not a pseudoerror

                // Increase inspection plan total counter
                total_pseudo[inspection_id] += 1;

                // Check if the macro already exists in the list, if not make a new one
                let macro_name = format!("{}_{}", window.analysis_mode, window.analysis_sub_mode);
                let macro_counter =
                    if let Some(i) = macros.iter().position(|f: &MacroErrC| f.name == macro_name) {
                        &mut macros[i]
                    } else {
                        macros.push(MacroErrC {
                            name: macro_name.clone(),
                            total_pseudo: vec![0; inspection_plans.len()],
                            show: false,
                            packages: Vec::new(),
                        });
                        macros.last_mut().unwrap() // can't fail
                    };

                // Increase macro total counter
                macro_counter.total_pseudo[inspection_id] += 1;

                // Check if the package already exists in the list, if not make a new one
                let package_counter = if let Some(i) = macro_counter
                    .packages
                    .iter()
                    .position(|f| f.name == window.win_type)
                {
                    &mut macro_counter.packages[i]
                } else {
                    macro_counter.packages.push(PackageErrC {
                        name: window.win_type.clone(),
                        total_pseudo: vec![0; inspection_plans.len()],
                        show: false,
                        positions: Vec::new(),
                    });
                    macro_counter.packages.last_mut().unwrap() // can't fail
                };

                // Increase package total counter
                package_counter.total_pseudo[inspection_id] += 1;

                // Check if the position already exists in the list, if not make a new one
                let position_counter = if let Some(i) = package_counter
                    .positions
                    .iter()
                    .position(|f| f.name == window.id)
                {
                    &mut package_counter.positions[i]
                } else {
                    package_counter.positions.push(PositionErrC {
                        name: window.id.clone(),
                        total_pseudo: vec![0; inspection_plans.len()],
                    });
                    package_counter.positions.last_mut().unwrap() // can't fail
                };

                // Increase positions total counter
                position_counter.total_pseudo[inspection_id] += 1;
            }
        }

        Self {
            inspection_plans,
            total_pseudo,
            total_boards,
            macros,
        }
    }

    pub fn sort_by_ip_name(&mut self, ip_name: Option<&str>) {
        if let Some(name) = ip_name {
            if let Some(id) = self.inspection_plans.iter().position(|f| f == name) {
                self.sort_by_ip_id(Some(id));
            }
        } else {
            self.sort_by_ip_id(None);
        }
    }

    pub fn sort_by_ip_id(&mut self, ip_id: Option<usize>) {
        if let Some(i) = ip_id {
            if i >= self.inspection_plans.len() {
                return;
            }

            self.macros
                .sort_by(|a, b| b.total_pseudo[i].cmp(&a.total_pseudo[i]));
            for macroc in &mut self.macros {
                macroc
                    .packages
                    .sort_by(|a, b| b.total_pseudo[i].cmp(&a.total_pseudo[i]));
                for package in &mut macroc.packages {
                    package
                        .positions
                        .sort_by(|a, b| b.total_pseudo[i].cmp(&a.total_pseudo[i]));
                }
            }
        } else {
            self.macros.sort_by(|a, b| {
                b.total_pseudo
                    .iter()
                    .sum::<u32>()
                    .cmp(&a.total_pseudo.iter().sum())
            });
            for macroc in &mut self.macros {
                macroc.packages.sort_by(|a, b| {
                    b.total_pseudo
                        .iter()
                        .sum::<u32>()
                        .cmp(&a.total_pseudo.iter().sum())
                });
                for package in &mut macroc.packages {
                    package.positions.sort_by(|a, b| {
                        b.total_pseudo
                            .iter()
                            .sum::<u32>()
                            .cmp(&a.total_pseudo.iter().sum())
                    });
                }
            }
        }
    }
}

// Counting pseudo errors from repair stations
// for statistics. Grouped by insection_plan -> macro -> package -> positions
#[derive(Default, Debug)]
pub struct ErrorCounter {
    pub total_pseudo: u32,
    pub total_error: u32,

    pub number_of_boards: u32,
    pub boards_with_errors: u32,

    pub inspection_plans: Vec<InspectionPlanCounter>,
}

#[derive(Default, Debug)]
pub struct InspectionPlanCounter {
    pub name: String,
    pub total_pseudo: u32,
    pub total_error: u32,

    pub number_of_boards: u32,
    pub boards_with_errors: u32,

    pub macros: Vec<MacroCounter>,
}

#[derive(Default, Debug)]
pub struct MacroCounter {
    pub name: String,
    pub total_pseudo: u32,
    pub total_error: u32,

    pub packages: Vec<PackageCounter>,
}

#[derive(Default, Debug)]
pub struct PackageCounter {
    pub name: String,
    pub total_pseudo: u32,
    pub total_error: u32,

    pub positions: Vec<PositionCounter>,
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
            self.inspection_plans.push(InspectionPlanCounter {
                name: plan.to_string(),
                ..Default::default()
            });
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

        info!(
            "Total pseudo: {}, total errors: {}",
            self.total_pseudo, self.total_error
        );
    }

    fn sort(&mut self) {
        self.inspection_plans
            .sort_by(|a, b| b.total_pseudo.cmp(&a.total_pseudo));
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
                    let macro_name =
                        format!("{}_{}", window.analysis_mode, window.analysis_sub_mode);
                    let macro_counter = plan.get_or_insert(&macro_name);
                    let package_counter = macro_counter.get_or_insert(&window.win_type);
                    let position_counter = package_counter.get_or_insert(&window.id);

                    match window.result {
                        crate::WindowResult::Fail => {
                            position_counter.total_error += 1;
                        }
                        crate::WindowResult::PseudoError => {
                            position_counter.total_pseudo += 1;
                        }
                        crate::WindowResult::Unknown | crate::WindowResult::Pass => {
                            error!(
                                "Recived illegal window result: {} - {:?}",
                                window.id, window.result
                            );
                        }
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
            self.macros.push(MacroCounter {
                name: x.to_string(),
                ..Default::default()
            });
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
        self.macros
            .sort_by(|a, b| b.total_pseudo.cmp(&a.total_pseudo));
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
            self.packages.push(PackageCounter {
                name: x.to_string(),
                ..Default::default()
            });
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
        self.packages
            .sort_by(|a, b| b.total_pseudo.cmp(&a.total_pseudo));
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
            self.positions.push(PositionCounter {
                name: x.to_string(),
                ..Default::default()
            });
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
        self.positions
            .sort_by(|a, b| b.total_pseudo.cmp(&a.total_pseudo));
    }
}
