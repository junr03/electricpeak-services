use std::process::ExitCode;

use electricpeak_status::{Config, collect, write_snapshot};

fn main() -> ExitCode {
    let config = Config::from_env();
    let snapshot = collect(&config);
    if std::env::args().any(|arg| arg == "--stdout") {
        match serde_json::to_string_pretty(&snapshot) {
            Ok(json) => println!("{json}"),
            Err(error) => {
                eprintln!("electricpeak-status: {error}");
                return ExitCode::FAILURE;
            }
        }
        return ExitCode::SUCCESS;
    }
    match write_snapshot(&config.output, &snapshot) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("electricpeak-status: {error}");
            ExitCode::FAILURE
        }
    }
}
