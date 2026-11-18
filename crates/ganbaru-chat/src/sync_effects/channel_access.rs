//! Settling concurrent edits of one AI channel membership's access.
//!
//! The sync priority ranks a channel membership's access value by what it clears and by its
//! approval overrides, because a priority reads only the value itself. It cannot combine
//! narrowings of different parts, and it cannot rank an inherited scratch override, which follows
//! the channel override and so may be stricter than a concurrent explicit scratch override.
//! [`settled_channel_access`] combines the concurrent versions into the narrowest of each: every
//! capability any version cleared, the latest history boundary, and the strictest channel and
//! scratch overrides. A concurrent narrowing therefore never loses to a concurrent widening of
//! another part of the same value.

use super::approval_strictness;

/// The history boundary that starts at the grant.
const FROM_GRANT: &str = "from_grant";

/// One version of a channel membership's access, without its removal time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelAccess {
    /// Whether the agent reads channel history.
    pub read_history: bool,
    /// Whether the agent participates and can take assignments.
    pub participate: bool,
    /// History boundary, `entire` or `from_grant`.
    pub history_boundary: String,
    /// Lowest message order key a `from_grant` boundary lets the agent read.
    pub history_from_order_key: Option<String>,
    /// Channel approval override, `None` when the agent's default applies.
    pub runtime_approval_policy: Option<String>,
    /// Scratch approval override, `None` when the channel override applies.
    pub scratch_runtime_approval_policy: Option<String>,
}

/// History a version lets the agent read, ordered from widest to narrowest.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Readable {
    /// Every message.
    Entire,
    /// Messages from this order key on; a later key reads less.
    From(String),
    /// Nothing, as a `from_grant` boundary without an order key.
    Nothing,
}

/// The access that settles the concurrent `versions` of one live channel membership, or `None`
/// when the version at `winner` already is the narrowest of them.
///
/// The result starts from the winner and takes, from every version: a clear of history reading
/// or participation that any version makes; a `from_grant` boundary at the latest order key any
/// version reads from, or a history reading clear when a version reads nothing; the strictest
/// channel override; and, when any version sets a scratch override, the strictest override any
/// version applies to scratch work. The result never grants more than any version and holds
/// overrides at least as strict, so the sync priority ranks it at or above every version and no
/// concurrent edit can reverse it.
pub fn settled_channel_access(versions: &[ChannelAccess], winner: usize) -> Option<ChannelAccess> {
    let winning = versions.get(winner)?;
    let mut settled = winning.clone();
    if versions.iter().any(|version| !version.read_history) {
        settled.read_history = false;
    }
    if versions.iter().any(|version| !version.participate) {
        settled.participate = false;
    }

    let narrowest = versions
        .iter()
        .map(readable)
        .max()
        .unwrap_or(Readable::Entire);
    let latest_key = versions
        .iter()
        .filter(|version| version.history_boundary == FROM_GRANT)
        .filter_map(|version| version.history_from_order_key.clone())
        .max();
    match narrowest {
        Readable::From(key) => set_from_grant(&mut settled, key),
        Readable::Nothing => {
            if let Some(key) = latest_key {
                set_from_grant(&mut settled, key);
            }
            if readable(&settled) < Readable::Nothing {
                settled.read_history = false;
            }
        }
        Readable::Entire => {}
    }

    settled.runtime_approval_policy = strictest(
        versions
            .iter()
            .map(|version| version.runtime_approval_policy.as_deref()),
    );
    if versions
        .iter()
        .any(|version| version.scratch_runtime_approval_policy.is_some())
    {
        settled.scratch_runtime_approval_policy = strictest(versions.iter().map(|version| {
            version
                .scratch_runtime_approval_policy
                .as_deref()
                .or(version.runtime_approval_policy.as_deref())
        }));
    }

    (settled != *winning).then_some(settled)
}

/// The history `access` lets the agent read, resolved as the runtime does.
fn readable(access: &ChannelAccess) -> Readable {
    if access.history_boundary != FROM_GRANT {
        return Readable::Entire;
    }
    access
        .history_from_order_key
        .clone()
        .map_or(Readable::Nothing, Readable::From)
}

/// Sets a `from_grant` boundary at `key`.
fn set_from_grant(access: &mut ChannelAccess, key: String) {
    access.history_boundary = FROM_GRANT.to_string();
    access.history_from_order_key = Some(key);
}

/// The strictest of `policies`, `None` counting as the least strict.
fn strictest<'a>(policies: impl Iterator<Item = Option<&'a str>>) -> Option<String> {
    policies
        .flatten()
        .max_by_key(|policy| approval_strictness(policy))
        .map(str::to_string)
}

#[cfg(test)]
mod tests;
