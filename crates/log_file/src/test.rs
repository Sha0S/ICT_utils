#[cfg(test)]
mod tests {
    use crate::*;

    #[test]
    fn init_log() {
        env_logger::init();
    }

    #[test]
    fn panel_all_ok() {
        let path = PathBuf::from(".\\test_files\\panel_all_ok.ict");
        let panel = LogFile::load_panel(&path).unwrap();

        let board = &panel[0];

        assert!(board.is_ok());
        assert_eq!(board.get_status(), 0);
        assert!(!board.has_report());
        assert_eq!(board.get_DMC(), "V112506200217B70016003");
        assert_eq!(board.get_product_id(), "DCDC_PSA_C1");
        assert!(board.get_failed_tests().is_empty());
        assert_eq!(board.get_tests().len(), 723);

        let board = &panel[1];

        assert!(board.is_ok());
        assert_eq!(board.get_status(), 0);
        assert!(!board.has_report());
        assert_eq!(board.get_DMC(), "V112506200218B70016003");
        assert_eq!(board.get_product_id(), "DCDC_PSA_C1");
        assert!(board.get_failed_tests().is_empty());
        assert_eq!(board.get_tests().len(), 723);
    }

    #[test]
    fn panel_nok() {
        let path = PathBuf::from(".\\test_files\\panel_board_one_nok.ict");
        let panel = LogFile::load_panel(&path).unwrap();

        let board = &panel[0];

        assert!(board.is_ok());
        assert_eq!(board.get_status(), 4);
        assert!(board.has_report());
        assert_eq!(board.get_DMC(), "V112506300205B70016003");
        assert_eq!(board.get_product_id(), "DCDC_PSA_C1");
        assert!(!board.get_failed_tests().is_empty());
        assert_eq!(board.get_tests().len(), 19);

        assert_eq!(board.get_failed_tests()[0], "shorts");

        let board = &panel[1];

        assert!(board.is_ok());
        assert_eq!(board.get_status(), 0);
        assert!(!board.has_report());
        assert_eq!(board.get_DMC(), "V112506300206B70016003");
        assert_eq!(board.get_product_id(), "DCDC_PSA_C1");
        assert!(board.get_failed_tests().is_empty());
        assert_eq!(board.get_tests().len(), 723);
    }

    #[test]
    fn ict_depth_fault() {
        let path = PathBuf::from(".\\test_files\\kaizen_drv_faulty_log.ict");
        let panel = LogFile::load_panel(&path).unwrap();

        let board = &panel[0];

        assert!(board.is_ok());
        assert_eq!(board.get_status(), 9);
        assert!(board.has_report());
        assert_eq!(board.get_DMC(), "V102513400685AB847026030");
        assert_eq!(board.get_product_id(), "RSA_Kaizen_INV_Driver");
        assert!(!board.get_failed_tests().is_empty());
        assert_eq!(board.get_tests().len(), 946);
    }

    #[test]
    fn cmd_all_ok() {
        let path = PathBuf::from(".\\test_files\\cmd_all_ok.ict");
        let board = LogFile::load(&path).unwrap();

        assert!(board.is_ok());
        assert_eq!(board.get_status(), 0);
        assert!(!board.has_report());
        assert_eq!(board.get_DMC(), "V102508400021DB828853020");
        assert_eq!(board.get_product_id(), "RSA_Kaizen_INV_Command");
        assert!(board.get_failed_tests().is_empty());
        assert_eq!(board.get_tests().len(), 736);
    }

    #[test]
    fn cmd_analog_nok() {
        let path = PathBuf::from(".\\test_files\\cmd_analog_nok.ict");
        let board = LogFile::load(&path).unwrap();

        assert!(board.is_ok());
        assert_eq!(board.get_status(), 6);
        assert!(board.has_report());
        assert_eq!(board.get_DMC(), "V102508400024DB828853020");
        assert_eq!(board.get_product_id(), "RSA_Kaizen_INV_Command");
        assert!(!board.get_failed_tests().is_empty());
        assert_eq!(board.get_tests().len(), 684);

        assert_eq!(board.get_failed_tests()[0], "c201");
    }

    #[test]
    fn fct_all_ok() {
        let path = PathBuf::from(".\\test_files\\fct_all_ok.csv");
        let board = LogFile::load(&path).unwrap();

        assert!(board.is_ok());
        assert_eq!(board.get_status(), 0);
        assert!(!board.has_report());
        assert_eq!(board.get_DMC(), "V102431800038DB828853020");
        assert_eq!(board.get_product_id(), "Kaized CMD");
        assert!(board.get_failed_tests().is_empty());
        assert_eq!(board.get_tests().len(), 275);
    }

    #[test]
    fn fct_nok() {
        let path = PathBuf::from(".\\test_files\\fct_nok.csv");
        let board = LogFile::load(&path).unwrap();

        assert!(board.is_ok());
        assert_eq!(board.get_status(), 10001);
        assert!(board.has_report());
        assert_eq!(board.get_DMC(), "V102431800039DB828853020");
        assert_eq!(board.get_product_id(), "Kaized CMD");
        assert!(!board.get_failed_tests().is_empty());
        assert_eq!(board.get_tests().len(), 13);
    }
}
