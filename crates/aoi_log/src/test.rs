#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use crate::*;

    #[test]
    fn init_log() {
        env_logger::init();
    }

    #[test]
    fn rep_1() {
        let rep_1 = Panel::load_xml(".\\test_files\\rep_1.xml").unwrap();
        assert_eq!(rep_1.station, "JV_Line10");
        assert_eq!(rep_1.inspection_plan, "B847026_TOP");
        assert_eq!(rep_1.variant, "01_ME");

        assert_eq!(
            rep_1.inspection_date_time,
            NaiveDate::from_ymd_opt(2025, 3, 7)
                .unwrap()
                .and_hms_opt(21, 33, 46)
        );

        assert_eq!(
            rep_1.repair,
            Some(Repair {
                date_time: NaiveDate::from_ymd_opt(2025, 3, 7)
                    .unwrap()
                    .and_hms_opt(21, 35, 5)
                    .unwrap(),
                operator: "PK".to_string()
            })
        );

        assert_eq!(
            rep_1.boards[0].barcode,
            "V102506600403EB847026010".to_string()
        );
        assert_eq!(
            rep_1.boards[1].barcode,
            "V102506600404EB847026010".to_string()
        );
        assert_eq!(
            rep_1.boards[2].barcode,
            "V102506600405EB847026010".to_string()
        );
        assert_eq!(
            rep_1.boards[3].barcode,
            "V102506600406EB847026010".to_string()
        );
        assert_eq!(
            rep_1.boards[4].barcode,
            "V102506600407EB847026010".to_string()
        );
        assert_eq!(
            rep_1.boards[5].barcode,
            "V102506600408EB847026010".to_string()
        );

        for b in &rep_1.boards {
            assert!(b.result);
        }

        // Windows

        let board_3 = &rep_1.boards[2].windows;
        assert_eq!(
            board_3[0],
            Window {
                id: "C4354-3".to_string(),
                win_type: "C0402_3D".to_string(),
                analysis_mode: "LAND".to_string(),
                analysis_sub_mode: "15".to_string(),
                result: WindowResult::PseudoError
            }
        );
        assert_eq!(
            board_3[1],
            Window {
                id: "C43502-3".to_string(),
                win_type: "C1206_H1M8_3D".to_string(),
                analysis_mode: "LAND".to_string(),
                analysis_sub_mode: "15".to_string(),
                result: WindowResult::PseudoError
            }
        );
        assert_eq!(
            board_3[2],
            Window {
                id: "R42613-3".to_string(),
                win_type: "R1210_3D".to_string(),
                analysis_mode: "LAND".to_string(),
                analysis_sub_mode: "15".to_string(),
                result: WindowResult::PseudoError
            }
        );

        let board_5 = &rep_1.boards[4].windows;
        assert_eq!(
            board_5[0],
            Window {
                id: "R42625-5".to_string(),
                win_type: "R1210_3D".to_string(),
                analysis_mode: "MENI".to_string(),
                analysis_sub_mode: "9".to_string(),
                result: WindowResult::PseudoError
            }
        );
    }

    #[test]
    fn ins_1() {
        let rep_1 = Panel::load_xml(".\\test_files\\ins_1.xml").unwrap();
        assert_eq!(rep_1.station, "JV_Line10");
        assert_eq!(rep_1.inspection_plan, "B847026_TOP");
        assert_eq!(rep_1.variant, "01_ME");

        assert_eq!(
            rep_1.inspection_date_time,
            NaiveDate::from_ymd_opt(2025, 3, 7)
                .unwrap()
                .and_hms_opt(21, 33, 46)
        );

        assert_eq!(rep_1.repair, None);

        assert_eq!(
            rep_1.boards[0].barcode,
            "V102506600403EB847026010".to_string()
        );
        assert_eq!(
            rep_1.boards[1].barcode,
            "V102506600404EB847026010".to_string()
        );
        assert_eq!(
            rep_1.boards[2].barcode,
            "V102506600405EB847026010".to_string()
        );
        assert_eq!(
            rep_1.boards[3].barcode,
            "V102506600406EB847026010".to_string()
        );
        assert_eq!(
            rep_1.boards[4].barcode,
            "V102506600407EB847026010".to_string()
        );
        assert_eq!(
            rep_1.boards[5].barcode,
            "V102506600408EB847026010".to_string()
        );

        assert!(rep_1.boards[0].result);
        assert!(!rep_1.boards[1].result);
        assert!(!rep_1.boards[2].result);
        assert!(rep_1.boards[3].result);
        assert!(!rep_1.boards[4].result);
        assert!(rep_1.boards[5].result);

        // Windows

        let board_3 = &rep_1.boards[2].windows;
        assert_eq!(
            board_3[2],
            Window {
                id: "C4354-3".to_string(),
                win_type: "C0402_3D".to_string(),
                analysis_mode: "LAND".to_string(),
                analysis_sub_mode: "15".to_string(),
                result: WindowResult::Fail
            }
        );
        assert_eq!(
            board_3[1],
            Window {
                id: "C43502-3".to_string(),
                win_type: "C1206_H1M8_3D".to_string(),
                analysis_mode: "LAND".to_string(),
                analysis_sub_mode: "15".to_string(),
                result: WindowResult::Fail
            }
        );
        assert_eq!(
            board_3[0],
            Window {
                id: "R42613-3".to_string(),
                win_type: "R1210_3D".to_string(),
                analysis_mode: "LAND".to_string(),
                analysis_sub_mode: "15".to_string(),
                result: WindowResult::Fail
            }
        );

        let board_5 = &rep_1.boards[4].windows;
        assert_eq!(
            board_5[0],
            Window {
                id: "R42625-5".to_string(),
                win_type: "R1210_3D".to_string(),
                analysis_mode: "MENI".to_string(),
                analysis_sub_mode: "9".to_string(),
                result: WindowResult::Fail
            }
        );
    }
}
