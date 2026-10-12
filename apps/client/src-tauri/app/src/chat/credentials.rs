//! Operating-system credential storage for Chat secrets.

use ganbaru_chat_contracts::models::CredentialReferenceId;

pub use ganbaru_chat::credentials::*;

const CHAT_CREDENTIAL_SERVICE: &str = "com.ganbaru-ai.chat";
const AVAILABILITY_PROBE_REFERENCE: &str = "credential-store-availability-probe";

fn native_availability() -> CredentialStoreAvailability {
    match keyring::Entry::new(CHAT_CREDENTIAL_SERVICE, AVAILABILITY_PROBE_REFERENCE) {
        Ok(_) => CredentialStoreAvailability::Available,
        Err(_) => CredentialStoreAvailability::Unavailable,
    }
}

fn native_entry(
    reference: &CredentialReferenceId,
    operation: CredentialStoreOperation,
) -> Result<keyring::Entry, CredentialStoreError> {
    keyring::Entry::new(CHAT_CREDENTIAL_SERVICE, reference.as_str())
        .map_err(|error| map_keyring_error(&error, operation))
}

fn native_read(
    reference: &CredentialReferenceId,
) -> Result<Option<SecretValue>, CredentialStoreError> {
    let entry = native_entry(reference, CredentialStoreOperation::Read)?;
    match entry.get_password() {
        Ok(value) => SecretValue::new(value).map(Some),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(map_keyring_error(&error, CredentialStoreOperation::Read)),
    }
}

fn native_replace(
    reference: &CredentialReferenceId,
    value: &SecretValue,
) -> Result<(), CredentialStoreError> {
    let entry = native_entry(reference, CredentialStoreOperation::Replace)?;
    entry
        .set_password(value.expose())
        .map_err(|error| map_keyring_error(&error, CredentialStoreOperation::Replace))
}

fn native_remove(reference: &CredentialReferenceId) -> Result<bool, CredentialStoreError> {
    let entry = native_entry(reference, CredentialStoreOperation::Remove)?;
    match entry.delete_credential() {
        Ok(()) => Ok(true),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(error) => Err(map_keyring_error(&error, CredentialStoreOperation::Remove)),
    }
}

fn map_keyring_error(
    error: &keyring::Error,
    operation: CredentialStoreOperation,
) -> CredentialStoreError {
    let code = match error {
        keyring::Error::NoDefaultStore | keyring::Error::NotSupportedByStore(_) => {
            CredentialStoreErrorCode::Unavailable
        }
        keyring::Error::NoStorageAccess(_) => CredentialStoreErrorCode::AccessDenied,
        keyring::Error::Invalid(_, _) | keyring::Error::TooLong(_, _) => {
            CredentialStoreErrorCode::InvalidReference
        }
        keyring::Error::BadEncoding(_)
        | keyring::Error::BadDataFormat(_, _)
        | keyring::Error::BadStoreFormat(_) => CredentialStoreErrorCode::CorruptEntry,
        keyring::Error::Ambiguous(_) => CredentialStoreErrorCode::AmbiguousEntry,
        keyring::Error::PlatformFailure(_) | keyring::Error::NoEntry => {
            CredentialStoreErrorCode::OperationFailed
        }
        _ => CredentialStoreErrorCode::OperationFailed,
    };
    CredentialStoreError::new(code, operation)
}

macro_rules! native_credential_store {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $name;

        impl CredentialStore for $name {
            fn availability(&self) -> CredentialStoreAvailability {
                native_availability()
            }

            fn read(
                &self,
                reference: &CredentialReferenceId,
            ) -> Result<Option<SecretValue>, CredentialStoreError> {
                native_read(reference)
            }

            fn replace(
                &self,
                reference: &CredentialReferenceId,
                value: &SecretValue,
            ) -> Result<(), CredentialStoreError> {
                native_replace(reference, value)
            }

            fn remove(
                &self,
                reference: &CredentialReferenceId,
            ) -> Result<bool, CredentialStoreError> {
                native_remove(reference)
            }
        }
    };
}

#[cfg(target_os = "linux")]
native_credential_store!(LinuxCredentialStore);

#[cfg(target_os = "windows")]
native_credential_store!(WindowsCredentialStore);

#[cfg(target_os = "macos")]
native_credential_store!(MacOsCredentialStore);

#[cfg(target_os = "linux")]
pub type PlatformCredentialStore = LinuxCredentialStore;

#[cfg(target_os = "windows")]
pub type PlatformCredentialStore = WindowsCredentialStore;

#[cfg(target_os = "macos")]
pub type PlatformCredentialStore = MacOsCredentialStore;
