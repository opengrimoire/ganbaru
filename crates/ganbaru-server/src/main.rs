//! Command line entry of the Ganbaru self-hosted server.

use ganbaru_server::config::{self, Command, DATA_DIR_ENV, USAGE, VERSION};
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let default_data_dir = std::env::var_os(DATA_DIR_ENV).map(PathBuf::from);
    let command = match config::parse(std::env::args().skip(1), default_data_dir) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("{error}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match command {
        Command::Help => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Command::Version => {
            println!("ganbaru-server {VERSION}");
            return ExitCode::SUCCESS;
        }
        _ => {}
    }
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("start server runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    let result = runtime.block_on(async move {
        match command {
            Command::Relay(config) => ganbaru_server::serve(config).await,
            Command::RelaySetup(config) => ganbaru_server::setup(config)
                .await
                .map(|text| print!("{text}")),
            Command::RelaySpaces(data_dir) => ganbaru_server::list_spaces(&data_dir)
                .await
                .map(|text| print!("{text}")),
            Command::RelayRemoveSpace(config) => ganbaru_server::remove_space(config)
                .await
                .map(|text| print!("{text}")),
            Command::RelayRevokeSetup(config) => ganbaru_server::revoke_setup(config)
                .await
                .map(|text| print!("{text}")),
            Command::Anchor(config) => ganbaru_server::anchor::run(config).await,
            Command::AnchorSetup(config) => {
                ganbaru_server::anchor::setup(&config).map(|text| print!("{text}"))
            }
            Command::AnchorBackup(config) => ganbaru_server::anchor::backup(config)
                .await
                .map(|text| print!("{text}")),
            Command::Help | Command::Version => Ok(()),
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
