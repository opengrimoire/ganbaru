//! Platform notification commands. Desktop shows native notifications and plays alert sounds;
//! Android reports exact-alarm and notification permission capabilities.

// Desktop test builds also compile the Android capabilities so their handler paths stay checked.
#[cfg(any(test, mobile))]
pub(crate) mod android;
#[cfg(desktop)]
pub(crate) mod desktop;
