//! Command line parsing. The relay takes a few flags, so they are parsed by hand instead of
//! through an argument parser dependency.

use crate::store::{
    DEFAULT_MAX_AGE, DEFAULT_QUOTA_BYTES, MIN_MAX_AGE, MIN_QUOTA_BYTES, RetentionPolicy,
};
use ganbaru_handoff::protocol::relay::RELAY_PORT;
use ganbaru_sync_contracts::bounds::MAX_SERVER_ENDPOINTS;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

/// File name of the relay database in the data directory.
pub const DATABASE_FILE: &str = "relay.sqlite";
const MIB: u64 = 1024 * 1024;
const SECONDS_PER_DAY: u64 = 24 * 60 * 60;

/// Usage text printed for `help` and after a parse error.
pub const USAGE: &str = "\
usage:
  ganbaru-sync-relay serve --data-dir <path> [--listen <address:port>] [--quota-mib <n>] [--max-age-days <n>]
  ganbaru-sync-relay setup --data-dir <path> --endpoint <host:port> [--endpoint <host:port> ...]
  ganbaru-sync-relay help

serve   runs the relay. The quota applies to each space.
setup   prints a one-time setup code that lets one device claim a space on this relay.";

/// Settings of the `serve` subcommand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServeConfig {
    /// Directory holding the database and the TLS identity.
    pub data_dir: PathBuf,
    /// Address the relay listens on.
    pub listen: SocketAddr,
    /// Storage limits of each space.
    pub retention: RetentionPolicy,
}

/// Settings of the `setup` subcommand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupConfig {
    /// Directory holding the database and the TLS identity.
    pub data_dir: PathBuf,
    /// Addresses devices use to reach the relay, as `host:port`.
    pub endpoints: Vec<String>,
}

/// A parsed command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Runs the relay.
    Serve(ServeConfig),
    /// Prints a setup code.
    Setup(SetupConfig),
    /// Prints usage.
    Help,
}

/// The default listen address: every IPv4 interface on [`RELAY_PORT`].
pub fn default_listen() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), RELAY_PORT)
}

/// Parses the arguments after the program name.
pub fn parse<I>(arguments: I) -> Result<Command, String>
where
    I: IntoIterator<Item = String>,
{
    let mut arguments = arguments.into_iter();
    let Some(subcommand) = arguments.next() else {
        return Ok(Command::Help);
    };
    let flags = parse_flags(arguments)?;
    match subcommand.as_str() {
        "serve" => parse_serve(flags).map(Command::Serve),
        "setup" => parse_setup(flags).map(Command::Setup),
        "help" | "--help" | "-h" => Ok(Command::Help),
        _ => Err(format!("unknown subcommand {subcommand:?}")),
    }
}

fn parse_flags(arguments: impl Iterator<Item = String>) -> Result<Vec<(String, String)>, String> {
    let mut flags = Vec::new();
    let mut arguments = arguments;
    while let Some(flag) = arguments.next() {
        if !flag.starts_with("--") {
            return Err(format!("unexpected argument {flag:?}"));
        }
        let Some(value) = arguments.next() else {
            return Err(format!("{flag} needs a value"));
        };
        flags.push((flag, value));
    }
    Ok(flags)
}

fn single(flags: &[(String, String)], name: &str) -> Result<Option<String>, String> {
    let mut values = flags.iter().filter(|(flag, _)| flag == name);
    let first = values.next().map(|(_, value)| value.clone());
    if values.next().is_some() {
        return Err(format!("{name} is given more than once"));
    }
    Ok(first)
}

fn reject_unknown(flags: &[(String, String)], known: &[&str]) -> Result<(), String> {
    match flags
        .iter()
        .find(|(flag, _)| !known.contains(&flag.as_str()))
    {
        Some((flag, _)) => Err(format!("unknown option {flag}")),
        None => Ok(()),
    }
}

fn data_dir(flags: &[(String, String)]) -> Result<PathBuf, String> {
    match single(flags, "--data-dir")? {
        Some(path) if !path.is_empty() => Ok(PathBuf::from(path)),
        _ => Err("--data-dir is required".to_string()),
    }
}

fn parse_count(name: &str, value: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|_| format!("{name} must be a whole number"))
}

fn parse_serve(flags: Vec<(String, String)>) -> Result<ServeConfig, String> {
    reject_unknown(
        &flags,
        &["--data-dir", "--listen", "--quota-mib", "--max-age-days"],
    )?;
    let listen = match single(&flags, "--listen")? {
        Some(value) => value.parse::<SocketAddr>().map_err(|_| {
            "--listen must be an address and port, such as 0.0.0.0:43823".to_string()
        })?,
        None => default_listen(),
    };
    let quota_bytes = match single(&flags, "--quota-mib")? {
        Some(value) => parse_count("--quota-mib", &value)?
            .checked_mul(MIB)
            .ok_or_else(|| "--quota-mib is too large".to_string())?,
        None => DEFAULT_QUOTA_BYTES,
    };
    if quota_bytes < MIN_QUOTA_BYTES {
        return Err(format!(
            "--quota-mib must be at least {}",
            MIN_QUOTA_BYTES / MIB
        ));
    }
    let max_age = match single(&flags, "--max-age-days")? {
        Some(value) => Duration::from_secs(
            parse_count("--max-age-days", &value)?
                .checked_mul(SECONDS_PER_DAY)
                .ok_or_else(|| "--max-age-days is too large".to_string())?,
        ),
        None => DEFAULT_MAX_AGE,
    };
    if max_age < MIN_MAX_AGE {
        return Err(format!(
            "--max-age-days must be at least {}",
            MIN_MAX_AGE.as_secs() / SECONDS_PER_DAY
        ));
    }
    Ok(ServeConfig {
        data_dir: data_dir(&flags)?,
        listen,
        retention: RetentionPolicy {
            quota_bytes,
            max_age,
        },
    })
}

fn parse_setup(flags: Vec<(String, String)>) -> Result<SetupConfig, String> {
    reject_unknown(&flags, &["--data-dir", "--endpoint"])?;
    let endpoints: Vec<String> = flags
        .iter()
        .filter(|(flag, _)| flag == "--endpoint")
        .map(|(_, value)| value.clone())
        .collect();
    if endpoints.is_empty() || endpoints.len() > MAX_SERVER_ENDPOINTS {
        return Err(format!(
            "give from 1 to {MAX_SERVER_ENDPOINTS} --endpoint values as host:port"
        ));
    }
    for endpoint in &endpoints {
        if !ganbaru_sync_contracts::authorization::validate_server_endpoint(endpoint) {
            return Err(format!("{endpoint:?} is not a valid host:port endpoint"));
        }
    }
    Ok(SetupConfig {
        data_dir: data_dir(&flags)?,
        endpoints,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn serve_uses_defaults_and_accepts_overrides() {
        let Command::Serve(config) = parse(args("serve --data-dir /srv/relay")).unwrap() else {
            panic!("expected serve");
        };
        assert_eq!(config.listen, default_listen());
        assert_eq!(config.retention.quota_bytes, DEFAULT_QUOTA_BYTES);
        assert_eq!(config.retention.max_age, DEFAULT_MAX_AGE);
        let Command::Serve(config) = parse(args(
            "serve --data-dir d --listen 127.0.0.1:9000 --quota-mib 100 --max-age-days 7",
        ))
        .unwrap() else {
            panic!("expected serve");
        };
        assert_eq!(config.listen, "127.0.0.1:9000".parse().unwrap());
        assert_eq!(config.retention.quota_bytes, 100 * MIB);
        assert_eq!(
            config.retention.max_age,
            Duration::from_secs(7 * SECONDS_PER_DAY)
        );
    }

    #[test]
    fn serve_refuses_limits_below_their_floors_and_unknown_or_repeated_flags() {
        assert!(parse(args("serve --data-dir d --quota-mib 1")).is_err());
        assert!(parse(args("serve --data-dir d --max-age-days 0")).is_err());
        assert!(parse(args("serve --data-dir d --quota-mib 99999999999999999")).is_err());
        assert!(parse(args("serve --data-dir d --port 1")).is_err());
        assert!(parse(args("serve --data-dir d --data-dir e")).is_err());
        assert!(parse(args("serve --data-dir")).is_err());
        assert!(parse(args("serve")).is_err());
    }

    #[test]
    fn setup_needs_valid_endpoints_within_the_bound() {
        let Command::Setup(config) = parse(args(
            "setup --data-dir d --endpoint relay.example.org:43823 --endpoint 192.0.2.4:43823",
        ))
        .unwrap() else {
            panic!("expected setup");
        };
        assert_eq!(config.endpoints.len(), 2);
        assert!(parse(args("setup --data-dir d")).is_err());
        assert!(parse(args("setup --data-dir d --endpoint nope")).is_err());
        let many = format!(
            "setup --data-dir d{}",
            " --endpoint a.example:1".repeat(MAX_SERVER_ENDPOINTS + 1)
        );
        assert!(parse(args(&many)).is_err());
    }

    #[test]
    fn no_arguments_or_help_prints_usage() {
        assert_eq!(parse(Vec::new()).unwrap(), Command::Help);
        assert_eq!(parse(args("help")).unwrap(), Command::Help);
        assert!(parse(args("launch")).is_err());
    }
}
