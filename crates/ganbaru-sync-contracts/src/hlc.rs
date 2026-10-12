//! Hybrid logical clock: 48 bits of Unix milliseconds and a 16-bit counter in one u64.
//!
//! Clocks order concurrent versions and never cause rejection. Validation stays a pure function
//! of operation bytes, and supersession is decided by version vectors.

/// Bits of the counter part.
const COUNTER_BITS: u32 = 16;
/// Largest physical part in milliseconds.
const MAX_PHYSICAL_MS: u64 = (1u64 << (64 - COUNTER_BITS)) - 1;
/// Remote clocks are adopted only up to local physical time plus this many milliseconds.
pub const MAX_ADOPTION_AHEAD_MS: u64 = 60_000;
/// An applied clock this far ahead of local time raises a clock warning.
pub const CLOCK_WARNING_AHEAD_MS: u64 = 5 * 60_000;

/// One hybrid logical clock value. Values compare as their packed u64.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hlc(u64);

impl Hlc {
    /// The smallest clock.
    pub const ZERO: Self = Self(0);

    /// Packs a physical time and counter, saturating the physical part at 48 bits.
    pub const fn new(physical_ms: u64, counter: u16) -> Self {
        let physical = if physical_ms > MAX_PHYSICAL_MS {
            MAX_PHYSICAL_MS
        } else {
            physical_ms
        };
        Self((physical << COUNTER_BITS) | counter as u64)
    }

    /// Wraps a packed value.
    pub const fn from_u64(value: u64) -> Self {
        Self(value)
    }

    /// Packed value.
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Physical part in Unix milliseconds.
    pub const fn physical_ms(self) -> u64 {
        self.0 >> COUNTER_BITS
    }

    /// Counter part.
    pub const fn counter(self) -> u16 {
        (self.0 & 0xffff) as u16
    }

    /// The next clock value; counter overflow advances the physical part.
    pub const fn successor(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

/// Whether a clock is far enough ahead of local time to warn about the writer's device clock.
pub fn is_far_ahead(clock: Hlc, now_ms: u64) -> bool {
    clock.physical_ms() > now_ms.saturating_add(CLOCK_WARNING_AHEAD_MS)
}

/// A replica's clock state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HlcClock {
    last: Hlc,
}

impl HlcClock {
    /// Starts from the last clock this replica sealed or adopted.
    pub const fn new(last: Hlc) -> Self {
        Self { last }
    }

    /// Last clock sealed or adopted.
    pub const fn last(&self) -> Hlc {
        self.last
    }

    /// Adopts a remote clock, capped at local physical time plus [`MAX_ADOPTION_AHEAD_MS`].
    pub fn observe(&mut self, remote: Hlc, now_ms: u64) {
        let cap = Hlc::new(now_ms.saturating_add(MAX_ADOPTION_AHEAD_MS), u16::MAX);
        self.last = self.last.max(remote.min(cap));
    }

    /// Clock for a local seal: the physical time when it is ahead, else the last value plus one.
    pub fn tick(&mut self, now_ms: u64) -> Hlc {
        let physical = Hlc::new(now_ms, 0);
        self.last = if physical > self.last {
            physical
        } else {
            self.last.successor()
        };
        self.last
    }
}
