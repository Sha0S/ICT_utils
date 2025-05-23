use crate::{Window, WindowResult};
use chrono::{Datelike, NaiveDate, NaiveDateTime};
use log::{debug, error, info};
use std::collections::HashSet;

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

// Generate a list of the boards and positions marked as faulty. ("real" errors)

#[derive(Default, Debug)]
pub struct ErrorList {
    pub inspection_plans: Vec<InspectionErrList>,
}

#[derive(Default, Debug)]
pub struct InspectionErrList {
    pub name: String,
    pub failed_boards: Vec<BoardErrList>,
}

#[derive(Default, Debug)]
pub struct BoardErrList {
    pub barcode: String,
    pub date_time: NaiveDateTime,
    pub failed_positions: Vec<String>,
}

impl ErrorList {
    pub fn generate(limit: usize, board_data: &[SingleBoard]) -> Self {
        let mut ret = Self::default();

        for board in board_data {
            if board.operator.is_empty() || board.result {
                continue;
            } // ignore logs not from the repair station and logs from passed boards

            // ignore any board with failures above the set limit
            if board.windows.len() > limit {
                continue;
            }

            // 1 - check if the inspection_plan already exists, if not create one.
            let inspection_plan = if let Some(id) = ret
                .inspection_plans
                .iter()
                .position(|f| f.name == board.inspection_plan)
            {
                &mut ret.inspection_plans[id]
            } else {
                ret.inspection_plans.push(InspectionErrList {
                    name: board.inspection_plan.clone(),
                    failed_boards: Vec::new(),
                });

                ret.inspection_plans.last_mut().unwrap()
            };

            // 2 - gather all uinque failed positions
            let mut failed_positions = Vec::new();

            for pos in board
                .windows
                .iter()
                .filter(|f| f.result == WindowResult::Fail)
            {
                if !failed_positions.contains(&pos.id) {
                    failed_positions.push(pos.id.clone());
                }
            }

            // 3 - insert new board data
            inspection_plan.failed_boards.push(BoardErrList {
                barcode: board.barcode.clone(),
                date_time: board.date_time,
                failed_positions,
            });
        }

        ret
    }
}

// For tracking pseudo errors over time
// Calculates daily/weekly failure rate for the loaded boards
#[derive(Default, Debug)]
pub struct ErrorTrackerT {
    pub inspection_plans: Vec<InspectionErrT>,
}

#[derive(Default, Clone, Debug)]
pub struct InspectionErrT {
    pub name: String,
    pub days: Vec<DailyErrT>,
    pub weeks: Vec<WeeklyErrT>,
}

#[derive(Default, Clone, Debug)]
pub struct DailyErrT {
    pub date: NaiveDate,
    pub total_boards: u32,
    pub r_failed_boards: u32,
    pub p_failed_boards: u32,
    pub total_real_errors: u32,
    pub total_pseudo_errors: u32,
    pub pseudo_errors_per_board: f32,
    pub real_errors_per_board: f32,
}

#[derive(Default, Clone, Debug)]
pub struct WeeklyErrT {
    pub year: i32,
    pub week: u32,
    pub total_boards: u32,
    pub r_failed_boards: u32,
    pub p_failed_boards: u32,
    pub total_real_errors: u32,
    pub total_pseudo_errors: u32,
    pub pseudo_errors_per_board: f32,
    pub real_errors_per_board: f32,
}

impl ErrorTrackerT {
    pub fn generate(limit: usize, board_data: &[SingleBoard]) -> Self {
        let mut ret = ErrorTrackerT::default();

        // tracks which barcode we have already processed for which inspection_plan (barcode, inspection_plan)
        let mut barcodes_at_repair: HashSet<(String, String)> = HashSet::new();

        for board in board_data {
            if board.operator.is_empty() {
                continue;
            } // ignore logs not from the repair station

            // ignore any board with failures above the set limit
            if board.windows.len() > limit {
                continue;
            }

            // 1 - check if the inspection_plan already exists, if not create one.
            let inspection_plan = if let Some(id) = ret
                .inspection_plans
                .iter()
                .position(|f| f.name == board.inspection_plan)
            {
                &mut ret.inspection_plans[id]
            } else {
                ret.inspection_plans.push(InspectionErrT {
                    name: board.inspection_plan.clone(),
                    days: Vec::new(),
                    weeks: Vec::new(),
                });

                ret.inspection_plans.last_mut().unwrap()
            };

            barcodes_at_repair.insert((board.barcode.clone(), inspection_plan.name.clone()));

            // 2 - check if the date already exist, if not create one
            let date = board.date_time.date();
            let day = if let Some(id) = inspection_plan.days.iter().position(|f| f.date == date) {
                &mut inspection_plan.days[id]
            } else {
                inspection_plan.days.push(DailyErrT {
                    date,
                    ..Default::default()
                });

                inspection_plan.days.last_mut().unwrap()
            };

            day.total_boards += 1;

            if board.windows.is_empty() {
                continue;
            } // ignore logs not containing faults

            // number of windows > 0 && result = OK  -> pseudo failed board
            // number of windows > 0 && result = NOK -> real failed board

            if board.result {
                day.p_failed_boards += 1;
            } else {
                day.r_failed_boards += 1;
            }

            for window in &board.windows {
                if window.result == WindowResult::PseudoError {
                    day.total_pseudo_errors += 1;
                } else {
                    day.total_real_errors += 1;
                }
            }
        }

        // if a panel contains no errors at all, then it will have no result from the Repair station
        // to get the correct number of PCBs, we have to check for these too.
        // We also filter any boards which have no repair result, but have failed windows.
        // This mainly happens while panels are in the buffer, waiting repair.
        for board in board_data {
            if !board.operator.is_empty() || !board.windows.is_empty() {
                continue;
            } // ignore logs from the repair station and those which have failures

            // 1 - check if the inspection_plan already exists, if not create one.
            let inspection_plan = if let Some(id) = ret
                .inspection_plans
                .iter()
                .position(|f| f.name == board.inspection_plan)
            {
                &mut ret.inspection_plans[id]
            } else {
                ret.inspection_plans.push(InspectionErrT {
                    name: board.inspection_plan.clone(),
                    days: Vec::new(),
                    weeks: Vec::new(),
                });

                ret.inspection_plans.last_mut().unwrap()
            };

            if !barcodes_at_repair.contains(&(board.barcode.clone(), inspection_plan.name.clone()))
            {
                let date = board.date_time.date();
                let day = if let Some(id) = inspection_plan.days.iter().position(|f| f.date == date)
                {
                    &mut inspection_plan.days[id]
                } else {
                    inspection_plan.days.push(DailyErrT {
                        date,
                        ..Default::default()
                    });

                    inspection_plan.days.last_mut().unwrap()
                };

                day.total_boards += 1;
            }
        }

        for ip in &mut ret.inspection_plans {
            ip.days.sort_by_key(|f| f.date);

            for day in &mut ip.days {
                day.pseudo_errors_per_board =
                    day.total_pseudo_errors as f32 / day.total_boards as f32;
                day.real_errors_per_board = day.total_real_errors as f32 / day.total_boards as f32;
            }
        }

        // Pupulate weekly stats
        for ip in &mut ret.inspection_plans {
            for day in &mut ip.days {
                let year = day.date.year();
                let week_number = day.date.iso_week().week();

                let week = if let Some(i) = ip
                    .weeks
                    .iter()
                    .position(|f| f.year == year && f.week == week_number)
                {
                    &mut ip.weeks[i]
                } else {
                    ip.weeks.push(WeeklyErrT {
                        year,
                        week: week_number,
                        ..Default::default()
                    });

                    ip.weeks.last_mut().unwrap()
                };

                week.total_boards += day.total_boards;
                week.p_failed_boards += day.p_failed_boards;
                week.total_pseudo_errors += day.total_pseudo_errors;
                week.r_failed_boards += day.r_failed_boards;
                week.total_real_errors += day.total_real_errors;
            }

            ip.weeks.sort_by(|a, b| {
                if a.year == b.year {
                    a.week.cmp(&b.week)
                } else {
                    a.year.cmp(&b.year)
                }
            });

            for week in &mut ip.weeks {
                week.pseudo_errors_per_board =
                    week.total_pseudo_errors as f32 / week.total_boards as f32;
                week.real_errors_per_board =
                    week.total_real_errors as f32 / week.total_boards as f32;
            }
        }

        ret.inspection_plans.sort_by(|a, b| a.name.cmp(&b.name));

        ret
    }
}

// Counts pseudo errors for statistics, v2

#[derive(Debug)]
pub struct PseudoErrC {
    pub inspection_plans: Vec<String>,
    pub total_pseudo: Vec<u32>,
    pub total_boards: Vec<u32>,
    pub failed_boards: Vec<u32>,
    pub pseudo_per_board: Vec<f32>,
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
    pub fn generate(limit: usize, board_data: &[SingleBoard]) -> Self {
        let mut inspection_plans: Vec<String> = Vec::new();

        // 1 - gather the inspection plans
        for board in board_data {
            if !inspection_plans.contains(&board.inspection_plan) {
                inspection_plans.push(board.inspection_plan.clone());
            }
        }

        inspection_plans.sort();

        let mut total_pseudo = vec![0; inspection_plans.len()];
        let mut total_boards = vec![0; inspection_plans.len()];
        let mut failed_boards = vec![0; inspection_plans.len()];

        // 2 - iterate over the boards, and search for faulty windows
        let mut macros = Vec::new();
        let mut barcodes_at_repair: HashSet<(String, usize)> = HashSet::new();

        for board in board_data {
            if board.operator.is_empty() {
                continue;
            } // ignore logs not from the repair station

            // ignore any board with failures above the set limit
            if board.windows.len() > limit {
                continue;
            }

            let inspection_id = inspection_plans
                .iter()
                .position(|f| *f == board.inspection_plan)
                .unwrap(); // can't fail
            total_boards[inspection_id] += 1;

            barcodes_at_repair.insert((board.barcode.clone(), inspection_id));

            if board.windows.is_empty() {
                continue;
            } // ignore logs not containing faults

            failed_boards[inspection_id] += 1;

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

        // if a panel contains no errors at all, then it will have no result from the Repair station
        // to get the correct number of PCBs, we have to check for these too.
        // We also filter any boards which have no repair result, but have failed windows.
        // This mainly happens while panels are in the buffer, waiting repair.
        for board in board_data {
            if !board.operator.is_empty() || !board.windows.is_empty() {
                continue;
            } // ignore logs from the repair station and those which have failures

            let inspection_id = inspection_plans
                .iter()
                .position(|f| *f == board.inspection_plan)
                .unwrap(); // can't fail

            if !barcodes_at_repair.contains(&(board.barcode.clone(), inspection_id)) {
                total_boards[inspection_id] += 1;
            }
        }

        let mut pseudo_per_board = vec![0.0; inspection_plans.len()];
        for i in 0..inspection_plans.len() {
            pseudo_per_board[i] = total_pseudo[i] as f32 / total_boards[i] as f32;
        }

        Self {
            inspection_plans,
            total_pseudo,
            total_boards,
            failed_boards,
            pseudo_per_board,
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
