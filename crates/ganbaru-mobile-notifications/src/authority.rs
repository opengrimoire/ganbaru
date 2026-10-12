//! Private callback guard for platform requests already queued by Tauri.

use std::sync::OnceLock;

type AuthorityChecker = dyn Fn(u64, i64) -> bool + Send + Sync;
struct RegisteredAuthority {
    process_nonce: i64,
    check: Box<AuthorityChecker>,
}
static AUTHORITY: OnceLock<RegisteredAuthority> = OnceLock::new();

/// Register the application-owned cached revision check before starting delivery.
/// Android callbacks cannot use a WebView or retained notification as authority.
pub fn set_focus_authority_checker(
    process_nonce: i64,
    checker: impl Fn(u64, i64) -> bool + Send + Sync + 'static,
) -> Result<(), String> {
    if process_nonce <= 0 {
        return Err("Native Android Focus process identity must be positive".to_owned());
    }
    AUTHORITY
        .set(RegisteredAuthority {
            process_nonce,
            check: Box::new(checker),
        })
        .map_err(|_| "Native Android Focus authority is already installed".to_owned())
}

#[cfg(target_os = "android")]
pub(super) fn process_nonce() -> Result<i64, String> {
    AUTHORITY
        .get()
        .map(|authority| authority.process_nonce)
        .ok_or_else(|| "Native Android Focus authority is not installed".to_owned())
}

fn is_phase_current(
    authority: Option<&RegisteredAuthority>,
    process_nonce: i64,
    generation: i64,
    revision: i64,
) -> bool {
    let Ok(generation) = u64::try_from(generation) else {
        return false;
    };
    if process_nonce <= 0 || generation == 0 || revision < 0 {
        return false;
    }
    authority.is_some_and(|authority| {
        authority.process_nonce == process_nonce && (authority.check)(generation, revision)
    })
}

fn is_process_current(authority: Option<&RegisteredAuthority>, process_nonce: i64) -> bool {
    process_nonce > 0 && authority.is_some_and(|authority| authority.process_nonce == process_nonce)
}

fn contained(check: impl FnOnce() -> bool) -> bool {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(check)) {
        Ok(current) => current,
        Err(payload) => {
            // Its destructor may also panic. Never restart unwinding through JNI.
            std::mem::forget(payload);
            false
        }
    }
}

/// Revocation and language updates require the current process, even without a live phase.
#[cfg(target_os = "android")]
// SAFETY: This unique symbol matches the Kotlin static native process check.
// Two opaque JNI pointers are ignored, one signed 64-bit jlong is inspected,
// and the returned unsigned 8-bit jboolean is zero or one. This total check
// invokes no callback, accesses no raw pointer, and cannot unwind.
#[unsafe(export_name = "Java_org_opengrimoire_ganbaruai_mobile_notifications_NativeFocusAuthority_isProcessCurrent")]
pub extern "system" fn native_focus_process_is_current(
    _environment: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
    process_nonce: i64,
) -> u8 {
    u8::from(is_process_current(AUTHORITY.get(), process_nonce))
}

/// JNI uses two opaque pointers plus three primitive jlong inputs and a jboolean result.
/// The pointers are neither dereferenced nor retained; no JNI object lifetime crosses Rust.
/// Primitive widths follow the [JNI type specification](https://docs.oracle.com/en/java/javase/21/docs/specs/jni/types.html).
#[cfg(target_os = "android")]
// SAFETY: This unique JNI symbol matches the Kotlin @JvmStatic native method.
// JNI supplies opaque environment/class pointers, three signed 64-bit jlong values,
// and consumes an unsigned 8-bit jboolean. No pointer is accessed or retained.
// The installed checker lives for the process and panics are contained below.
#[unsafe(export_name = "Java_org_opengrimoire_ganbaruai_mobile_notifications_NativeFocusAuthority_isCurrent")]
pub extern "system" fn native_focus_is_current(
    _environment: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
    process_nonce: i64,
    generation: i64,
    revision: i64,
) -> u8 {
    u8::from(contained(|| {
        is_phase_current(AUTHORITY.get(), process_nonce, generation, revision)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_callback_contains_panics_even_when_the_payload_panics_on_drop() {
        struct PanicOnDrop;
        impl Drop for PanicOnDrop {
            fn drop(&mut self) {
                panic!("payload destructor must not run in JNI");
            }
        }
        assert!(contained(|| true));
        assert!(!contained(|| false));
        assert!(!contained(|| panic!("authority unavailable")));
        assert!(!contained(|| std::panic::panic_any(PanicOnDrop)));
    }
    #[test]
    fn native_callback_rejects_uninstalled_or_invalid_authority() {
        assert!(!is_phase_current(None, 100, 1, 0));
        let authority = RegisteredAuthority {
            process_nonce: 100,
            check: Box::new(|_, _| true),
        };
        assert!(!is_phase_current(Some(&authority), 0, 1, 0));
        assert!(!is_phase_current(Some(&authority), -1, 1, 0));
        assert!(!is_phase_current(Some(&authority), 101, 1, 0));
        assert!(!is_phase_current(Some(&authority), 100, -1, 0));
        assert!(!is_phase_current(Some(&authority), 100, 0, 0));
        assert!(!is_phase_current(Some(&authority), 100, 1, -1));
    }
    #[test]
    fn native_callback_revocation_requires_the_process_without_requiring_an_active_phase() {
        let authority = RegisteredAuthority {
            process_nonce: 100,
            check: Box::new(|_, _| false),
        };
        assert!(!is_phase_current(Some(&authority), 100, 3, 10));
        assert!(is_process_current(Some(&authority), 100));
        assert!(!is_process_current(Some(&authority), 101));
        assert!(!is_process_current(Some(&authority), 0));
        assert!(!is_process_current(None, 100));
    }
    #[test]
    fn native_callback_checks_live_revision_at_execution_time() {
        use std::sync::{
            Arc,
            atomic::{AtomicI64, Ordering},
        };
        let revision = Arc::new(AtomicI64::new(10));
        let current = Arc::clone(&revision);
        assert!(set_focus_authority_checker(0, |_, _| true).is_err());
        set_focus_authority_checker(100, move |generation: u64, candidate: i64| {
            generation == 3 && candidate == current.load(Ordering::Acquire)
        })
        .unwrap();
        let authority = AUTHORITY.get();
        assert!(is_phase_current(authority, 100, 3, 10));
        revision.store(11, Ordering::Release);
        assert!(!is_phase_current(authority, 100, 3, 10));
        assert!(is_phase_current(authority, 100, 3, 11));
        assert!(!is_phase_current(authority, 100, 2, 11));
        // A restarted process must reject old envelopes even if counters coincide.
        assert!(!is_phase_current(authority, 99, 3, 11));
        assert!(set_focus_authority_checker(101, |_, _| true).is_err());
    }
}
