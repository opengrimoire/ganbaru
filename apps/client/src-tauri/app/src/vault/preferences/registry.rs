//! The registry of every vault preference path: where it is stored, how a patch maps to rows,
//! and which values it accepts.
//!
//! Value domains mirror the frontend option sets that write them (`preference-options.ts`,
//! `sidebar-order.ts`, `calendar/types.ts`, and the feature stores).

use ganbaru_sync::domains::preferences::ProfileImage;
use serde_json::Value;

/// Largest canonical JSON text of one portable value, matching the `vault_preferences` check.
pub(crate) const MAX_VALUE_BYTES: usize = 64 * 1024;
/// Largest portable row key in bytes, matching the `vault_preferences` check.
pub(crate) const MAX_KEY_BYTES: usize = 128;
/// Separator between the segments of a registry path and between a path and its child.
pub(crate) const PATH_SEPARATOR: &str = ".";

/// Highest color index of the shared palette (`PALETTE_SIZE - 1` in `calendar/types.ts`).
const PALETTE_LAST_INDEX: i64 = 31;
/// Upper bound on projects that one per-child preference keeps entries for.
const MAX_PROJECT_CHILDREN: u16 = 1024;
/// Upper bound on ids in one stored list.
const MAX_LIST_ITEMS: usize = 4096;
/// Upper bound on one stored id.
const MAX_ID_BYTES: usize = 256;
/// Upper bound on a short free-form label or identifier value.
const MAX_LABEL_BYTES: usize = 256;

/// Where a preference lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scope {
    /// One row of `vault_preferences`, shared by every device of the vault.
    Portable,
    /// The device file in the app config directory, never shared.
    Device,
    /// The vault's `config.json`, which native features own.
    VaultConfig,
}

/// How a patch at or below a preference path maps to stored entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shape {
    /// The whole value at the path is one entry.
    Exact,
    /// Each child of the object at the path is its own entry, keyed `path.child`.
    PerChild { max_children: u16 },
}

/// The values a preference accepts. A stored value that fails its rule reads as the default.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ValueRule {
    Bool,
    Integer {
        min: i64,
        max: i64,
    },
    /// One integer of a fixed option set.
    IntegerChoice(&'static [i64]),
    /// A finite number within the inclusive bounds.
    Number {
        min: f64,
        max: f64,
    },
    Choice(&'static [&'static str]),
    Text {
        max_bytes: usize,
    },
    /// An array of non-empty strings.
    IdList {
        max_items: usize,
        max_id_bytes: usize,
    },
    /// Any JSON value whose canonical text fits the bound.
    Json {
        max_bytes: usize,
    },
    /// JSON `null` or a value the inner rule accepts.
    Nullable(&'static ValueRule),
    /// A profile picture reference, the one value shape the sync engine reads: the managed
    /// file path and its exact size, as [`ProfileImage`] parses it.
    ProfileImage,
}

impl ValueRule {
    /// Whether `value` belongs to this rule's domain.
    pub(crate) fn accepts(&self, value: &Value) -> bool {
        match self {
            Self::Bool => value.is_boolean(),
            Self::Integer { min, max } => value
                .as_i64()
                .is_some_and(|number| (*min..=*max).contains(&number)),
            Self::IntegerChoice(options) => value
                .as_i64()
                .is_some_and(|number| options.contains(&number)),
            Self::Number { min, max } => value
                .as_f64()
                .is_some_and(|number| number.is_finite() && *min <= number && number <= *max),
            Self::Choice(options) => value.as_str().is_some_and(|text| options.contains(&text)),
            Self::Text { max_bytes } => value.as_str().is_some_and(|text| text.len() <= *max_bytes),
            Self::IdList {
                max_items,
                max_id_bytes,
            } => value.as_array().is_some_and(|items| {
                items.len() <= *max_items
                    && items.iter().all(|item| {
                        item.as_str()
                            .is_some_and(|id| !id.is_empty() && id.len() <= *max_id_bytes)
                    })
            }),
            Self::Json { max_bytes } => {
                serde_json::to_vec(value).is_ok_and(|text| text.len() <= *max_bytes)
            }
            Self::Nullable(inner) => value.is_null() || inner.accepts(value),
            Self::ProfileImage => ProfileImage::from_json(value).is_some(),
        }
    }
}

/// One registered preference path.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PreferenceSpec {
    /// Dotted path from the root of the effective view.
    pub path: &'static str,
    pub scope: Scope,
    pub shape: Shape,
    /// The rule for the value, or for each child of a per-child path.
    pub rule: ValueRule,
}

impl PreferenceSpec {
    /// The path split into its segments.
    pub(crate) fn segments(&self) -> impl Iterator<Item = &'static str> {
        self.path.split(PATH_SEPARATOR)
    }

    /// The row key of `child` under this per-child path.
    pub(crate) fn child_key(&self, child: &str) -> String {
        format!("{}{PATH_SEPARATOR}{child}", self.path)
    }
}

/// Paths are unique in the registry, so a spec is identified by its path.
impl PartialEq for PreferenceSpec {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl Eq for PreferenceSpec {}

const fn spec(path: &'static str, scope: Scope, rule: ValueRule) -> PreferenceSpec {
    PreferenceSpec {
        path,
        scope,
        shape: Shape::Exact,
        rule,
    }
}

const fn per_project(path: &'static str, rule: ValueRule) -> PreferenceSpec {
    PreferenceSpec {
        path,
        scope: Scope::Portable,
        shape: Shape::PerChild {
            max_children: MAX_PROJECT_CHILDREN,
        },
        rule,
    }
}

const fn portable(path: &'static str, rule: ValueRule) -> PreferenceSpec {
    spec(path, Scope::Portable, rule)
}

const fn device(path: &'static str, rule: ValueRule) -> PreferenceSpec {
    spec(path, Scope::Device, rule)
}

const BOOL: ValueRule = ValueRule::Bool;
const PALETTE_COLOR: ValueRule = ValueRule::Integer {
    min: 0,
    max: PALETTE_LAST_INDEX,
};
const LABEL: ValueRule = ValueRule::Text {
    max_bytes: MAX_LABEL_BYTES,
};
const IDS: ValueRule = ValueRule::IdList {
    max_items: MAX_LIST_ITEMS,
    max_id_bytes: MAX_ID_BYTES,
};
const SMALL_JSON: ValueRule = ValueRule::Json { max_bytes: 4096 };
const LARGE_JSON: ValueRule = ValueRule::Json {
    max_bytes: MAX_VALUE_BYTES,
};
const SIDEBAR_ORDER: ValueRule = ValueRule::Choice(&[
    "name-asc",
    "name-desc",
    "modified-desc",
    "modified-asc",
    "created-desc",
    "created-asc",
    "custom",
]);
const CONTACT_TRUST: ValueRule = ValueRule::Choice(&[
    "notAllowed",
    "once",
    "sevenDays",
    "thirtyDays",
    "untilRevoked",
]);
const FOCUS_BREAK_SOUND_INTERVAL: ValueRule = ValueRule::IntegerChoice(&[0, 10, 15, 30, 60]);
const FOCUS_ESC_PRESSES: ValueRule = ValueRule::IntegerChoice(&[1, 3, 10, 20, 50]);
const FOCUS_EXTENSION_LIMIT: ValueRule = ValueRule::IntegerChoice(&[1, 3, 5, 10, 15]);

/// Every preference path the app reads or writes. Paths never nest inside one another.
pub(crate) const REGISTRY: &[PreferenceSpec] = &[
    spec("distractions", Scope::VaultConfig, LARGE_JSON),
    spec("chat", Scope::VaultConfig, LARGE_JSON),
    portable("profile.displayName", LABEL),
    portable("profile.fullName", ValueRule::Text { max_bytes: 512 }),
    portable("profile.image", ValueRule::ProfileImage),
    portable("profile.useImage", BOOL),
    portable("profile.color", PALETTE_COLOR),
    portable("profile.imageCrop", SMALL_JSON),
    portable("preferences.language", ValueRule::Text { max_bytes: 64 }),
    portable(
        "preferences.eventTimezoneDisplay",
        ValueRule::Choice(&["device", "homeZone"]),
    ),
    portable(
        "preferences.calendarTimeFormat",
        ValueRule::Choice(&["24h", "12h"]),
    ),
    portable("preferences.calendarDimPastEvents", BOOL),
    portable(
        "preferences.focusIdleThresholdMinutes",
        ValueRule::IntegerChoice(&[1, 2, 3, 4, 5, 10, 15]),
    ),
    portable("preferences.focusIdlePauseOnEventCreate", BOOL),
    portable(
        "preferences.focusBreakFinishedRepeatSeconds",
        FOCUS_BREAK_SOUND_INTERVAL,
    ),
    portable(
        "preferences.focusBreakEndWarningSeconds",
        FOCUS_BREAK_SOUND_INTERVAL,
    ),
    portable(
        "preferences.focusBreakEndEscPresses",
        ValueRule::Nullable(&FOCUS_ESC_PRESSES),
    ),
    portable(
        "preferences.focusBreakExtensionLimit",
        ValueRule::Nullable(&FOCUS_EXTENSION_LIMIT),
    ),
    portable(
        "preferences.focusPauseNotificationIntervalMinutes",
        ValueRule::IntegerChoice(&[0, 3, 5, 10, 15]),
    ),
    portable("preferences.musicPauseOnPomodoroPause", BOOL),
    portable("preferences.musicPauseSoundscapeWithMusic", BOOL),
    portable("preferences.quickNotesDefaultColor", PALETTE_COLOR),
    portable(
        "preferences.notesDefaultOpenMode",
        ValueRule::Choice(&["side", "center", "full"]),
    ),
    portable("preferences.notesSidebarOrder", SIDEBAR_ORDER),
    portable("preferences.projectsSidebarOrder", SIDEBAR_ORDER),
    portable("preferences.chatSidebarOrder", SIDEBAR_ORDER),
    portable("preferences.notesShowPropertyIcons", BOOL),
    portable("preferences.projectsShowPropertyIcons", BOOL),
    device(
        "preferences.fontFamilyId",
        ValueRule::Text { max_bytes: 64 },
    ),
    device(
        "preferences.fontScale",
        ValueRule::Number {
            min: 0.85,
            max: 1.3,
        },
    ),
    device(
        "preferences.calendarViewMode",
        ValueRule::Choice(&["day", "workweek", "week", "month"]),
    ),
    device("preferences.titleBarVisibility", SMALL_JSON),
    device("preferences.notesMentionNotificationsEnabled", BOOL),
    device("preferences.notesReminderNotificationsEnabled", BOOL),
    device("preferences.notesUserMentionNotificationsEnabled", BOOL),
    device("preferences.notesTaskMentionNotificationsEnabled", BOOL),
    device("preferences.notesNotificationIncludeContent", BOOL),
    portable("contacts.trustDefaults.invite", CONTACT_TRUST),
    portable("contacts.trustDefaults.message", CONTACT_TRUST),
    portable("theme.quickToggleLightId", LABEL),
    portable("theme.quickToggleDarkId", LABEL),
    device("theme.activeId", LABEL),
    portable("updates.autoNotifications", BOOL),
    device("updates.lastAutoCheckAt", ValueRule::Text { max_bytes: 64 }),
    portable("projects.starredProjectIds", IDS),
    portable("projects.starredViewIds", IDS),
    portable("projects.iconPicker.askEveryTime", BOOL),
    portable("projects.iconPicker.defaultColor", PALETTE_COLOR),
    device("projects.activeProjectId", LABEL),
    device("projects.activeViewIds", LARGE_JSON),
    device("projects.sidebarCollapsed", BOOL),
    device("projects.sidebarCollapsedProjectIds", IDS),
    device("projects.showInactiveProjects", BOOL),
    device("projects.iconPicker.recent", IDS),
    portable("notes.favoritePageIds", IDS),
    portable("notes.favoriteFolderIds", IDS),
    per_project("notes.sidebarOrderByProject", SIDEBAR_ORDER),
    per_project("notes.customOrderByProject", IDS),
    portable("notes.showArchivedInPickers", BOOL),
    portable("notes.databaseViewTabDisplay", LARGE_JSON),
    portable("notes.coverPicker.defaultColor", PALETTE_COLOR),
    portable("notes.coverPicker.askEveryTime", BOOL),
    device("notes.recentPageIds", IDS),
    device("notes.selectedPageId", LABEL),
    device("notes.sidebarCollapsedFolderIds", IDS),
    portable("chatSidebar.starredChannelIds", IDS),
    per_project("chatSidebar.orderByProject", SIDEBAR_ORDER),
    per_project("chatSidebar.customOrderByProject", IDS),
    device("chatSidebar.showArchivedInPickers", BOOL),
    portable("music.review.autoplay", BOOL),
    portable("music.sources.localName", LABEL),
    portable("music.sources.youtubeName", LABEL),
];

/// Where a patch path lands.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Target<'a> {
    /// The whole value of an exact path, or the whole child object of a per-child path.
    Whole(&'static PreferenceSpec),
    /// One child of a per-child path.
    Child(&'static PreferenceSpec, &'a str),
}

/// Finds the registered entry a patch at `path` writes, or explains why there is none. A
/// vault config path may reach any depth, since native features own the shape below it.
pub(crate) fn route(path: &[String]) -> Result<Target<'_>, String> {
    let joined = path.join(PATH_SEPARATOR);
    let unregistered = || format!("preference path {joined} is not registered");
    let spec = REGISTRY
        .iter()
        .find(|spec| {
            let segments = spec.segments().collect::<Vec<_>>();
            path.len() >= segments.len()
                && segments
                    .iter()
                    .zip(path)
                    .all(|(segment, part)| *segment == part.as_str())
        })
        .ok_or_else(unregistered)?;
    let depth = spec.segments().count();
    let rest = &path[depth..];
    match (spec.scope, spec.shape, rest) {
        (Scope::VaultConfig, _, _) | (_, _, []) => Ok(Target::Whole(spec)),
        (_, Shape::PerChild { .. }, [child]) => {
            validate_child_key(spec, child)?;
            Ok(Target::Child(spec, child.as_str()))
        }
        _ => Err(unregistered()),
    }
}

/// Refuses an empty child key or one whose row key would exceed [`MAX_KEY_BYTES`].
pub(crate) fn validate_child_key(spec: &PreferenceSpec, child: &str) -> Result<(), String> {
    if child.is_empty() {
        return Err(format!("{} refuses an empty child key", spec.path));
    }
    if spec.child_key(child).len() > MAX_KEY_BYTES {
        return Err(format!(
            "{} child key exceeds {MAX_KEY_BYTES} bytes",
            spec.path
        ));
    }
    Ok(())
}

/// The registered spec and child that a stored portable row key names, if any.
pub(crate) fn portable_row(key: &str) -> Option<(&'static PreferenceSpec, Option<&str>)> {
    REGISTRY
        .iter()
        .filter(|spec| spec.scope == Scope::Portable)
        .find_map(|spec| match spec.shape {
            Shape::Exact => (key == spec.path).then_some((spec, None)),
            Shape::PerChild { .. } => key
                .strip_prefix(spec.path)
                .and_then(|rest| rest.strip_prefix(PATH_SEPARATOR))
                .filter(|child| !child.is_empty())
                .map(|child| (spec, Some(child))),
        })
}

/// The device spec registered at exactly `key`, if any.
pub(crate) fn device_spec(key: &str) -> Option<&'static PreferenceSpec> {
    REGISTRY
        .iter()
        .find(|spec| spec.scope == Scope::Device && spec.path == key)
}

/// The specs of one scope.
pub(crate) fn specs(scope: Scope) -> impl Iterator<Item = &'static PreferenceSpec> {
    REGISTRY.iter().filter(move |spec| spec.scope == scope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn path(dotted: &str) -> Vec<String> {
        dotted.split('.').map(str::to_owned).collect()
    }

    #[test]
    fn preferences_registry_paths_are_unique_and_never_nest() {
        for (index, spec) in REGISTRY.iter().enumerate() {
            assert!(spec.path.len() <= MAX_KEY_BYTES, "{}", spec.path);
            assert!(
                spec.segments().all(|segment| !segment.is_empty()),
                "{}",
                spec.path
            );
            for other in &REGISTRY[index + 1..] {
                assert_ne!(spec.path, other.path);
                for (outer, inner) in [(spec, other), (other, spec)] {
                    assert!(
                        !inner
                            .path
                            .starts_with(&format!("{}{PATH_SEPARATOR}", outer.path)),
                        "{} nests inside {}",
                        inner.path,
                        outer.path
                    );
                }
            }
            if spec.scope != Scope::Portable {
                assert_eq!(spec.shape, Shape::Exact, "{}", spec.path);
            }
        }
        let vault_config = specs(Scope::VaultConfig)
            .map(|spec| spec.path)
            .collect::<Vec<_>>();
        assert_eq!(vault_config, ["distractions", "chat"]);
    }

    #[test]
    fn preferences_registry_covers_the_frontend_key_inventory() {
        let portable = [
            "profile.displayName",
            "profile.imageCrop",
            "preferences.language",
            "preferences.focusBreakEndEscPresses",
            "contacts.trustDefaults.invite",
            "theme.quickToggleDarkId",
            "notes.coverPicker.askEveryTime",
            "music.sources.youtubeName",
        ];
        let device = [
            "preferences.fontScale",
            "preferences.titleBarVisibility",
            "theme.activeId",
            "projects.iconPicker.recent",
            "notes.selectedPageId",
            "chatSidebar.showArchivedInPickers",
        ];
        for key in portable {
            assert!(portable_row(key).is_some(), "{key}");
        }
        for key in device {
            assert!(device_spec(key).is_some(), "{key}");
        }
        assert_eq!(specs(Scope::Portable).count(), 49);
        assert_eq!(specs(Scope::Device).count(), 21);
    }

    #[test]
    fn preferences_route_exact_per_child_and_vault_config_paths() {
        let theme = path("theme.activeId");
        assert!(matches!(route(&theme), Ok(Target::Whole(spec)) if spec.path == "theme.activeId"));
        let orders = path("notes.customOrderByProject");
        assert!(
            matches!(route(&orders), Ok(Target::Whole(spec)) if spec.path == "notes.customOrderByProject")
        );
        let child = path("notes.customOrderByProject.project-1");
        assert!(
            matches!(route(&child), Ok(Target::Child(spec, "project-1")) if spec.path == "notes.customOrderByProject")
        );
        let rules = path("distractions.rules.0.domain");
        assert!(matches!(route(&rules), Ok(Target::Whole(spec)) if spec.path == "distractions"));

        for refused in [
            "theme",
            "theme.activeId.nested",
            "preferences",
            "notes.customOrderByProject.project-1.extra",
            "preferences.unknownKey",
            "unknown",
        ] {
            let error = route(&path(refused)).unwrap_err();
            assert!(error.contains(refused), "{refused}: {error}");
        }
    }

    #[test]
    fn preferences_per_child_keys_stay_within_the_row_key_bound() {
        let spec = REGISTRY
            .iter()
            .find(|spec| spec.path == "chatSidebar.customOrderByProject")
            .unwrap();
        let room = MAX_KEY_BYTES - spec.path.len() - 1;
        assert!(validate_child_key(spec, &"p".repeat(room)).is_ok());
        assert!(validate_child_key(spec, &"p".repeat(room + 1)).is_err());
        assert!(validate_child_key(spec, "").is_err());
        assert_eq!(
            portable_row("chatSidebar.customOrderByProject.a.b"),
            Some((spec, Some("a.b")))
        );
        assert_eq!(portable_row("chatSidebar.customOrderByProject."), None);
        assert_eq!(portable_row("chatSidebar.customOrderByProject"), None);
    }

    #[test]
    fn preferences_value_rules_accept_and_refuse_at_their_edges() {
        let integer = ValueRule::Integer { min: 0, max: 31 };
        assert!(integer.accepts(&json!(0)) && integer.accepts(&json!(31)));
        assert!(!integer.accepts(&json!(-1)) && !integer.accepts(&json!(32)));
        assert!(!integer.accepts(&json!(1.5)) && !integer.accepts(&json!("1")));

        assert!(FOCUS_ESC_PRESSES.accepts(&json!(50)));
        assert!(!FOCUS_ESC_PRESSES.accepts(&json!(2)));
        let nullable = ValueRule::Nullable(&FOCUS_ESC_PRESSES);
        assert!(nullable.accepts(&json!(null)) && nullable.accepts(&json!(10)));
        assert!(!FOCUS_ESC_PRESSES.accepts(&json!(null)));

        let number = ValueRule::Number {
            min: 0.85,
            max: 1.3,
        };
        assert!(number.accepts(&json!(0.85)) && number.accepts(&json!(1.3)));
        assert!(!number.accepts(&json!(0.84)) && !number.accepts(&json!(1.31)));

        assert!(SIDEBAR_ORDER.accepts(&json!("custom")));
        assert!(!SIDEBAR_ORDER.accepts(&json!("Custom")));
        assert!(BOOL.accepts(&json!(false)) && !BOOL.accepts(&json!(0)));

        let text = ValueRule::Text { max_bytes: 4 };
        assert!(text.accepts(&json!("")) && text.accepts(&json!("abcd")));
        assert!(!text.accepts(&json!("abcde")) && !text.accepts(&json!("éé!")));

        let ids = ValueRule::IdList {
            max_items: 2,
            max_id_bytes: 3,
        };
        assert!(ids.accepts(&json!([])) && ids.accepts(&json!(["abc", "d"])));
        assert!(!ids.accepts(&json!(["a", "b", "c"])));
        assert!(!ids.accepts(&json!(["abcd"])));
        assert!(!ids.accepts(&json!([""])) && !ids.accepts(&json!([1])));
        assert!(!ids.accepts(&json!("abc")));

        let small = ValueRule::Json { max_bytes: 8 };
        assert!(small.accepts(&json!({"a": 12})));
        assert!(!small.accepts(&json!({"a": 123})));

        let path = format!("profile/{}.png", "a".repeat(64));
        let picture = ValueRule::ProfileImage;
        assert!(picture.accepts(&json!({"path": path, "byteSize": 10})));
        assert!(!picture.accepts(&json!({"path": path})));
        assert!(!picture.accepts(&json!({"path": path, "byteSize": 0})));
        assert!(!picture.accepts(&json!(path)));
        assert!(!picture.accepts(&json!(null)));
    }
}
