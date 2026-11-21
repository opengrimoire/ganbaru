use super::super::sandbox_check::{
    CodexSandboxKey, cached_sandbox_result, record_sandbox_result, sandbox_starts,
};
use super::*;

impl CodexProviderDriver {
    /// The authority this Codex installation supports on this device: every organizational
    /// scope, or only conversations once the check found that the command sandbox cannot start.
    pub(super) fn device_authority_support(&self) -> ProviderAuthoritySupport {
        if self.known_sandbox_failure() {
            organizational_conversation_support()
        } else {
            organizational_authority_support()
        }
    }

    /// Whether the Codex command sandbox can start on this device, checked once per Codex
    /// installation and home for the app run. Any failure to resolve the installation counts as
    /// a sandbox that cannot start.
    pub(super) async fn command_sandbox_starts(&self) -> bool {
        #[cfg(test)]
        if self.connection_factory.is_some() {
            return self.test_sandbox_starts;
        }
        let Some(key) = self.sandbox_key() else {
            return false;
        };
        if let Some(starts) = cached_sandbox_result(&key) {
            return starts;
        }
        let starts = self.check_command_sandbox().await;
        record_sandbox_result(key, starts);
        starts
    }

    fn known_sandbox_failure(&self) -> bool {
        #[cfg(test)]
        if self.connection_factory.is_some() {
            return !self.test_sandbox_starts;
        }
        self.sandbox_key()
            .and_then(|key| cached_sandbox_result(&key))
            .is_some_and(|starts| !starts)
    }

    fn sandbox_key(&self) -> Option<CodexSandboxKey> {
        let layout = resolve_codex_home_layout(&self.configuration, &self.settings).ok()?;
        Some(CodexSandboxKey::new(
            &self.configuration.executable,
            &layout.shared_home,
            &layout.effective_home,
        ))
    }

    async fn check_command_sandbox(&self) -> bool {
        let Ok(mut layout) = resolve_codex_home_layout(&self.configuration, &self.settings) else {
            return false;
        };
        if materialize_codex_shadow_home(&mut layout).is_err()
            || verify_codex_shadow_home(&layout).is_err()
        {
            return false;
        }
        let Ok(environment) = codex_process_environment(&self.configuration, &layout) else {
            return false;
        };
        let Ok(executable) = resolve_codex_executable(&self.configuration.executable, &environment)
        else {
            return false;
        };
        sandbox_starts(
            &executable.executable,
            &executable.prefix_arguments,
            &environment,
        )
        .await
    }
}
