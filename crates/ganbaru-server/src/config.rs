//! Command line parsing. The server takes a few flags, so they are parsed by hand instead of
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

/// Environment variable that names the data directory when `--data-dir` is not given.
pub const DATA_DIR_ENV: &str = "GANBARU_SERVER_DATA_DIR";

/// The release this binary was built for, or the crate version outside a release build.
pub const VERSION: &str = match option_env!("GANBARU_RELEASE_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

/// Usage text printed for `help` and after a parse error.
pub const USAGE: &str = "\
usage: ganbaru-server <anchor | relay> [action] [options]

Every command takes --data-dir <path>, or reads it from GANBARU_SERVER_DATA_DIR.
Use one data directory per server and one mode per data directory.

Anchor: an always-on copy of one vault. It can read the vault, so run it on hardware you own.
  ganbaru-server anchor [--listen <address:port>]
  ganbaru-server anchor setup --endpoint <host:port> [--endpoint <host:port> ...]
  ganbaru-server anchor backup --out <file>

Relay: holds encrypted changes for devices that are not online together. It cannot read them.
  ganbaru-server relay [--listen <address:port>] [--quota-mib <n>] [--max-age-days <n>]
  ganbaru-server relay setup --endpoint <host:port> [--endpoint <host:port> ...]
  ganbaru-server relay spaces
  ganbaru-server relay remove-space --space <space id>
  ganbaru-server relay revoke-setup [--code <setup code>]

  ganbaru-server version
  ganbaru-server help

anchor               runs the anchor on 0.0.0.0:43823 unless --listen is given.
anchor setup         prints a setup code, valid for 15 minutes and one vault, to paste in a desktop:
                     one that adds this server, or a new one that joins through it.
anchor backup        writes an encrypted backup that opens with the recovery kit. Stop the anchor first.
relay                runs the relay on 0.0.0.0:43823 unless --listen is given. The quota applies to each vault.
relay setup          prints a one-time setup code that lets one desktop add this relay.
relay spaces         lists the vaults this relay stores, with their epoch and stored bytes.
relay remove-space   deletes one vault and everything stored for it. Stop the relay first.
relay revoke-setup   revokes one unused setup code, or every unused one when --code is omitted.

Setup commands work while the server runs. Each --endpoint is an address your devices can reach.";

/// Settings of the `relay` command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServeConfig {
    /// Directory holding the database and the TLS identity.
    pub data_dir: PathBuf,
    /// Address the relay listens on.
    pub listen: SocketAddr,
    /// Storage limits of each space.
    pub retention: RetentionPolicy,
}

/// Settings of the `relay setup` and `anchor setup` commands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupConfig {
    /// Directory holding the database and the TLS identity.
    pub data_dir: PathBuf,
    /// Addresses devices use to reach the relay, as `host:port`.
    pub endpoints: Vec<String>,
}

/// Settings of the `relay remove-space` command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoveSpaceConfig {
    /// Directory holding the database.
    pub data_dir: PathBuf,
    /// Space to delete.
    pub space: SpaceId,
}

/// Settings of the `relay revoke-setup` command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevokeSetupConfig {
    /// Directory holding the database.
    pub data_dir: PathBuf,
    /// Token of the setup code to revoke, or `None` to revoke every unused one.
    pub token: Option<RelaySetupToken>,
}

/// Settings of the `anchor` command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnchorConfig {
    /// Directory holding the replica, its keys, and its identity.
    pub data_dir: PathBuf,
    /// Address the anchor listens on.
    pub listen: SocketAddr,
}

/// Settings of the `anchor setup` command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnchorSetupConfig {
    /// Directory of the anchor.
    pub data_dir: PathBuf,
    /// Addresses devices use to reach the anchor, as `host:port`.
    pub endpoints: Vec<String>,
}

/// Settings of the `anchor backup` command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnchorBackupConfig {
    /// Directory of the anchor.
    pub data_dir: PathBuf,
    /// File the backup is written to.
    pub out: PathBuf,
}

/// A parsed command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Runs an anchor.
    Anchor(AnchorConfig),
    /// Prints an anchor setup code.
    AnchorSetup(AnchorSetupConfig),
    /// Writes an encrypted backup of an anchor's vault.
    AnchorBackup(AnchorBackupConfig),
    /// Runs the relay.
    Relay(ServeConfig),
    /// Prints a relay setup code.
    RelaySetup(SetupConfig),
    /// Lists the claimed spaces of the relay in a data directory.
    RelaySpaces(PathBuf),
    /// Deletes one relay space.
    RelayRemoveSpace(RemoveSpaceConfig),
    /// Revokes unused relay setup codes.
    RelayRevokeSetup(RevokeSetupConfig),
    /// Prints the version.
    Version,
    /// Prints usage.
    Help,
}

/// The default listen address: every IPv4 interface on [`RELAY_PORT`].
pub fn default_listen() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), RELAY_PORT)
}

/// Options of one command, with the data directory a missing `--data-dir` falls back to.
struct Flags {
    values: Vec<(String, String)>,
    default_data_dir: Option<PathBuf>,
}

/// Parses the arguments after the program name. `default_data_dir` is the directory
/// [`DATA_DIR_ENV`] names, used when a command has no `--data-dir`.
pub fn parse<I>(arguments: I, default_data_dir: Option<PathBuf>) -> Result<Command, String>
where
    I: IntoIterator<Item = String>,
{
    let mut arguments = arguments.into_iter().peekable();
    let Some(mode) = arguments.next() else {
        return Ok(Command::Help);
    };
    let action = arguments.next_if(|word| !word.starts_with("--"));
    let flags = Flags {
        values: parse_flags(arguments)?,
        default_data_dir: default_data_dir.filter(|path| !path.as_os_str().is_empty()),
    };
    match (mode.as_str(), action.as_deref()) {
        ("anchor", None) => parse_anchor(flags).map(Command::Anchor),
        ("anchor", Some("setup")) => parse_setup(flags)
            .map(|setup| AnchorSetupConfig {
                data_dir: setup.data_dir,
                endpoints: setup.endpoints,
            })
            .map(Command::AnchorSetup),
        ("anchor", Some("backup")) => parse_anchor_backup(flags).map(Command::AnchorBackup),
        ("relay", None) => parse_serve(flags).map(Command::Relay),
        ("relay", Some("setup")) => parse_setup(flags).map(Command::RelaySetup),
        ("relay", Some("spaces")) => {
            reject_unknown(&flags, &["--data-dir"])?;
            data_dir(&flags).map(Command::RelaySpaces)
        }
        ("relay", Some("remove-space")) => parse_remove_space(flags).map(Command::RelayRemoveSpace),
        ("relay", Some("revoke-setup")) => parse_revoke_setup(flags).map(Command::RelayRevokeSetup),
        ("anchor" | "relay", Some(other)) => Err(format!("unknown {mode} action {other:?}")),
        ("version" | "--version" | "-V", None) if flags.values.is_empty() => Ok(Command::Version),
        ("help" | "--help" | "-h", None) if flags.values.is_empty() => Ok(Command::Help),
        _ => Err(format!("unknown command {mode:?}")),
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

fn single(flags: &Flags, name: &str) -> Result<Option<String>, String> {
    let mut values = flags.values.iter().filter(|(flag, _)| flag == name);
    let first = values.next().map(|(_, value)| value.clone());
    if values.next().is_some() {
        return Err(format!("{name} is given more than once"));
    }
    Ok(first)
}

fn reject_unknown(flags: &Flags, known: &[&str]) -> Result<(), String> {
    match flags
        .values
        .iter()
        .find(|(flag, _)| !known.contains(&flag.as_str()))
    {
        Some((flag, _)) => Err(format!("unknown option {flag}")),
        None => Ok(()),
    }
}

fn data_dir(flags: &Flags) -> Result<PathBuf, String> {
    match single(flags, "--data-dir")? {
        Some(path) if !path.is_empty() => Ok(PathBuf::from(path)),
        Some(_) => Err("--data-dir needs a path".to_string()),
        None => flags
            .default_data_dir
            .clone()
            .ok_or_else(|| format!("give --data-dir <path> or set {DATA_DIR_ENV}")),
    }
}

fn parse_count(name: &str, value: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|_| format!("{name} must be a whole number"))
}

fn parse_listen(flags: &Flags) -> Result<SocketAddr, String> {
    match single(flags, "--listen")? {
        Some(value) => value
            .parse::<SocketAddr>()
            .map_err(|_| "--listen must be an address and port, such as 0.0.0.0:43823".to_string()),
        None => Ok(default_listen()),
    }
}

fn parse_anchor(flags: Flags) -> Result<AnchorConfig, String> {
    reject_unknown(&flags, &["--data-dir", "--listen"])?;
    Ok(AnchorConfig {
        data_dir: data_dir(&flags)?,
        listen: parse_listen(&flags)?,
    })
}

fn parse_anchor_backup(flags: Flags) -> Result<AnchorBackupConfig, String> {
    reject_unknown(&flags, &["--data-dir", "--out"])?;
    match single(&flags, "--out")? {
        Some(out) if !out.is_empty() => Ok(AnchorBackupConfig {
            data_dir: data_dir(&flags)?,
            out: PathBuf::from(out),
        }),
        _ => Err("--out is required".to_string()),
    }
}

fn parse_serve(flags: Flags) -> Result<ServeConfig, String> {
    reject_unknown(
        &flags,
        &["--data-dir", "--listen", "--quota-mib", "--max-age-days"],
    )?;
    let listen = parse_listen(&flags)?;
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

fn parse_setup(flags: Flags) -> Result<SetupConfig, String> {
    reject_unknown(&flags, &["--data-dir", "--endpoint"])?;
    let endpoints: Vec<String> = flags
        .values
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

fn parse_remove_space(flags: Flags) -> Result<RemoveSpaceConfig, String> {
    reject_unknown(&flags, &["--data-dir", "--space"])?;
    let space = single(&flags, "--space")?.ok_or_else(|| "--space is required".to_string())?;
    let space = SpaceId::from_hex(space.trim())
        .ok_or_else(|| "--space must be a space id as printed by the spaces command".to_string())?;
    Ok(RemoveSpaceConfig {
        data_dir: data_dir(&flags)?,
        space,
    })
}

fn parse_revoke_setup(flags: Flags) -> Result<RevokeSetupConfig, String> {
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

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_string).collect()
    }

    fn parse_text(text: &str) -> Result<Command, String> {
        parse(args(text), None)
    }

    #[test]
    fn relay_operator_commands_parse_their_targets() {
        assert_eq!(
            parse_text("relay spaces --data-dir d").unwrap(),
            Command::RelaySpaces(PathBuf::from("d"))
        );
        assert!(parse_text("relay spaces --data-dir d --space x").is_err());
        let space = SpaceId::from_bytes([7; 16]);
        let Command::RelayRemoveSpace(config) = parse_text(&format!(
            "relay remove-space --data-dir d --space {}",
            space.to_hex()
        ))
        .unwrap() else {
            panic!("expected relay remove-space");
        };
        assert_eq!(config.space, space);
        assert!(parse_text("relay remove-space --data-dir d").is_err());
        assert!(parse_text("relay remove-space --data-dir d --space 0102").is_err());

        let code = RelaySetupCode {
            endpoints: vec!["relay.example.org:43823".to_string()],
            certificate_fingerprint: [3; 32],
            token: RelaySetupToken::from_bytes([9; 32]),
        };
        let Command::RelayRevokeSetup(config) = parse_text(&format!(
            "relay revoke-setup --data-dir d --code {}",
            code.to_text().unwrap()
        ))
        .unwrap() else {
            panic!("expected relay revoke-setup");
        };
        assert_eq!(config.token, Some(code.token));
        let Command::RelayRevokeSetup(config) =
            parse_text("relay revoke-setup --data-dir d").unwrap()
        else {
            panic!("expected relay revoke-setup");
        };
        assert_eq!(config.token, None);
        assert!(parse_text("relay revoke-setup --data-dir d --code nope").is_err());
    }

    #[test]
    fn relay_uses_defaults_and_accepts_overrides() {
        let Command::Relay(config) = parse_text("relay --data-dir /srv/relay").unwrap() else {
            panic!("expected relay");
        };
        assert_eq!(config.listen, default_listen());
        assert_eq!(config.retention.quota_bytes, DEFAULT_QUOTA_BYTES);
        assert_eq!(config.retention.max_age, DEFAULT_MAX_AGE);
        let Command::Relay(config) = parse_text(
            "relay --data-dir d --listen 127.0.0.1:9000 --quota-mib 100 --max-age-days 7",
        )
        .unwrap() else {
            panic!("expected relay");
        };
        assert_eq!(config.listen, "127.0.0.1:9000".parse().unwrap());
        assert_eq!(config.retention.quota_bytes, 100 * MIB);
        assert_eq!(
            config.retention.max_age,
            Duration::from_secs(7 * SECONDS_PER_DAY)
        );
    }

    #[test]
    fn relay_refuses_limits_below_their_floors_and_unknown_or_repeated_flags() {
        assert!(parse_text("relay --data-dir d --quota-mib 1").is_err());
        assert!(parse_text("relay --data-dir d --max-age-days 0").is_err());
        assert!(parse_text("relay --data-dir d --quota-mib 99999999999999999").is_err());
        assert!(parse_text("relay --data-dir d --port 1").is_err());
        assert!(parse_text("relay --data-dir d --data-dir e").is_err());
        assert!(parse_text("relay --data-dir").is_err());
        assert!(parse_text("relay").is_err());
    }

    #[test]
    fn setup_needs_valid_endpoints_within_the_bound() {
        let Command::RelaySetup(config) = parse_text(
            "relay setup --data-dir d --endpoint relay.example.org:43823 --endpoint 192.0.2.4:43823",
        )
        .unwrap() else {
            panic!("expected relay setup");
        };
        assert_eq!(config.endpoints.len(), 2);
        assert!(parse_text("relay setup --data-dir d").is_err());
        assert!(parse_text("relay setup --data-dir d --endpoint nope").is_err());
        let many = format!(
            "relay setup --data-dir d{}",
            " --endpoint a.example:1".repeat(MAX_SERVER_ENDPOINTS + 1)
        );
        assert!(parse_text(&many).is_err());
    }

    #[test]
    fn anchor_actions_parse_their_own_flags() {
        let Command::Anchor(config) = parse_text("anchor --data-dir d").unwrap() else {
            panic!("expected anchor");
        };
        assert_eq!(config.listen, default_listen());
        assert!(matches!(
            parse_text("anchor setup --data-dir d --endpoint a.example:1").unwrap(),
            Command::AnchorSetup(_)
        ));
        let Command::AnchorBackup(config) =
            parse_text("anchor backup --data-dir d --out f.ganbaru-backup").unwrap()
        else {
            panic!("expected anchor backup");
        };
        assert_eq!(config.out, PathBuf::from("f.ganbaru-backup"));
        assert!(parse_text("anchor").is_err());
        assert!(parse_text("anchor setup --data-dir d").is_err());
        assert!(parse_text("anchor backup --data-dir d").is_err());
        assert!(parse_text("anchor restore --data-dir d").is_err());
        assert!(parse_text("anchor --data-dir d --quota-mib 100").is_err());
    }

    #[test]
    fn the_environment_names_the_data_directory_unless_a_flag_overrides_it() {
        let fallback = Some(PathBuf::from("/data"));
        let Command::Anchor(config) = parse(args("anchor"), fallback.clone()).unwrap() else {
            panic!("expected anchor");
        };
        assert_eq!(config.data_dir, PathBuf::from("/data"));
        assert_eq!(
            parse(args("relay spaces --data-dir d"), fallback.clone()).unwrap(),
            Command::RelaySpaces(PathBuf::from("d"))
        );
        assert!(parse(args("anchor"), Some(PathBuf::new())).is_err());
        assert!(parse(args("anchor --data-dir"), fallback).is_err());
    }

    #[test]
    fn help_and_version_take_no_options_and_unknown_words_are_refused() {
        assert_eq!(parse(Vec::new(), None).unwrap(), Command::Help);
        assert_eq!(parse_text("help").unwrap(), Command::Help);
        assert_eq!(parse_text("version").unwrap(), Command::Version);
        assert_eq!(parse_text("--version").unwrap(), Command::Version);
        assert!(parse_text("version --data-dir d").is_err());
        assert!(parse_text("launch").is_err());
        assert!(parse_text("serve --data-dir d").is_err());
        assert!(parse_text("relay serve --data-dir d").is_err());
    }
}
