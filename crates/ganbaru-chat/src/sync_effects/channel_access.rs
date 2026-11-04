//! Settling concurrent edits of one AI channel membership's access.
//!
//! The sync priority ranks a channel membership's access value by what it clears explicitly and
//! by its approval overrides, because a priority reads only the value itself. Two things it
//! cannot rank: the access profile, whose restrictiveness lives in the profile's effective
//! revision, and an inherited scratch override, which follows the channel override and so may
//! be stricter than a concurrent explicit scratch override. [`settled_channel_access`] combines
//! the concurrent versions into the narrowest of each: the profile with the lowest folder
//! capability, every capability any version cleared or did not resolve to, the latest history
//! boundary, and the strictest channel and scratch overrides. A concurrent narrowing therefore
//! never loses to a concurrent widening of another part of the same value.

use super::{CAPABILITY_RANKS, approval_strictness, persistence_error};
use crate::models::ChatResult;
use sqlx::SqliteConnection;
use std::collections::BTreeMap;

/// The history boundary that starts at the grant.
const FROM_GRANT: &str = "from_grant";

/// One version of a channel membership's access, without its removal time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelAccess {
    /// Access profile the membership names.
    pub access_profile_id: String,
    /// Explicit history reading, capped by the profile.
    pub read_history: bool,
    /// Whether history reading follows the profile.
    pub read_history_inherits_profile: bool,
    /// Explicit participation, capped by the profile.
    pub participate: bool,
    /// Whether participation follows the profile.
    pub participate_inherits_profile: bool,
    /// Explicit history boundary, `entire` or `from_grant`.
    pub history_boundary: String,
    /// Whether the history boundary follows the profile.
    pub history_boundary_inherits_profile: bool,
    /// Lowest message order key a `from_grant` boundary lets the teammate read.
    pub history_from_order_key: Option<String>,
    /// Channel approval override, `None` when the teammate's default applies.
    pub runtime_approval_policy: Option<String>,
    /// Scratch approval override, `None` when the channel override applies.
    pub scratch_runtime_approval_policy: Option<String>,
}

/// What an access profile's effective revision grants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileDefaults {
    /// Whether teammates read history by default.
    pub read_history: bool,
    /// Whether teammates participate by default.
    pub participate: bool,
    /// Default history boundary.
    pub history_boundary: String,
    /// Rank of the maximum folder capability, lower being narrower.
    pub folder_capability_rank: i64,
}

/// History a version lets the teammate read, ordered from widest to narrowest.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Readable {
    /// Every message.
    Entire,
    /// Messages from this order key on; a later key reads less.
    From(String),
    /// Nothing, as a `from_grant` boundary without an order key.
    Nothing,
}

/// Reads the effective revision of each profile in `profile_ids`. A profile without one is left
/// out.
pub async fn channel_access_profiles(
    conn: &mut SqliteConnection,
    profile_ids: &[&str],
) -> ChatResult<BTreeMap<String, ProfileDefaults>> {
    let sql = format!(
        "WITH {CAPABILITY_RANKS}
         SELECT revision.default_read_history, revision.default_participate,
                revision.default_history_boundary, capability_rank.rank
         FROM chat_access_profile_effective_revisions revision
         JOIN capability_rank ON capability_rank.capability = revision.maximum_folder_capability
         WHERE revision.access_profile_id = ?"
    );
    let mut profiles = BTreeMap::new();
    for &profile_id in profile_ids {
        if profiles.contains_key(profile_id) {
            continue;
        }
        let row: Option<(bool, bool, String, i64)> =
            sqlx::query_as(sqlx::AssertSqlSafe(sql.as_str()))
                .bind(profile_id)
                .fetch_optional(&mut *conn)
                .await
                .map_err(persistence_error)?;
        if let Some((read_history, participate, history_boundary, folder_capability_rank)) = row {
            profiles.insert(
                profile_id.to_string(),
                ProfileDefaults {
                    read_history,
                    participate,
                    history_boundary,
                    folder_capability_rank,
                },
            );
        }
    }
    Ok(profiles)
}

/// The access that settles the concurrent `versions` of one live channel membership, or `None`
/// when the version at `winner` already is the narrowest of them.
///
/// The result starts from the winner and takes, from every version: the access profile with the
/// lowest maximum folder capability (the winner's among equals, else the lowest id); an explicit
/// clear of history reading or participation that any version clears explicitly or resolves to
/// off while the result would resolve to on; an explicit `from_grant` boundary at the latest
/// order key any version reads from, or an explicit history reading clear when a version reads
/// nothing; the strictest channel override; and, when any version sets a scratch override, the
/// strictest override any version applies to scratch work. The result never resolves wider than
/// any version, and it clears at least what each version clears explicitly and holds overrides
/// at least as strict, so the sync priority ranks it at or above every version and no
/// concurrent edit can reverse it.
///
/// `profiles` holds the effective revision of each named profile, from
/// [`channel_access_profiles`]. While one is missing the hook keeps the winner, since its
/// restrictiveness is unknown; a device that settles before a profile revision arrives may
/// therefore pick a different profile than one that settles after, and the priority then keeps
/// one of the two settles.
pub fn settled_channel_access(
    versions: &[ChannelAccess],
    winner: usize,
    profiles: &BTreeMap<String, ProfileDefaults>,
) -> Option<ChannelAccess> {
    let winning = versions.get(winner)?;
    let mut defaults = Vec::with_capacity(versions.len());
    for version in versions {
        defaults.push(profiles.get(&version.access_profile_id)?);
    }
    let mut settled = winning.clone();
    settled.access_profile_id = narrowest_profile(versions, winning, profiles)?;
    let profile = profiles.get(&settled.access_profile_id)?;

    let reads = |access: &ChannelAccess, defaults: &ProfileDefaults| {
        resolved(
            access.read_history,
            access.read_history_inherits_profile,
            defaults.read_history,
        )
    };
    let participates = |access: &ChannelAccess, defaults: &ProfileDefaults| {
        resolved(
            access.participate,
            access.participate_inherits_profile,
            defaults.participate,
        )
    };
    let paired = || versions.iter().zip(defaults.iter().copied());
    if paired()
        .any(|(version, _)| clears(version.read_history, version.read_history_inherits_profile))
        || (paired().any(|(version, defaults)| !reads(version, defaults))
            && reads(&settled, profile))
    {
        settled.read_history = false;
        settled.read_history_inherits_profile = false;
    }
    if paired()
        .any(|(version, _)| clears(version.participate, version.participate_inherits_profile))
        || (paired().any(|(version, defaults)| !participates(version, defaults))
            && participates(&settled, profile))
    {
        settled.participate = false;
        settled.participate_inherits_profile = false;
    }

    let narrowest = paired()
        .map(|(version, defaults)| readable(version, defaults))
        .max()
        .unwrap_or(Readable::Entire);
    let explicit_from_grant = versions.iter().any(explicitly_from_grant);
    let latest_key = versions
        .iter()
        .filter(|version| explicitly_from_grant(version))
        .filter_map(|version| version.history_from_order_key.clone())
        .max();
    match narrowest {
        Readable::From(key)
            if explicit_from_grant || readable(&settled, profile) < Readable::From(key.clone()) =>
        {
            set_from_grant(&mut settled, key);
        }
        Readable::Nothing => {
            if let Some(key) = latest_key {
                set_from_grant(&mut settled, key);
            }
            if readable(&settled, profile) < Readable::Nothing {
                settled.read_history = false;
                settled.read_history_inherits_profile = false;
            }
        }
        _ => {}
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

/// The profile with the lowest maximum folder capability among the versions' profiles: the
/// winner's when it is among them, else the lowest id. `None` while one has no effective
/// revision.
fn narrowest_profile(
    versions: &[ChannelAccess],
    winning: &ChannelAccess,
    profiles: &BTreeMap<String, ProfileDefaults>,
) -> Option<String> {
    let lowest = versions
        .iter()
        .map(|version| {
            profiles
                .get(&version.access_profile_id)
                .map(|defaults| defaults.folder_capability_rank)
        })
        .collect::<Option<Vec<i64>>>()?
        .into_iter()
        .min()?;
    let narrowest = |id: &String| {
        profiles
            .get(id)
            .is_some_and(|defaults| defaults.folder_capability_rank == lowest)
    };
    if narrowest(&winning.access_profile_id) {
        return Some(winning.access_profile_id.clone());
    }
    versions
        .iter()
        .map(|version| &version.access_profile_id)
        .filter(|id| narrowest(id))
        .min()
        .cloned()
}

/// A capability as the runtime resolves it: inherited from the profile, or explicit and capped
/// by it.
fn resolved(explicit: bool, inherits: bool, profile: bool) -> bool {
    if inherits {
        profile
    } else {
        explicit && profile
    }
}

/// Whether a capability is cleared explicitly, which the sync priority ranks.
fn clears(explicit: bool, inherits: bool) -> bool {
    !inherits && !explicit
}

/// Whether a version sets a `from_grant` boundary explicitly, which the sync priority ranks.
fn explicitly_from_grant(access: &ChannelAccess) -> bool {
    !access.history_boundary_inherits_profile && access.history_boundary == FROM_GRANT
}

/// The history `access` lets the teammate read under `profile`, resolved as the runtime does: an
/// inherited boundary is the profile's, and an explicit one starts at the grant when either the
/// membership or the profile says so.
fn readable(access: &ChannelAccess, profile: &ProfileDefaults) -> Readable {
    let from_grant = if access.history_boundary_inherits_profile {
        profile.history_boundary == FROM_GRANT
    } else {
        access.history_boundary == FROM_GRANT || profile.history_boundary == FROM_GRANT
    };
    if !from_grant {
        return Readable::Entire;
    }
    access
        .history_from_order_key
        .clone()
        .map_or(Readable::Nothing, Readable::From)
}

/// Sets an explicit `from_grant` boundary at `key`.
fn set_from_grant(access: &mut ChannelAccess, key: String) {
    access.history_boundary = FROM_GRANT.to_string();
    access.history_boundary_inherits_profile = false;
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
