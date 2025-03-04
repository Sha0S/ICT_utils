use egui_extras::{Column, TableBuilder};

pub struct FctOverlay {
    enabled: bool,
    pos: ICT_config::OverlayPos
}

impl FctOverlay {
    pub fn new(pos:  ICT_config::OverlayPos) -> Self {
        Self { enabled: false, pos }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn enable(&mut self) {
        self.enabled = true;
    }

    pub fn update(
        &mut self,
        ctx: &egui::Context,
        hourly_stats: &[ICT_log_file::HourlyStats],
        hourly_boards: bool,
        hourly_gs: bool,
    ) {
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("FctOverlay"),
            egui::ViewportBuilder::default()
                .with_position(egui::Pos2 { x: self.pos.x, y: self.pos.y })
                .with_inner_size(egui::Vec2 { x: self.pos.w, y: self.pos.h })
                .with_decorations(false)
                .with_window_level(egui::WindowLevel::AlwaysOnTop), 
                //.with_mouse_passthrough(true)
                //.with_transparent(true)
            |ctx, class| {
                assert!(
                    class == egui::ViewportClass::Immediate,
                    "This egui backend doesn't support multiple viewports"
                );

                egui::SidePanel::left("cntrl")
                    .min_width(20.0)
                    .show(ctx, |ui| {
                        ui.add_space(5.0);
                        if ui.button("x").clicked() {
                            self.enabled = false;
                        }
                    });

                egui::CentralPanel::default()
                    //.frame(egui::Frame::none())
                    .show(ctx, |ui| {
                        let rows = ((self.pos.h - 30.0) / (14.0+3.0)).floor().max(1.0) as usize;


                        TableBuilder::new(ui)
                            .striped(true)
                            .column(Column::exact(150.0))
                            .column(Column::exact(50.0))
                            .column(Column::exact(50.0))
                            .column(Column::remainder())
                            .header(20.0, |mut header| {
                                header.col(|_| {});
                                header.col(|ui| {
                                    ui.heading("OK");
                                });
                                header.col(|ui| {
                                    ui.heading("NOK");
                                });
                                header.col(|_| {});
                            })
                            .body(|mut body| {
                                for hour in hourly_stats.iter().rev().take(rows) {
                                    let used_yield = if hourly_gs {
                                        if hourly_boards {
                                            &hour.1.boards_with_gs
                                        } else {
                                            &hour.1.panels_with_gs
                                        }
                                    } else if hourly_boards {
                                        &hour.1.boards
                                    } else {
                                        &hour.1.panels
                                    };

                                    body.row(14.0, |mut row| {
                                        row.col(|ui| {
                                            ui.label(crate::u64_to_timeframe(hour.0));
                                        });
                                        row.col(|ui| {
                                            ui.label(format!("{}", used_yield.0));
                                        });
                                        row.col(|ui| {
                                            ui.label(format!("{}", used_yield.1));
                                        });
                                        row.col(|ui| {
                                            ui.spacing_mut().interact_size =
                                                egui::Vec2::new(0.0, 0.0);
                                            ui.spacing_mut().item_spacing =
                                                egui::Vec2::new(3.0, 3.0);

                                            ui.horizontal(|ui| {
                                                for (r, _, _, gs) in &hour.2 {
                                                    crate::draw_result_box(ui, r, *gs).clicked();
                                                }
                                            });
                                        });
                                    });
                                }
                            });
                    });
            },
        );
    }
}
