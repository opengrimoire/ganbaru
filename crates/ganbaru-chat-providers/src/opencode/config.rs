//! OpenCode instance configuration and server identity validation.

use crate::models::{
    ChatError, ChatErrorCode, ChatResult, ContinuationGroupId, ProviderInstanceConfig,
};
use reqwest::Url;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fmt;
use std::net::IpAddr;

pub const OPENCODE_PASSWORD_ENVIRONMENT: &str = "OPENCODE_SERVER_PASSWORD";
const MAX_SERVER_URL_BYTES: usize = 2 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OpenCodeConnectionMode {
    Local,
    External { origin: String, insecure_http: bool },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenCodeProviderSettings {
    pub connection: OpenCodeConnectionMode,
    pub external_workspace_access_confirmed: bool,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredOpenCodeSettings {
    mode: String,
    #[serde(default)]
    server_url: Option<String>,
    #[serde(default)]
    allow_insecure_external_http: bool,
    confirm_external_workspace_access: Option<bool>,
}

pub struct OpenCodeSecret(String);

impl OpenCodeSecret {
    pub(super) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for OpenCodeSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OpenCodeSecret([REDACTED])")
    }
}

impl OpenCodeProviderSettings {
    pub fn parse(configuration: &ProviderInstanceConfig) -> ChatResult<Self> {
        if configuration.provider_config.schema_version != 1 {
            return Err(ChatError::validation(
                "providerConfig.schemaVersion",
                "OpenCode provider configuration schema is unsupported",
            ));
        }
        let stored = serde_json::from_value::<StoredOpenCodeSettings>(
            configuration.provider_config.value.clone(),
        )
        .map_err(|_| {
            ChatError::validation(
                "providerConfig",
                "OpenCode provider configuration is invalid",
            )
        })?;
        if !matches!(stored.mode.as_str(), "local" | "external") {
            return Err(ChatError::validation(
                "providerConfig.mode",
                "OpenCode connection mode is invalid",
            ));
        }
        if stored.mode == "external"
            && stored
                .server_url
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
        {
            return Err(ChatError::validation(
                "providerConfig.serverUrl",
                "External OpenCode mode requires a server URL",
            ));
        }
        let configured_server_url = (stored.mode != "local")
            .then_some(stored.server_url.as_deref())
            .flatten();
        let Some(server_url) = configured_server_url
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Ok(Self {
                connection: OpenCodeConnectionMode::Local,
                external_workspace_access_confirmed: false,
            });
        };
        let (origin, loopback) = normalize_server_origin(server_url)?;
        let insecure_http = origin.starts_with("http://") && !loopback;
        if insecure_http && !stored.allow_insecure_external_http {
            return Err(ChatError::validation(
                "providerConfig.allowInsecureExternalHttp",
                "External OpenCode HTTP requires an explicit insecure-connection override",
            ));
        }
        let external_workspace_access_confirmed =
            stored.confirm_external_workspace_access.ok_or_else(|| {
                ChatError::validation(
                    "providerConfig.confirmExternalWorkspaceAccess",
                    "External OpenCode mode requires an explicit workspace access decision",
                )
            })?;
        Ok(Self {
            connection: OpenCodeConnectionMode::External {
                origin,
                insecure_http,
            },
            external_workspace_access_confirmed,
        })
    }

    pub fn server_origin(&self) -> Option<&str> {
        match &self.connection {
            OpenCodeConnectionMode::Local => None,
            OpenCodeConnectionMode::External { origin, .. } => Some(origin),
        }
    }

    pub fn is_external(&self) -> bool {
        matches!(self.connection, OpenCodeConnectionMode::External { .. })
    }
}

pub fn take_server_password(configuration: &mut ProviderInstanceConfig) -> Option<OpenCodeSecret> {
    configuration
        .environment
        .remove(OPENCODE_PASSWORD_ENVIRONMENT)
        .filter(|value| !value.is_empty())
        .map(OpenCodeSecret)
}

pub fn continuation_group(
    settings: &OpenCodeProviderSettings,
    account_identity: Option<&str>,
) -> ChatResult<ContinuationGroupId> {
    let mut digest = Sha256::new();
    digest.update(b"ganbaru-chat-opencode-server-v1\0");
    digest.update(
        settings
            .server_origin()
            .unwrap_or("owned-loopback")
            .as_bytes(),
    );
    digest.update(b"\0");
    digest.update(account_identity.unwrap_or("default").as_bytes());
    ContinuationGroupId::new(format!("opencode-server-{:x}", digest.finalize())).map_err(|_| {
        ChatError::new(
            ChatErrorCode::Protocol,
            "OpenCode continuation identity is invalid",
            false,
        )
    })
}

fn normalize_server_origin(value: &str) -> ChatResult<(String, bool)> {
    if value.len() > MAX_SERVER_URL_BYTES || value.contains(['\0', '\n', '\r']) {
        return Err(invalid_server_url_error());
    }
    let parsed = Url::parse(value).map_err(|_| invalid_server_url_error())?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.host_str().is_none()
        || !matches!(parsed.path(), "" | "/")
    {
        return Err(invalid_server_url_error());
    }
    let loopback = is_loopback_host(&parsed);
    let port = parsed
        .port_or_known_default()
        .ok_or_else(invalid_server_url_error)?;
    let host = normalized_host(&parsed)?;
    let default_port = matches!((parsed.scheme(), port), ("http", 80) | ("https", 443));
    let origin = if default_port {
        format!("{}://{host}", parsed.scheme())
    } else {
        format!("{}://{host}:{port}", parsed.scheme())
    };
    Ok((origin, loopback))
}

fn normalized_host(url: &Url) -> ChatResult<String> {
    let host = url.host_str().ok_or_else(invalid_server_url_error)?;
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V6(address)) => Ok(format!("[{address}]")),
        Ok(IpAddr::V4(address)) => Ok(address.to_string()),
        Err(_) => Ok(host.to_ascii_lowercase()),
    }
}

fn is_loopback_host(url: &Url) -> bool {
    match url.host_str() {
        Some(host) if host.eq_ignore_ascii_case("localhost") => true,
        Some(host) => host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback()),
        None => false,
    }
}

fn invalid_server_url_error() -> ChatError {
    ChatError::validation(
        "providerConfig.serverUrl",
        "OpenCode server URL must be an HTTP or HTTPS origin without credentials",
    )
}
