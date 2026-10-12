//! Private, cached authority for volatile, process-local decoder deliveries.

use std::sync::OnceLock;

type Checker = dyn Fn(i64) -> bool + Send + Sync;
static CHECKER: OnceLock<Box<Checker>> = OnceLock::new();

/// Install the native owner's bounded cached check before attaching the service.
pub fn set_music_authority_checker(
    checker: impl Fn(i64) -> bool + Send + Sync + 'static,
) -> Result<(), String> {
    CHECKER
        .set(Box::new(checker))
        .map_err(|_| "Android Music authority is already installed".to_string())
}

fn is_delivery_current(checker: Option<&Checker>, delivery_id: i64) -> bool {
    if delivery_id <= 0 {
        return false;
    }
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        checker.is_some_and(|checker| checker(delivery_id))
    })) {
        Ok(current) => current,
        Err(payload) => {
            // A panic payload can itself panic on drop. Keep unwinding out of JNI.
            std::mem::forget(payload);
            false
        }
    }
}

/// Primitive-only JNI callback; opaque pointers are neither read nor retained.
/// Widths follow the [JNI type contract](https://docs.oracle.com/en/java/javase/21/docs/specs/jni/types.html).
#[cfg(target_os = "android")]
// SAFETY: The unique symbol matches Kotlin's static isCurrent(long): boolean.
// JNI passes two ignored opaque pointers and one signed 64-bit jlong, and reads
// an unsigned 8-bit jboolean. The process-lifetime checker uses safe cached reads.
// All callback panics, including panic-on-drop payloads, are contained by is_delivery_current.
#[unsafe(export_name = "Java_org_opengrimoire_ganbaruai_mobile_media_NativeMusicAuthority_isCurrent")]
pub extern "system" fn native_music_is_current(
    _environment: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
    delivery_id: i64,
) -> u8 {
    u8::from(is_delivery_current(
        CHECKER.get().map(Box::as_ref),
        delivery_id,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_invalid_or_revoked_delivery_cannot_reach_the_decoder() {
        let checker = |id| id == 2;
        assert!(!is_delivery_current(None, 2));
        assert!(!is_delivery_current(Some(&checker), 0));
        assert!(!is_delivery_current(Some(&checker), -1));
        assert!(!is_delivery_current(Some(&checker), 1));
        assert!(is_delivery_current(Some(&checker), 2));
    }

    #[test]
    fn callback_panics_and_panicking_payload_destructors_fail_closed() {
        struct PanicOnDrop;
        impl Drop for PanicOnDrop {
            fn drop(&mut self) {
                panic!("payload destructor must not run across JNI");
            }
        }
        assert!(!is_delivery_current(Some(&|_| panic!("checker failed")), 1));
        assert!(!is_delivery_current(
            Some(&|_| std::panic::panic_any(PanicOnDrop)),
            1
        ));
    }

    #[test]
    fn one_process_checker_observes_revocation_and_cannot_be_replaced() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let active = Arc::new(AtomicBool::new(true));
        let snapshot = Arc::clone(&active);
        set_music_authority_checker(move |_| snapshot.load(Ordering::Acquire)).unwrap();
        assert!(is_delivery_current(CHECKER.get().map(Box::as_ref), 1));
        active.store(false, Ordering::Release);
        assert!(!is_delivery_current(CHECKER.get().map(Box::as_ref), 1));
        assert!(set_music_authority_checker(|_| true).is_err());
    }
}
