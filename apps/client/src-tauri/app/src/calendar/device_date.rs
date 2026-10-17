//! Platform device-date sources for Calendar scope workers.

use std::sync::Arc;

use tauri::{AppHandle, Runtime};

use ganbaru_calendar::events::scope::DeviceDate;

/// Returns the device-date source for the current platform.
#[cfg(not(target_os = "android"))]
pub(crate) fn source<R: Runtime>(_app: &AppHandle<R>) -> Arc<dyn DeviceDate> {
    Arc::new(SystemDeviceDate)
}

/// Returns the device-date source for the current platform.
#[cfg(target_os = "android")]
pub(crate) fn source<R: Runtime>(app: &AppHandle<R>) -> Arc<dyn DeviceDate> {
    Arc::new(AndroidDeviceDate(app.clone()))
}

/// Reads the device date from the operating system time zone.
#[cfg(not(target_os = "android"))]
struct SystemDeviceDate;

#[cfg(not(target_os = "android"))]
impl DeviceDate for SystemDeviceDate {
    fn local_date(&self, epoch_ms: i64) -> Result<chrono::NaiveDate, String> {
        use ganbaru_civil_time as civil_time;
        Ok(civil_time::instant_to_local(epoch_ms, &civil_time::system_zone()?)?.date())
    }
}

/// Reads the device date from Android's local time facts.
#[cfg(target_os = "android")]
struct AndroidDeviceDate<R: Runtime>(AppHandle<R>);

#[cfg(target_os = "android")]
impl<R: Runtime> DeviceDate for AndroidDeviceDate<R> {
    fn local_date(&self, epoch_ms: i64) -> Result<chrono::NaiveDate, String> {
        use ganbaru_mobile_notifications::MobileNotificationsExt;
        let facts = self
            .0
            .mobile_notifications()
            .device_local_time_facts(&[epoch_ms])?;
        let [fact] = facts.as_slice() else {
            return Err("Android returned incomplete Calendar device-date facts".into());
        };
        if fact.epoch_ms != epoch_ms {
            return Err("Android returned mismatched Calendar device-date facts".into());
        }
        ganbaru_calendar::recurrence::canonical::parse_date(&fact.date_key)
    }
}
