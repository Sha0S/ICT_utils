use std::sync::Arc;
use tiberius::Client;
use tokio::net::TcpStream;
use tokio_util::compat::Compat;

mod aoi_station;
use aoi_station::*;

mod ict_station;
use ict_station::*;

mod fct_station;
use fct_station::*;

use crate::TimeFrame;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Station {
    Aoi,
    Ict,
    Fct,
}

#[derive(Debug)]
pub struct StationHandler {
    connection: Arc<tokio::sync::Mutex<Option<Client<Compat<TcpStream>>>>>,

    selected_station: Station,
    aoi_station: AoiStation,
    _ict_station: IctStation,
    _fct_station: FctStation,
}

impl StationHandler {
    pub fn new() -> Self {
        Self {
            connection: Arc::new(tokio::sync::Mutex::new(None)),
            selected_station: Station::Aoi,
            aoi_station: AoiStation::default(),
            _ict_station: IctStation::default(),
            _fct_station: FctStation::default(),
        }
    }

    pub fn print_selected_station(&self) -> &str {
        match self.selected_station {
            Station::Aoi => "AOI",
            Station::Ict => "ICT",
            Station::Fct => "FCT",
        }
    }

    pub fn change_station(&mut self, new_station: Station) {
        self.selected_station = new_station;
    }

    pub fn side_panel(&mut self, ctx: &egui::Context, ui: &mut egui::Ui, timeframe: TimeFrame<'_>) {
        match self.selected_station {
            Station::Aoi => {
                self.aoi_station
                    .side_panel(ctx, ui, timeframe, self.connection.clone())
            }
            Station::Ict => {}
            Station::Fct => {}
        }
    }

    pub fn central_panel(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        match self.selected_station {
            Station::Aoi => self.aoi_station.central_panel(ctx, ui),
            Station::Ict => {}
            Station::Fct => {}
        }
    }
}
