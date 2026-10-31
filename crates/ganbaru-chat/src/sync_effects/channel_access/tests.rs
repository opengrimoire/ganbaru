use super::*;

const NARROW: &str = "profile:narrow";
const WIDE: &str = "profile:wide";
const OTHER_NARROW: &str = "profile:another-narrow";

/// A narrow profile (read only, without history or participation by default), a wide one
/// (publishing, with both), and a second narrow one ranked like the first.
fn profiles() -> BTreeMap<String, ProfileDefaults> {
    BTreeMap::from([
        (
            NARROW.to_string(),
            ProfileDefaults {
                read_history: false,
                participate: false,
                history_boundary: "entire".to_string(),
                folder_capability_rank: 1,
            },
        ),
        (
            OTHER_NARROW.to_string(),
            ProfileDefaults {
                read_history: true,
                participate: true,
                history_boundary: "entire".to_string(),
                folder_capability_rank: 1,
            },
        ),
        (
            WIDE.to_string(),
            ProfileDefaults {
                read_history: true,
                participate: true,
                history_boundary: "entire".to_string(),
                folder_capability_rank: 4,
            },
        ),
    ])
}

/// Access under `profile` inheriting every capability and override.
fn access(profile: &str) -> ChannelAccess {
    ChannelAccess {
        access_profile_id: profile.to_string(),
        read_history: true,
        read_history_inherits_profile: true,
        participate: true,
        participate_inherits_profile: true,
        history_boundary: "entire".to_string(),
        history_boundary_inherits_profile: true,
        history_from_order_key: None,
        runtime_approval_policy: None,
        scratch_runtime_approval_policy: None,
    }
}

fn with_channel(mut access: ChannelAccess, policy: &str) -> ChannelAccess {
    access.runtime_approval_policy = Some(policy.to_string());
    access
}

fn with_scratch(mut access: ChannelAccess, policy: &str) -> ChannelAccess {
    access.scratch_runtime_approval_policy = Some(policy.to_string());
    access
}

fn from_grant(mut access: ChannelAccess, key: &str) -> ChannelAccess {
    access.history_boundary = FROM_GRANT.to_string();
    access.history_boundary_inherits_profile = false;
    access.history_from_order_key = Some(key.to_string());
    access
}

/// A scratch override replaces the channel override, so an edit that clears it inherits the
/// stricter channel override; a concurrent looser explicit scratch override the priority kept
/// settles onto that stricter override.
#[test]
fn a_cleared_scratch_override_settles_a_looser_explicit_one_onto_the_channel_override() {
    let profiles = profiles();
    let inheriting = with_channel(access(WIDE), "ask");
    let looser = with_scratch(with_channel(access(WIDE), "ask"), "unattended");

    let settled = settled_channel_access(&[inheriting, looser.clone()], 1, &profiles);

    assert_eq!(settled, Some(with_scratch(looser, "ask")));
}

/// The profile carries no priority rank, so an "ask" edit outranks a concurrent switch to a
/// narrower profile; the settle keeps both narrowings.
#[test]
fn a_narrower_profile_switch_survives_a_concurrent_stricter_channel_override() {
    let profiles = profiles();
    let narrower = access(NARROW);
    let stricter = with_channel(access(WIDE), "ask");

    let settled = settled_channel_access(&[narrower, stricter], 1, &profiles);

    assert_eq!(settled, Some(with_channel(access(NARROW), "ask")));
}

/// A wider profile switch never displaces a narrower winner, and versions that agree on what
/// matters need no settle.
#[test]
fn a_winner_that_is_already_the_narrowest_needs_no_settle() {
    let profiles = profiles();
    let narrower = with_channel(access(NARROW), "ask");
    let wider = access(WIDE);

    assert_eq!(
        settled_channel_access(&[narrower.clone(), wider], 0, &profiles),
        None
    );
    assert_eq!(
        settled_channel_access(&[narrower.clone(), narrower], 0, &profiles),
        None
    );
}

/// Among profiles that rank alike the winner's stays, and otherwise the lowest id is chosen,
/// so every device settles on the same profile.
#[test]
fn profiles_that_rank_alike_keep_the_winners_or_the_lowest_id() {
    let mut profiles = profiles();
    if let Some(defaults) = profiles.get_mut(NARROW) {
        defaults.read_history = true;
        defaults.participate = true;
    }
    let first = access(OTHER_NARROW);
    let second = access(NARROW);

    assert_eq!(
        settled_channel_access(&[first.clone(), second.clone()], 0, &profiles),
        None
    );
    let stricter = with_channel(access(WIDE), "ask");
    let settled =
        settled_channel_access(&[second, first, stricter], 2, &profiles).expect("a settle");
    assert_eq!(settled.access_profile_id, OTHER_NARROW);
}

/// A profile whose effective revision has not arrived has unknown restrictiveness, so the
/// winner stands until it does.
#[test]
fn a_profile_without_an_effective_revision_keeps_the_winner() {
    let mut profiles = profiles();
    profiles.remove(NARROW);

    let settled = settled_channel_access(
        &[access(NARROW), with_channel(access(WIDE), "ask")],
        1,
        &profiles,
    );

    assert_eq!(settled, None);
}

/// Each explicit clear a version holds is kept, and a capability a version resolves to off
/// through its profile is cleared when the settled profile would grant it.
#[test]
fn explicit_and_resolved_clears_of_every_version_are_kept() {
    let profiles = profiles();
    let mut clears_participation = access(WIDE);
    clears_participation.participate = false;
    clears_participation.participate_inherits_profile = false;
    let mut clears_history = with_channel(access(WIDE), "ask");
    clears_history.read_history = false;
    clears_history.read_history_inherits_profile = false;

    let settled = settled_channel_access(
        &[clears_participation, clears_history.clone()],
        1,
        &profiles,
    )
    .expect("a settle");
    assert!(!settled.participate && !settled.participate_inherits_profile);
    assert!(!settled.read_history && !settled.read_history_inherits_profile);

    // The other narrow profile grants history, while a version under the narrow one inherits
    // none; the settled value keeps the winner's profile but clears history explicitly.
    let inherits_none = access(NARROW);
    let inherits_all = with_channel(access(OTHER_NARROW), "ask");
    let settled =
        settled_channel_access(&[inherits_none, inherits_all], 1, &profiles).expect("a settle");
    assert_eq!(settled.access_profile_id, OTHER_NARROW);
    assert!(!settled.read_history && !settled.read_history_inherits_profile);
    assert!(!settled.participate && !settled.participate_inherits_profile);
}

/// The latest boundary any version reads from wins, and a version that reads no history
/// clears history reading.
#[test]
fn the_latest_history_boundary_and_a_version_without_history_are_kept() {
    let mut profiles = profiles();
    let earlier = with_channel(from_grant(access(WIDE), "c001"), "ask");
    let later = from_grant(access(WIDE), "c009");
    let settled =
        settled_channel_access(&[earlier.clone(), later], 0, &profiles).expect("a settle");
    assert_eq!(settled, from_grant(earlier.clone(), "c009"));

    let entire = with_channel(access(WIDE), "ask");
    let settled = settled_channel_access(
        &[entire.clone(), from_grant(access(WIDE), "c004")],
        0,
        &profiles,
    )
    .expect("a settle");
    assert_eq!(settled, from_grant(entire.clone(), "c004"));

    // A version inheriting a from-grant default without an order key reads nothing, so the
    // settled value under the winner's equally ranked profile stops reading history.
    if let Some(defaults) = profiles.get_mut(NARROW) {
        defaults.read_history = true;
        defaults.history_boundary = FROM_GRANT.to_string();
    }
    let reads_entire = with_channel(access(OTHER_NARROW), "ask");
    let reads_nothing = access(NARROW);
    let settled =
        settled_channel_access(&[reads_entire, reads_nothing], 0, &profiles).expect("a settle");
    assert_eq!(settled.access_profile_id, OTHER_NARROW);
    assert!(!settled.read_history && !settled.read_history_inherits_profile);
}

/// The strictest channel override wins, and a scratch override takes the strictest override
/// any version applies to scratch work once any version sets one.
#[test]
fn the_strictest_channel_and_scratch_overrides_win() {
    let profiles = profiles();
    let channel_only = with_channel(access(WIDE), "auto_approve");
    let scratch = with_scratch(with_channel(access(WIDE), "unattended"), "provider_custom");

    let settled =
        settled_channel_access(&[scratch, channel_only.clone()], 1, &profiles).expect("a settle");

    assert_eq!(settled, with_scratch(channel_only, "auto_approve"));
}
