//! Classification of environment variable names that usually hold secrets.
//!
//! The classifier only drives warnings and display masking. Plaintext values stay valid because
//! name matching cannot tell a credential from an ordinary setting with a similar name.

/// Name segments that mark a variable as secret-like when they appear as a whole segment.
///
/// Plural forms such as `TOKENS` are left out so limits like `MAX_THINKING_TOKENS` stay ordinary.
pub const SECRET_NAME_SEGMENTS: &[&str] = &[
    "APIKEY",
    "AUTH",
    "CREDENTIAL",
    "CREDENTIALS",
    "KEY",
    "PASSPHRASE",
    "PASSWD",
    "PASSWORD",
    "PAT",
    "SECRET",
    "TOKEN",
];

/// Whether an environment variable name looks like it holds a secret.
///
/// Names are split on underscores and compared per segment without regard to case.
pub fn is_secret_like_environment_name(name: &str) -> bool {
    name.split('_').any(|segment| {
        SECRET_NAME_SEGMENTS
            .iter()
            .any(|known| segment.eq_ignore_ascii_case(known))
    })
}

#[cfg(test)]
mod tests {
    use super::is_secret_like_environment_name;

    #[test]
    fn credential_names_are_secret_like() {
        for name in [
            "OPENAI_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "CLAUDE_CODE_OAUTH_TOKEN",
            "OPENCODE_SERVER_PASSWORD",
            "GITHUB_PAT",
            "client_secret",
            "AWS_SECRET_ACCESS_KEY",
            "DEEPSEEK_APIKEY",
        ] {
            assert!(is_secret_like_environment_name(name), "{name}");
        }
    }

    #[test]
    fn ordinary_settings_with_similar_names_are_not_secret_like() {
        for name in [
            "MAX_THINKING_TOKENS",
            "CLAUDE_CODE_MAX_OUTPUT_TOKENS",
            "MCP_TIMEOUT",
            "KEYBOARD_LAYOUT",
            "AUTHOR_NAME",
            "PATH",
            "NODE_OPTIONS",
            "",
        ] {
            assert!(!is_secret_like_environment_name(name), "{name}");
        }
    }
}
