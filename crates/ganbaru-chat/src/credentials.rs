//! Native credential storage for provider secrets.

use super::models::{
    ChatError, ChatErrorCode, ChatResult, CredentialReferenceId, ProviderInstanceConfig,
    ProviderInstanceId,
};
use serde::Serialize;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialStoreAvailability {
    Available,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialStoreErrorCode {
    Unavailable,
    AccessDenied,
    InvalidReference,
    CorruptEntry,
    AmbiguousEntry,
    OperationFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialStoreOperation {
    Probe,
    Read,
    Replace,
    Remove,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialStoreError {
    pub code: CredentialStoreErrorCode,
    pub operation: CredentialStoreOperation,
}

impl CredentialStoreError {
    pub fn new(code: CredentialStoreErrorCode, operation: CredentialStoreOperation) -> Self {
        Self { code, operation }
    }
}

impl fmt::Display for CredentialStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "credential store {:?} failed with {:?}",
            self.operation, self.code
        )
    }
}

impl std::error::Error for CredentialStoreError {}

#[derive(Eq, PartialEq)]
pub struct SecretValue(String);

impl SecretValue {
    pub fn new(value: impl Into<String>) -> Result<Self, CredentialStoreError> {
        let value = value.into();
        if value.is_empty() {
            return Err(CredentialStoreError::new(
                CredentialStoreErrorCode::OperationFailed,
                CredentialStoreOperation::Replace,
            ));
        }
        Ok(Self(value))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretValue([REDACTED])")
    }
}

pub trait CredentialStore: Send + Sync {
    fn availability(&self) -> CredentialStoreAvailability;

    fn read(
        &self,
        reference: &CredentialReferenceId,
    ) -> Result<Option<SecretValue>, CredentialStoreError>;

    fn replace(
        &self,
        reference: &CredentialReferenceId,
        value: &SecretValue,
    ) -> Result<(), CredentialStoreError>;

    fn remove(&self, reference: &CredentialReferenceId) -> Result<bool, CredentialStoreError>;
}

/// The only credential reference a provider instance may use for one environment variable.
///
/// Rust derives references from the instance and variable name, so a configuration cannot name
/// another instance's secret or a source-control credential.
pub fn provider_credential_reference(
    instance_id: &ProviderInstanceId,
    environment_name: &str,
) -> ChatResult<CredentialReferenceId> {
    let mut characters = environment_name.chars();
    let valid_name = characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric());
    if !valid_name {
        return Err(ChatError::validation(
            "environmentName",
            "environment variable names use ASCII letters, digits, and underscores",
        ));
    }
    CredentialReferenceId::new(format!(
        "provider:{}:environment:{environment_name}",
        instance_id.as_str()
    ))
    .map_err(|_| ChatError::validation("environmentName", "credential reference is invalid"))
}

/// Whether `reference` is the derived reference for this instance and variable.
pub fn is_provider_credential_reference(
    instance_id: &ProviderInstanceId,
    environment_name: &str,
    reference: &CredentialReferenceId,
) -> bool {
    provider_credential_reference(instance_id, environment_name)
        .is_ok_and(|expected| &expected == reference)
}

pub fn materialize_provider_environment(
    configuration: &ProviderInstanceConfig,
    store: &impl CredentialStore,
) -> ChatResult<ProviderInstanceConfig> {
    for (name, reference) in &configuration.credential_references {
        if !is_provider_credential_reference(&configuration.instance_id, name, reference) {
            return Err(ChatError::new(
                ChatErrorCode::ConfigurationInvalid,
                format!("Stored credential reference for {name} does not belong to this provider"),
                true,
            ));
        }
    }
    let mut resolved = configuration.clone();
    for (name, value) in &configuration.environment {
        let Some(inherited_name) = value.strip_prefix("inherit:") else {
            continue;
        };
        if inherited_name != name {
            return Err(ChatError::new(
                ChatErrorCode::ConfigurationInvalid,
                format!("Inherited environment reference for {name} is invalid"),
                true,
            ));
        }
        let inherited = std::env::var(name).map_err(|_| {
            ChatError::new(
                ChatErrorCode::ConfigurationInvalid,
                format!("Inherited environment variable {name} is unavailable"),
                true,
            )
        })?;
        resolved.environment.insert(name.clone(), inherited);
    }
    for (name, reference) in &configuration.credential_references {
        if resolved.environment.contains_key(name) {
            return Err(ChatError::new(
                ChatErrorCode::ConfigurationInvalid,
                format!("Environment variable {name} has more than one configured source"),
                true,
            ));
        }
        let secret = store.read(reference).map_err(|_| {
            ChatError::new(
                ChatErrorCode::ConfigurationInvalid,
                format!("Stored credential for {name} could not be read"),
                true,
            )
        })?;
        let secret = secret.ok_or_else(|| {
            ChatError::new(
                ChatErrorCode::ConfigurationInvalid,
                format!("Stored credential for {name} is missing"),
                true,
            )
        })?;
        resolved
            .environment
            .insert(name.clone(), secret.expose().to_string());
    }
    resolved.credential_references.clear();
    Ok(resolved)
}
