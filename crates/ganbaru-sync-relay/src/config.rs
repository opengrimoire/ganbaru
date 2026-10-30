//! Command line parsing. The relay takes a few flags, so they are parsed by hand instead of
//! through an argument parser dependency.

use crate::store::{
    DEFAULT_MAX_AGE, DEFAULT_QUOTA_BYTES, MIN_MAX_AGE, MIN_QUOTA_BYTES, RetentionPolicy,
};
use ganbaru_handoff::protocol::relay::{RELAY_PORT, RelaySetupCode, RelaySetupToken};
use ganbaru_sync_contracts::SpaceId;
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
  ganbaru-sync-relay spaces --data-dir <path>
  ganbaru-sync-relay remove-space --data-dir <path> --space <space id>
  ganbaru-sync-relay revoke-setup --data-dir <path> [--code <setup code>]
  ganbaru-sync-relay help

serve         runs the relay. The quota applies to each space.
setup         prints a one-time setup code that lets one device claim a space on this relay.
spaces        lists the claimed spaces with their epoch and stored bytes.
remove-space  deletes a space and everything stored for it. Stop the relay first.
revoke-setup  revokes one unused setup code, or every unused one when --code is omitted.";

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

/// Settings of the `remove-space` subcommand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoveSpaceConfig {
    /// Directory holding the database.
    pub data_dir: PathBuf,
    /// Space to delete.
    pub space: SpaceId,
}

/// Settings of the `revoke-setup` subcommand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevokeSetupConfig {
    /// Directory holding the database.
    pub data_dir: PathBuf,
    /// Token of the setup code to revoke, or `None` to revoke every unused one.
    pub token: Option<RelaySetupToken>,
}

/// A parsed command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Runs the relay.
    Serve(ServeConfig),
    /// Prints a setup code.
    Setup(SetupConfig),
    /// Lists the claimed spaces of the relay in a data directory.
    Spaces(PathBuf),
    /// Deletes one space.
    RemoveSpace(RemoveSpaceConfig),
    /// Revokes unused setup codes.
    RevokeSetup(RevokeSetupConfig),
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
        "spaces" => {
            reject_unknown(&flags, &["--data-dir"])?;
            data_dir(&flags).map(Command::Spaces)
        }
        "remove-space" => parse_remove_space(flags).map(Command::RemoveSpace),
        "revoke-setup" => parse_revoke_setup(flags).map(Command::RevokeSetup),
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

fn parse_remove_space(flags: Vec<(String, String)>) -> Result<RemoveSpaceConfig, String> {
    reject_unknown(&flags, &["--data-dir", "--space"])?;
    let space = single(&flags, "--space")?.ok_or_else(|| "--space is required".to_string())?;
    let space = SpaceId::from_hex(space.trim())
        .ok_or_else(|| "--space must be a space id as printed by the spaces command".to_string())?;
    Ok(RemoveSpaceConfig {
        data_dir: data_dir(&flags)?,
        space,
    })
}

fn parse_revoke_setup(flags: Vec<(String, String)>) -> Result<RevokeSetupConfig, String> {
    reject_unknown(&flags, &["--data-dir", "--code"])?;
    let token = match single(&flags, "--code")? {
        Some(text) => Some(
            RelaySetupCode::from_text(&text)
                .map_err(|_| {
                    "--code must be a setup code printed by the setup command".to_string()
                })?
                .token,
        ),
        None => None,
    };
    Ok(RevokeSetupConfig {
        data_dir: data_dir(&flags)?,
        token,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operator_commands_parse_their_targets() {
        assert_eq!(
            parse(args("spaces --data-dir d")).unwrap(),
            Command::Spaces(PathBuf::from("d"))
        );
        assert!(parse(args("spaces --data-dir d --space x")).is_err());
        let space = SpaceId::from_bytes([7; 16]);
        let Command::RemoveSpace(config) = parse(args(&format!(
            "remove-space --data-dir d --space {}",
            space.to_hex()
        )))
        .unwrap() else {
            panic!("expected remove-space");
        };
        assert_eq!(config.space, space);
        assert!(parse(args("remove-space --data-dir d")).is_err());
        assert!(parse(args("remove-space --data-dir d --space 0102")).is_err());

        let code = RelaySetupCode {
            endpoints: vec!["relay.example.org:43823".to_string()],
            certificate_fingerprint: [3; 32],
            token: RelaySetupToken::from_bytes([9; 32]),
        };
        let Command::RevokeSetup(config) = parse(args(&format!(
            "revoke-setup --data-dir d --code {}",
            code.to_text().unwrap()
        )))
        .unwrap() else {
            panic!("expected revoke-setup");
        };
        assert_eq!(config.token, Some(code.token));
        let Command::RevokeSetup(config) = parse(args("revoke-setup --data-dir d")).unwrap() else {
            panic!("expected revoke-setup");
        };
        assert_eq!(config.token, None);
        assert!(parse(args("revoke-setup --data-dir d --code nope")).is_err());
    }

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
