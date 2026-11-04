//! One-time setup codes. A setup code names the relay's endpoints, pins its certificate, and
//! carries a token that lets exactly one device claim one space within the token lifetime.

use crate::identity::RelayIdentity;
use crate::store::RelayStore;
use ganbaru_handoff::protocol::QrMatrix;
use ganbaru_handoff::protocol::relay::{
    RELAY_SETUP_TOKEN_LIFETIME, RelaySetupCode, RelaySetupToken,
};
use std::fmt::Write as _;

/// Light modules around a QR code, as the QR specification asks.
const QR_QUIET_ZONE: usize = 4;

/// A setup code ready to print.
#[derive(Debug)]
pub struct IssuedSetupCode {
    /// The code a device scans or pastes.
    pub code: RelaySetupCode,
    /// When the token stops being accepted, in Unix milliseconds.
    pub expires_at_ms: i64,
}

/// Creates a token, stores its digest with its expiry, and returns the setup code.
pub async fn issue(
    store: &RelayStore,
    identity: &RelayIdentity,
    endpoints: Vec<String>,
    now_ms: i64,
) -> Result<IssuedSetupCode, String> {
    let token = RelaySetupToken::generate()?;
    let lifetime = i64::try_from(RELAY_SETUP_TOKEN_LIFETIME.as_millis())
        .map_err(|_| "setup token lifetime is too large".to_string())?;
    let expires_at_ms = now_ms.saturating_add(lifetime);
    let code = RelaySetupCode {
        endpoints,
        certificate_fingerprint: identity.fingerprint_bytes(),
        token,
    };
    // Validate before storing, so an unusable code never leaves a live token behind.
    code.to_bytes()?;
    store
        .add_setup_token(&code.token, expires_at_ms)
        .await
        .map_err(|error| format!("store setup token: {error}"))?;
    Ok(IssuedSetupCode {
        code,
        expires_at_ms,
    })
}

/// Renders a QR matrix for a terminal with half-block characters, two module rows per line,
/// drawing light modules so the code reads on dark and light terminal themes alike.
pub fn render_qr(matrix: &QrMatrix) -> String {
    let size = matrix.width + 2 * QR_QUIET_ZONE;
    let dark = |x: usize, y: usize| -> bool {
        if x < QR_QUIET_ZONE || y < QR_QUIET_ZONE {
            return false;
        }
        let (x, y) = (x - QR_QUIET_ZONE, y - QR_QUIET_ZONE);
        x < matrix.width
            && y < matrix.width
            && matrix
                .modules
                .get(y * matrix.width + x)
                .copied()
                .unwrap_or(false)
    };
    let mut output = String::new();
    for y in (0..size).step_by(2) {
        for x in 0..size {
            let top_light = !dark(x, y);
            let bottom_light = y + 1 < size && !dark(x, y + 1);
            output.push(match (top_light, bottom_light) {
                (true, true) => '\u{2588}',
                (true, false) => '\u{2580}',
                (false, true) => '\u{2584}',
                (false, false) => ' ',
            });
        }
        output.push('\n');
    }
    output
}

/// Text an operator reads: the QR code, the text code, the fingerprint, and the expiry.
pub fn describe(issued: &IssuedSetupCode, identity: &RelayIdentity) -> Result<String, String> {
    let matrix = ganbaru_handoff::protocol::qr_matrix_for_bytes(&issued.code.to_bytes()?)?;
    let minutes = RELAY_SETUP_TOKEN_LIFETIME.as_secs() / 60;
    let mut text = render_qr(&matrix);
    let _ = writeln!(text);
    let _ = writeln!(text, "Setup code: {}", issued.code.to_text()?);
    let _ = writeln!(text, "Certificate fingerprint: {}", identity.fingerprint());
    let _ = writeln!(
        text,
        "The code works once, for one space, for {minutes} minutes. Keep it private until it is used."
    );
    Ok(text)
}
