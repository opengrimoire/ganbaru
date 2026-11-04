//! The device-local quarantine file.

use super::TestDirectory;
use crate::quarantine::{MAX_QUARANTINE_BYTES, Quarantine};

#[test]
fn quarantine_persists_devices_until_lifted_and_refuses_malformed_files() {
    let directory = TestDirectory::new("quarantine");
    let path = directory.files().quarantine_path();
    let mut quarantine = Quarantine::load(&path).expect("missing file is empty");
    assert!(quarantine.devices().is_empty());

    assert!(quarantine.add("device-b").expect("quarantine"));
    assert!(!quarantine.add("device-b").expect("quarantine again"));
    assert!(quarantine.add("device-c").expect("quarantine another"));
    let reloaded = Quarantine::load(&path).expect("reload");
    assert!(reloaded.contains("device-b") && reloaded.contains("device-c"));

    assert!(quarantine.lift("device-b").expect("lift"));
    assert!(!quarantine.lift("device-b").expect("lift again"));
    let reloaded = Quarantine::load(&path).expect("reload after lift");
    assert!(!reloaded.contains("device-b") && reloaded.contains("device-c"));

    assert!(quarantine.add("").is_err());
    assert!(quarantine.add("device\nb").is_err());

    std::fs::write(&path, br#"{"schemaVersion":2,"devices":[]}"#).expect("write newer");
    assert!(Quarantine::load(&path).is_err());
    std::fs::write(&path, br#"{"schemaVersion":1,"devices":[""]}"#).expect("write malformed");
    assert!(Quarantine::load(&path).is_err());
    std::fs::write(&path, vec![b' '; MAX_QUARANTINE_BYTES + 1]).expect("write oversized");
    assert!(Quarantine::load(&path).is_err());
}
