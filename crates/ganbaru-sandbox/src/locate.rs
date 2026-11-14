//! Finding the helper binary and checking that it confines processes on this device.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{SandboxError, SandboxPolicy, encode_invocation};

/// Installed path of the helper in Linux packages.
const PACKAGED_HELPER_PATH: &str = "/usr/lib/ganbaru/ganbaru-sandbox";
/// File name of the helper that Cargo builds next to a development app.
const HELPER_FILE_NAME: &str = "ganbaru-sandbox";
/// Argument that makes the helper confine itself and exit, without running a program.
pub(crate) const PROBE_ARGUMENT: &str = "--probe";

/// The helper binary that runs programs under a [`SandboxPolicy`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxHelper {
    path: PathBuf,
}

impl SandboxHelper {
    /// Uses the helper at `path`.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Finds the helper of this installation: the packaged one, or in development builds the
    /// one Cargo built next to the running executable.
    pub fn locate() -> Result<Self, SandboxError> {
        let packaged = Path::new(PACKAGED_HELPER_PATH);
        if packaged.is_file() {
            return Ok(Self::at(packaged));
        }
        if cfg!(debug_assertions) {
            let sibling = std::env::current_exe().ok().and_then(|executable| {
                executable
                    .parent()
                    .map(|parent| parent.join(HELPER_FILE_NAME))
            });
            if let Some(sibling) = sibling.filter(|path| path.is_file()) {
                return Ok(Self::at(sibling));
            }
        }
        Err(SandboxError::HelperUnavailable(format!(
            "{PACKAGED_HELPER_PATH} is not installed"
        )))
    }

    /// Path of the helper binary.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the helper lies inside `directory`, where a confined process with write access
    /// could replace it.
    pub fn is_inside(&self, directory: &Path) -> bool {
        let helper = self
            .path
            .canonicalize()
            .unwrap_or_else(|_| self.path.clone());
        let directory = directory
            .canonicalize()
            .unwrap_or_else(|_| directory.to_path_buf());
        helper.starts_with(directory)
    }

    /// A command that runs `program` with `arguments` under `policy`.
    ///
    /// The helper is refused when a writable policy path contains it, because a confined
    /// process could then replace the helper that confines later programs.
    pub fn command(
        &self,
        policy: &SandboxPolicy,
        program: impl Into<OsString>,
        arguments: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Result<Command, SandboxError> {
        let mut command = Command::new(&self.path);
        command.args(self.arguments(policy, program, arguments)?);
        Ok(command)
    }

    /// The helper arguments that run `program` with `arguments` under `policy`, for callers
    /// that build their own command from [`Self::path`].
    ///
    /// The helper is refused when a writable policy path contains it, as in [`Self::command`].
    pub fn arguments(
        &self,
        policy: &SandboxPolicy,
        program: impl Into<OsString>,
        arguments: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Result<Vec<OsString>, SandboxError> {
        if policy.write.iter().any(|path| self.is_inside(path)) {
            return Err(SandboxError::HelperUnavailable(
                "the helper lies inside a writable sandbox path".to_string(),
            ));
        }
        encode_invocation(policy, program, arguments)
    }

    /// The policy file content that runs `program` with `arguments` and then each shell
    /// command under `policy`, for a provider that starts the helper as its shell; see
    /// [`crate::POLICY_FILE_VARIABLE`].
    ///
    /// The helper is refused when a writable policy path contains it, as in [`Self::command`].
    pub fn policy_file(
        &self,
        policy: &SandboxPolicy,
        program: impl Into<OsString>,
        arguments: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Result<Vec<u8>, SandboxError> {
        if policy.write.iter().any(|path| self.is_inside(path)) {
            return Err(SandboxError::HelperUnavailable(
                "the helper lies inside a writable sandbox path".to_string(),
            ));
        }
        crate::encode_policy_file(policy, program, arguments)
    }
}

/// Checks that `helper` can confine processes on this device.
///
/// The helper applies a policy that denies everything to itself and reports success only when
/// the kernel enforced all of it, so success means confined programs start and an error
/// means they would be refused.
pub fn probe(helper: &SandboxHelper) -> Result<(), SandboxError> {
    let output = Command::new(helper.path())
        .arg(PROBE_ARGUMENT)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| SandboxError::HelperUnavailable(error.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(SandboxError::Unsupported(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}
