//! Command line entry of the Ganbaru sync relay.

use ganbaru_sync_relay::config::{self, Command, USAGE};
use std::process::ExitCode;

fn main() -> ExitCode {
    let command = match config::parse(std::env::args().skip(1)) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("{error}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    if command == Command::Help {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("start relay runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    let result = runtime.block_on(async move {
        match command {
            Command::Serve(config) => ganbaru_sync_relay::serve(config).await,
            Command::Setup(config) => ganbaru_sync_relay::setup(config)
                .await
                .map(|text| print!("{text}")),
            Command::Spaces(data_dir) => ganbaru_sync_relay::list_spaces(&data_dir)
                .await
                .map(|text| print!("{text}")),
            Command::RemoveSpace(config) => ganbaru_sync_relay::remove_space(config)
                .await
                .map(|text| print!("{text}")),
            Command::RevokeSetup(config) => ganbaru_sync_relay::revoke_setup(config)
                .await
                .map(|text| print!("{text}")),
            Command::Help => Ok(()),
        }
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
