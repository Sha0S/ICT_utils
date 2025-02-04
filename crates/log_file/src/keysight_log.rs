/*
ToDo:
Implement special characters '~' (literal field) and '\' (list of fields)

Q:
- BATCH -> "version label" field?
*/

use std::{fs, io, path::Path, str::Chars};

type Result<T> = std::result::Result<T, ParsingError>;

#[derive(Debug, Clone)]
struct ParsingError;

#[derive(Debug, Clone, Copy)]
pub enum AnalogTest {
    Cap,         // A-CAP
    Diode,       // A-DIO
    Fuse,        // A-FUS
    Inductor,    // A-IND
    Jumper,      // A-JUM
    Measurement, // A-MEA
    NFet,        // A-NFE
    PFet,        // A-PFE
    Npn,         // A-NPN
    Pnp,         // A-PNP
    Pot,         // A-POT
    Res,         // A-RES
    Switch,      // A-SWI
    Zener,       // A-ZEN

    Error,
}

impl From<&str> for AnalogTest {
    fn from(value: &str) -> Self {
        match value {
            "@A-CAP" => Self::Cap,
            "@A-DIO" => Self::Diode,
            "@A-FUS" => Self::Fuse,
            "@A-IND" => Self::Inductor,
            "@A-JUM" => Self::Jumper,
            "@A-MEA" => Self::Measurement,
            "@A-NFE" => Self::NFet,
            "@A-PFE" => Self::PFet,
            "@A-NPN" => Self::Npn,
            "@A-PNP" => Self::Pnp,
            "@A-POT" => Self::Pot,
            "@A-RES" => Self::Res,
            "@A-SWI" => Self::Switch,
            "@A-ZEN" => Self::Zener,

            _ => Self::Error,
        }
    }
}

pub fn status_to_str(s: i32) -> String {
    match s {
        0 => "passed".to_string(),
        1 => "uncategorized_failure".to_string(),
        2 => "failed_pin_test".to_string(),
        3 => "failed_in_learn_mode".to_string(),
        4 => "failed_shorts_test".to_string(),
        5 => "(reserved)".to_string(),
        6 => "failed_analog_test".to_string(),
        7 => "failed_power_supply_test".to_string(),
        8 => "failed_digital_or_boundary_scan_test".to_string(),
        9 => "failed_functional_test".to_string(),
        10 => "failed_pre-shorts_test".to_string(),
        11 => "failed_in_board_handler".to_string(),
        12 => "failed_barcode".to_string(),
        13 => "X’d_out".to_string(),
        14 => "failed_in_VTEP_or_TestJet".to_string(),
        15 => "failed_in_polarity_check".to_string(),
        16 => "failed_in_ConnectCheck".to_string(),
        17 => "failed_in_analog_cluster_test".to_string(),
        18..=79 => "reserved".to_string(),
        80 => "runtime_error".to_string(),
        81 => "aborted_(STOP)".to_string(),
        82 => "aborted_(BREAK)".to_string(),
        83..=89 => "reserved".to_string(),
        90 => "programming_error".to_string(), // 90 is "user-definable" too, but we spec use if for programming failures
        91..=99 => "user-definable".to_string(),
        _ => "parsing_failed".to_string(),
    }
}

#[derive(Debug, Clone)]
pub enum KeysightPrefix {
    // {@A-???|test status|measured value (|subtest designator)}
    Analog(AnalogTest, i32, f32, Option<String>),

    // {@AID|time detected|serial number}
    AlarmId(u64, String),
    // {@ALM|alarm type|alarm status|time detected|board type|board type rev|alarm limit|detected value|controller|testhead number}
    Alarm(i32, bool, u64, String, String, i32, i32, String, i32),

    // {@ARRAY|subtest designator|status|failure count|samples}
    Array(String, i32, i32, i32),

    // {@BATCH|UUT type|UUT type rev|fixture id|testhead number|testhead type|process step|batch id|
    //      operator id|controller|testplan id|testplan rev|parent panel type|parent panel type rev (| version label)}
    Batch(
        String,
        String,
        i32,
        i32,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        Option<String>,
    ),

    // {@BLOCK|block designator|block status}
    Block(String, i32),

    // {@BS-CON|test designator|status|shorts count|opens count}
    Boundary(String, i32, i32, i32),
    // {@BS-O|first device|first pin|second device|second pin}
    BoundaryOpen(String, i32, String, i32),
    // {@BS-S|cause|node list}
    BoundaryShort(String, String),

    // {@BTEST|board id|test status|start datetime|duration|multiple test|log level|log set|learning|
    // known good|end datetime|status qualifier|board number|parent panel id}
    BTest(
        String,
        i32,
        u64,
        i32,
        bool,
        String,
        i32,
        bool,
        bool,
        u64,
        String,
        i32,
        Option<String>,
    ),

    // {@CCHK|test status|pin count|test designator}
    CChk(i32, i32, String),

    // {@DPIN|device name|node pin list} or
    // {@DPIN|device name|node pin list|thru devnode list} with DriveThru
    DPin(String, Vec<(String, String)>),

    // {@D-PLD|Filename|Action|Action return code|Result message string|Player program counter| }
    DPld(String, String, i32, String, i32),
    // {@EXPRT|Key|Field}
    Export(String, i32),
    // {@NOTE|Note name|Note string}
    Note(String, String),

    // {@D-T|test status|test substatus|failing vector number|pin count|test designator}
    Digital(i32, i32, i32, i32, String),

    // {@INDICT|technique|device list} ex: {@INDICT|DT\3|rp6:r2|c412|r22}
    Indict(String, Vec<String>),

    // {@LIM2|high limit|low limit}
    // {@LIM3|nominal value|high limit|low limit}
    Lim2(f32, f32),
    Lim3(f32, f32, f32),

    // {@NETV|datetime|test system|repair system|source}
    NetV(u64, String, String, bool),

    // {@NODE\node list}
    Node(Vec<String>),

    // {@PCHK|test status|test designator}
    PChk(i32, String),

    // {@PF|designator|test status|total pins}
    Pins(String, i32, i32),
    // {@PIN\pin list}
    Pin(Vec<String>),

    // {@PRB|test status|pin count|test designator}
    Prb(i32, i32, String),

    // {@RETEST|datetime}
    Retest(u64),
    // {@RPT|message}
    Report(String),

    // {@TJET|test status|pin count|test designator}
    TJet(i32, i32, String),

    // {@TS|test status|shorts count|opens count|phantoms count (|designator) }
    Shorts(i32, i32, i32, i32, Option<String>),
    // {@TS-S|shorts count|phantoms count|source node}  short source
    // {@TS-D\destination list}                         short destination
    // {@TS-P|deviation}                                phantom shorts
    // {@TS-O|source node|destination node|deviation}   opens
    ShortsSrc(i32, i32, String),
    ShortsDest(Vec<(String, f32)>),
    ShortsPhantom(f32),
    ShortsOpen(String, String, f32),

    UserDefined(Vec<String>),
    Error(String),
}

fn to_int(field: Option<&String>) -> Result<i32> {
    if let Some(string) = field {
        if string.is_empty() {
            return Ok(0);
        }

        if let Ok(i) = string.parse::<i32>() {
            return Ok(i);
        }
    }

    Err(ParsingError)
}

fn to_uint(field: Option<&String>) -> Result<u64> {
    if let Some(string) = field {
        if string.is_empty() {
            return Ok(0);
        }

        if let Ok(i) = string.parse::<u64>() {
            return Ok(i);
        }
    }

    Err(ParsingError)
}

fn to_float(field: Option<&String>) -> Result<f32> {
    if let Some(string) = field {
        if string.is_empty() {
            return Ok(0.0);
        }

        if let Ok(i) = string.parse::<f32>() {
            return Ok(i);
        }
    }

    Err(ParsingError)
}

fn to_bool(field: Option<&String>) -> Result<bool> {
    if let Some(string) = field {
        match string.as_str() {
            "0" => return Ok(false),
            "1" => return Ok(true),
            _ => return Err(ParsingError),
        }
    }

    Err(ParsingError)
}

fn to_bool_ch(field: Option<&String>) -> Result<bool> {
    if let Some(string) = field {
        match string.as_str() {
            "n" => return Ok(false),
            "y" => return Ok(true),
            _ => return Err(ParsingError),
        }
    }

    Err(ParsingError)
}

fn get_string(data: &[String], index: usize) -> Option<String> {
    if data.len() > index {
        Some(data[index].clone())
    } else {
        None
    }
}

fn get_prefix(string: &str, ch: char) -> &str {
    if let Some(end) = string.find(ch) {
        &string[0..end]
    } else {
        string
    }
}

impl KeysightPrefix {
    fn new(data: Vec<String>) -> Result<Self> {
        if let Some(first) = data.first() {
            match get_prefix(first, '\\') {
                // {@A-???|test status|measured value (|subtest designator)}
                "@A-CAP" | "@A-DIO" | "@A-FUS" | "@A-IND" | "@A-JUM" | "@A-MEA" | "@A-NFE"
                | "@A-PFE" | "@A-NPN" | "@A-PNP" | "@A-POT" | "@A-RES" | "@A-SWI" | "@A-ZEN" => {
                    Ok(KeysightPrefix::Analog(
                        data[0].as_str().into(),
                        to_int(data.get(1))?,
                        to_float(data.get(2))?,
                        get_string(&data, 3),
                    ))
                }

                // {@AID|time detected|serial number}
                "@AID" => {
                    if let Some(serial) = get_string(&data, 2) {
                        Ok(KeysightPrefix::AlarmId(to_uint(data.get(1))?, serial))
                    } else {
                        Err(ParsingError)
                    }
                }
                // {@ALM|alarm type|alarm status|time detected|board type|board type rev|alarm limit|detected value|controller|testhead number}
                "@ALM" => {
                    if data.len() < 10 {
                        return Err(ParsingError);
                    }

                    Ok(KeysightPrefix::Alarm(
                        to_int(data.get(1))?,
                        to_bool(data.get(2))?,
                        to_uint(data.get(3))?,
                        get_string(&data, 4).unwrap(),
                        get_string(&data, 5).unwrap(),
                        to_int(data.get(6))?,
                        to_int(data.get(7))?,
                        get_string(&data, 8).unwrap(),
                        to_int(data.get(9))?,
                    ))
                }

                // {@ARRAY|subtest designator|status|failure count|samples}
                "@ARRAY" => {
                    if data.len() < 5 {
                        return Err(ParsingError);
                    }

                    Ok(KeysightPrefix::Array(
                        get_string(&data, 1).unwrap(),
                        to_int(data.get(2))?,
                        to_int(data.get(3))?,
                        to_int(data.get(4))?,
                    ))
                }

                // {@BATCH|UUT type|UUT type rev|fixture id|testhead number|testhead type|process step|batch id|
                //      operator id|controller|testplan id|testplan rev|parent panel type|parent panel type rev (| version label)}
                "@BATCH" => {
                    if data.len() < 14 {
                        return Err(ParsingError);
                    }

                    Ok(KeysightPrefix::Batch(
                        get_string(&data, 1).unwrap(),
                        get_string(&data, 2).unwrap(),
                        to_int(data.get(3))?,
                        to_int(data.get(4))?,
                        get_string(&data, 5).unwrap(),
                        get_string(&data, 6).unwrap(),
                        get_string(&data, 7).unwrap(),
                        get_string(&data, 8).unwrap(),
                        get_string(&data, 9).unwrap(),
                        get_string(&data, 10).unwrap(),
                        get_string(&data, 11).unwrap(),
                        get_string(&data, 12).unwrap(),
                        get_string(&data, 13).unwrap(),
                        get_string(&data, 14),
                    ))
                }

                // {@BLOCK|block designator|block status}
                "@BLOCK" => {
                    if data.len() < 3 {
                        return Err(ParsingError);
                    }

                    Ok(KeysightPrefix::Block(
                        get_string(&data, 1).unwrap(),
                        to_int(data.get(2))?,
                    ))
                }

                // {@BS-CON|test designator|status|shorts count|opens count}
                "@BS-CON" => {
                    if data.len() < 5 {
                        return Err(ParsingError);
                    }

                    Ok(KeysightPrefix::Boundary(
                        get_string(&data, 1).unwrap(),
                        to_int(data.get(2))?,
                        to_int(data.get(3))?,
                        to_int(data.get(4))?,
                    ))
                }
                // {@BS-O|first device|first pin|second device|second pin}
                "@BS-O" => {
                    if data.len() < 5 {
                        return Err(ParsingError);
                    }

                    Ok(KeysightPrefix::BoundaryOpen(
                        get_string(&data, 1).unwrap(),
                        to_int(data.get(2))?,
                        get_string(&data, 3).unwrap(),
                        to_int(data.get(4))?,
                    ))
                }
                // {@BS-S|cause|node list}
                "@BS-S" => {
                    if data.len() < 3 {
                        return Err(ParsingError);
                    }

                    Ok(KeysightPrefix::BoundaryShort(
                        get_string(&data, 1).unwrap(),
                        get_string(&data, 2).unwrap(),
                    ))
                }

                // {@BTEST|board id|test status|start datetime|duration|multiple test|log level|log set|learning|
                // known good|end datetime|status qualifier|board number(|parent panel id)}
                "@BTEST" => {
                    if data.len() < 13 {
                        return Err(ParsingError);
                    }

                    Ok(KeysightPrefix::BTest(
                        get_string(&data, 1).unwrap(),
                        to_int(data.get(2))?,
                        to_uint(data.get(3))?,
                        to_int(data.get(4))?,
                        to_bool(data.get(5))?,
                        get_string(&data, 6).unwrap(),
                        to_int(data.get(7))?,
                        to_bool_ch(data.get(8))?,
                        to_bool_ch(data.get(9))?,
                        to_uint(data.get(10))?,
                        get_string(&data, 11).unwrap(),
                        to_int(data.get(12))?,
                        get_string(&data, 13),
                    ))
                }

                // {@CCHK|test status|pin count|test designator}
                "@CCHK" => {
                    if data.len() < 4 {
                        return Err(ParsingError);
                    }

                    Ok(KeysightPrefix::CChk(
                        to_int(data.get(1))?,
                        to_int(data.get(2))?,
                        get_string(&data, 3).unwrap(),
                    ))
                }

                // {@DPIN|device name|node pin list} or
                // {@DPIN|device name|node pin list|thru devnode list} with DriveThru
                "@DPIN" => {
                    if data.len() < 4 {
                        return Err(ParsingError);
                    }

                    let mut node_pin_list = Vec::new();
                    for i in (2..data.len()).step_by(2) {
                        node_pin_list.push((
                            get_string(&data, i).unwrap(),
                            get_string(&data, i + 1).unwrap(),
                        ));
                    }
                    Ok(KeysightPrefix::DPin(
                        get_prefix(&get_string(&data, 1).unwrap(), '\\').to_string(),
                        node_pin_list,
                    ))
                }

                // {@D-PLD|Filename|Action|Action return code|Result message string|Player program counter }
                "@D-PLD" => {
                    if data.len() < 6 {
                        return Err(ParsingError);
                    }

                    Ok(KeysightPrefix::DPld(
                        get_string(&data, 1).unwrap(),
                        get_string(&data, 2).unwrap(),
                        to_int(data.get(3))?,
                        get_string(&data, 4).unwrap(),
                        to_int(data.get(5))?,
                    ))
                }

                // {@EXPRT|Key|Field}
                "@EXPRT" => {
                    if data.len() < 3 {
                        return Err(ParsingError);
                    }

                    Ok(KeysightPrefix::Export(
                        get_string(&data, 1).unwrap(),
                        to_int(data.get(3))?,
                    ))
                }
                // {@NOTE|Note name|Note string}
                "@NOTE" => {
                    if data.len() < 3 {
                        return Err(ParsingError);
                    }

                    Ok(KeysightPrefix::Note(
                        get_string(&data, 1).unwrap(),
                        get_string(&data, 2).unwrap(),
                    ))
                }

                // {@D-T|test status|test substatus|failing vector number|pin count|test designator}
                "@D-T" => {
                    if data.len() < 6 {
                        return Err(ParsingError);
                    }

                    Ok(KeysightPrefix::Digital(
                        to_int(data.get(1))?,
                        to_int(data.get(2))?,
                        to_int(data.get(3))?,
                        to_int(data.get(4))?,
                        get_string(&data, 5).unwrap(),
                    ))
                }

                // {@INDICT|technique|device list} ex: {@INDICT|DT\3|rp6:r2|c412|r22}
                "@INDICT" => {
                    if data.len() < 2 {
                        return Err(ParsingError);
                    }

                    Ok(KeysightPrefix::Indict(
                        get_prefix(&data[1], '\\').to_string(),
                        data.iter().skip(2).cloned().collect(),
                    ))
                }

                // {@LIM2|high limit|low limit}
                // {@LIM3|nominal value|high limit|low limit}
                "@LIM2" => Ok(KeysightPrefix::Lim2(
                    to_float(data.get(1))?,
                    to_float(data.get(2))?,
                )),
                "@LIM3" => Ok(KeysightPrefix::Lim3(
                    to_float(data.get(1))?,
                    to_float(data.get(2))?,
                    to_float(data.get(3))?,
                )),

                // {@NETV|datetime|test system|repair system|source}
                "@NETV" => {
                    if data.len() < 5 {
                        return Err(ParsingError);
                    }

                    Ok(KeysightPrefix::NetV(
                        to_uint(data.get(1))?,
                        get_string(&data, 2).unwrap(),
                        get_string(&data, 3).unwrap(),
                        to_bool(data.get(4))?,
                    ))
                }

                // {@NODE\node list}
                "@NODE" => Ok(KeysightPrefix::Node(data.iter().skip(1).cloned().collect())),

                // {@PCHK|test status|test designator}
                "@PCHK" => {
                    if data.len() < 3 {
                        return Err(ParsingError);
                    }

                    Ok(KeysightPrefix::PChk(
                        to_int(data.get(1))?,
                        get_string(&data, 2).unwrap(),
                    ))
                }

                // {@PF|designator|test status|total pins}
                "@PF" => {
                    if data.len() < 4 {
                        return Err(ParsingError);
                    }

                    Ok(KeysightPrefix::Pins(
                        get_string(&data, 1).unwrap(),
                        to_int(data.get(2))?,
                        to_int(data.get(3))?,
                    ))
                }
                // {@PIN\pin list}
                "@PIN" => Ok(KeysightPrefix::Pin(data.iter().skip(1).cloned().collect())),

                // {@PRB|test status|pin count|test designator}
                "@PRB" => {
                    if data.len() < 4 {
                        return Err(ParsingError);
                    }

                    Ok(KeysightPrefix::Prb(
                        to_int(data.get(1))?,
                        to_int(data.get(2))?,
                        get_string(&data, 3).unwrap(),
                    ))
                }

                // {@RETEST|datetime}
                "@RETEST" => Ok(KeysightPrefix::Retest(to_uint(data.get(1))?)),

                // {@RPT|message}
                "@RPT" => {
                    if let Some(string) = get_string(&data, 1) {
                        Ok(KeysightPrefix::Report(string))
                    } else {
                        Err(ParsingError)
                    }
                }

                // {@TJET|test status|pin count|test designator}
                "@TJET" => {
                    if data.len() < 4 {
                        return Err(ParsingError);
                    }

                    Ok(KeysightPrefix::TJet(
                        to_int(data.get(1))?,
                        to_int(data.get(2))?,
                        get_string(&data, 3).unwrap(),
                    ))
                }

                // {@TS|test status|shorts count|opens count|phantoms count (|designator) }
                // Shorts(i32, i32, i32, i32, Option<String>),
                "@TS" => Ok(KeysightPrefix::Shorts(
                    to_int(data.get(1))?,
                    to_int(data.get(2))?,
                    to_int(data.get(3))?,
                    to_int(data.get(4))?,
                    get_string(&data, 5),
                )),

                // {@TS-S|shorts count|phantoms count|source node}  short source
                // {@TS-D\destination list}                         short destination
                // {@TS-P|deviation}                                phantom shorts
                // {@TS-O|source node|destination node|deviation}   opens
                "@TS-S" => {
                    if data.len() < 4 {
                        return Err(ParsingError);
                    }

                    Ok(KeysightPrefix::ShortsSrc(
                        to_int(data.get(1))?,
                        to_int(data.get(2))?,
                        get_string(&data, 3).unwrap(),
                    ))
                }

                "@TS-D" => {
                    let mut dest_list = Vec::new();
                    for i in (1..data.len()).step_by(2) {
                        dest_list.push((get_string(&data, i).unwrap(), to_float(data.get(i + 1))?));
                    }

                    Ok(KeysightPrefix::ShortsDest(dest_list))
                }

                "@TS-P" => Ok(KeysightPrefix::ShortsPhantom(to_float(data.get(1))?)),

                "@TS-O" => {
                    if data.len() < 4 {
                        return Err(ParsingError);
                    }

                    Ok(KeysightPrefix::ShortsOpen(
                        get_string(&data, 1).unwrap(),
                        get_string(&data, 2).unwrap(),
                        to_float(data.get(3))?,
                    ))
                }

                _ => Ok(KeysightPrefix::UserDefined(data)),
            }
        } else {
            Err(ParsingError)
        }
    }
}

#[derive(Debug)]
pub struct TreeNode {
    pub data: KeysightPrefix,
    pub branches: Vec<TreeNode>,
}

impl TreeNode {
    fn read(buffer: &mut Chars) -> Self {
        let mut branches: Vec<TreeNode> = Vec::new();
        let mut data_buff: String = String::new();

        loop {
            let c = buffer.next();
            if c.is_none() || c.is_some_and(|f| f == '}') {
                break;
            }

            let c = c.unwrap();
            if c == '{' {
                branches.push(TreeNode::read(buffer));
            } else if c != '\n' {
                data_buff.push(c);
            }
        }

        if let Ok(data) =
            KeysightPrefix::new(data_buff.split('|').map(|f| f.trim().to_string()).collect())
        {
            TreeNode { data, branches }
        } else {
            TreeNode {
                data: KeysightPrefix::Error(data_buff),
                branches,
            }
        }
    }
}

pub fn parse_file(path: &Path) -> io::Result<Vec<TreeNode>> {
    let file = fs::read_to_string(path)?;
    let mut buffer = file.chars();

    let mut tree: Vec<TreeNode> = Vec::new();
    loop {
        let c = buffer.next();
        if c.is_none() {
            break;
        }

        if c.is_some_and(|f| f != '{') {
            continue;
        }

        tree.push(TreeNode::read(&mut buffer));
    }

    Ok(tree)
}
