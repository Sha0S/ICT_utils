use std::env;
use std::io::prelude::*;
use std::net::TcpStream;

use ICT_config::*;

static CONFIG: &str = "config.ini";

/*
1) At the start of the ICT test, check if the board is "testable".
    Params: START {Main DMC of the board} {Number of noards on the panel}
    Return message from the traceability-server:
        a) Panel is golden sample: "GS"
        b) Panel is testable: "OK"
        c) Panel is not testable: "NK"
        d) System error: "ER: {error message}"

2) At the end of the ICT test, send the paths of the logs to be processed.
    Params: END {list of paths for the logs}
    Return message from the traceability-server:
        a) Upload is succesfull: "OK"
        b) Upload failed: "ER: {error message}"
*/

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();
    
    // The current working directory will be not the directory of the executable,
    // So we will need to make absolut paths for .\config and .\golden_samples
    let exe_path = env::current_exe().expect("ER: Can't read the directory of the executable!"); // Shouldn't fail.

    // Read configuration
    let config = match Config::read(exe_path.with_file_name(CONFIG)) {
        Ok(c) => c,
        Err(e) => {
            println!("{e}");
            std::process::exit(0)
        }
    };

    if config.get_MES_server().is_empty() {
        println!("ER: Configuartion is missing the adress of the MES server!")
    }

    let mut stream = TcpStream::connect(config.get_MES_server())?;

    let tokens: Vec<&str> = args.iter().skip(1).map(|f| f.trim() ).collect(); 
    let message = tokens.join("|");

    stream.write_all(message.as_bytes())?;

    let mut buf = [0;1024];
    if stream.read(&mut buf).is_ok() {
        let message = String::from_utf8_lossy(&buf).trim_end_matches('\0').to_string();
        println!("{message}");
    } else {
        println!("ER: Failed to read response from server!");
    }

    Ok(())
}