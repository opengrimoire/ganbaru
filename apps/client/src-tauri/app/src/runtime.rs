//! Platform composition roots that register plugins, managed state, and command handlers.

#[cfg(desktop)]
mod desktop;
// Desktop test builds also compile the mobile root so its handler paths stay type-checked.
#[cfg(any(mobile, all(test, desktop)))]
#[cfg_attr(not(mobile), allow(dead_code))]
mod mobile;

#[cfg(desktop)]
pub use desktop::run;
#[cfg(mobile)]
pub use mobile::run;

#[cfg(test)]
mod tests;
