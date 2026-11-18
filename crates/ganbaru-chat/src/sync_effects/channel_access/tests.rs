use super::*;

/// Access that reads entire history, participates, and sets no override.
fn access() -> ChannelAccess {
    ChannelAccess {
        read_history: true,
        participate: true,
        history_boundary: "entire".to_string(),
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
    access.history_from_order_key = Some(key.to_string());
    access
}

/// A scratch override replaces the channel override, so an edit that clears it inherits the
/// stricter channel override; a concurrent looser explicit scratch override the priority kept
/// settles onto that stricter override.
#[test]
fn a_cleared_scratch_override_settles_a_looser_explicit_one_onto_the_channel_override() {
    let inheriting = with_channel(access(), "ask");
    let looser = with_scratch(with_channel(access(), "ask"), "unattended");

    let settled = settled_channel_access(&[inheriting, looser.clone()], 1);

    assert_eq!(settled, Some(with_scratch(looser, "ask")));
}

/// Versions where the winner is already the narrowest need no settle.
#[test]
fn a_winner_that_is_already_the_narrowest_needs_no_settle() {
    let mut narrower = with_channel(access(), "ask");
    narrower.participate = false;
    let wider = access();

    assert_eq!(settled_channel_access(&[narrower.clone(), wider], 0), None);
    assert_eq!(
        settled_channel_access(&[narrower.clone(), narrower], 0),
        None
    );
}

/// Each clear a version holds is kept, even when the priority kept another version that
/// narrowed a different part.
#[test]
fn clears_of_every_version_are_kept() {
    let mut clears_participation = access();
    clears_participation.participate = false;
    let mut clears_history = with_channel(access(), "ask");
    clears_history.read_history = false;

    let settled =
        settled_channel_access(&[clears_participation, clears_history], 1).expect("a settle");

    assert!(!settled.participate);
    assert!(!settled.read_history);
    assert_eq!(settled.runtime_approval_policy.as_deref(), Some("ask"));
}

/// The latest boundary any version reads from wins, and a version that reads no history
/// clears history reading.
#[test]
fn the_latest_history_boundary_and_a_version_without_history_are_kept() {
    let earlier = with_channel(from_grant(access(), "c001"), "ask");
    let later = from_grant(access(), "c009");
    let settled = settled_channel_access(&[earlier.clone(), later], 0).expect("a settle");
    assert_eq!(settled, from_grant(earlier, "c009"));

    let entire = with_channel(access(), "ask");
    let settled = settled_channel_access(&[entire.clone(), from_grant(access(), "c004")], 0)
        .expect("a settle");
    assert_eq!(settled, from_grant(entire.clone(), "c004"));

    // A from-grant boundary without an order key reads nothing, so the settled value stops
    // reading history.
    let mut reads_nothing = access();
    reads_nothing.history_boundary = FROM_GRANT.to_string();
    let settled = settled_channel_access(&[entire, reads_nothing], 0).expect("a settle");
    assert!(!settled.read_history);
}

/// The strictest channel override wins, and a scratch override takes the strictest override
/// any version applies to scratch work once any version sets one.
#[test]
fn the_strictest_channel_and_scratch_overrides_win() {
    let channel_only = with_channel(access(), "auto_approve");
    let scratch = with_scratch(with_channel(access(), "unattended"), "provider_custom");

    let settled = settled_channel_access(&[scratch, channel_only.clone()], 1).expect("a settle");

    assert_eq!(settled, with_scratch(channel_only, "auto_approve"));
}
