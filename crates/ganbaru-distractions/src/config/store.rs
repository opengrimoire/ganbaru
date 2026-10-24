//! Distraction rules in the vault database: assembly into [`DistractionsConfig`] and the edits
//! Settings makes, one rule at a time so concurrent edits on other devices merge per rule.
//!
//! Host and app rules are keyed by their content as the sync manifest derives it, so removing a
//! rule and adding it again on another device writes the same row. Assembly is lenient: a row
//! that arrived from another device without its companions (a custom category without hosts, a
//! usage limit without entries or budget) is left out instead of failing the whole
//! configuration.

use std::collections::{HashMap, HashSet};
use std::fmt;

use ganbaru_sync_contracts::bounds::MAX_BLOB_FIELD_BYTES;
use ganbaru_sync_contracts::derived_key;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqliteConnection, SqlitePool};

use super::{
    CategoryRule, CustomCategory, DesktopAppRule, DistractionsConfig, DistractionsMode, HostRule,
    LimitEntryRule, LimitRule, MAX_CATEGORY_NAME_CHARS, MAX_COLOR, MAX_LIMIT_ENTRIES,
    MAX_MATCH_NAMES, MAX_MINUTES_PER_DAY, MAX_MINUTES_PER_WEEK, MAX_NAME_CHARS, MAX_USAGE_LIMITS,
    MobileAppRule, built_in_categories, is_built_in_category, is_rule_host, is_rule_id,
    normalize_app_name, normalize_host, normalize_hosts, normalize_match_names, normalize_name,
    normalize_package,
};
use crate::rules::{app_name_key, is_protected_desktop_app_name};

/// Row key of the settings singleton.
const SETTINGS_KEY: &str = "vault";

/// Key prefix of a host rule, as the sync manifest derives it.
const HOST_RULE_KEY_PREFIX: &str = "host";

/// Key prefix of an app rule, as the sync manifest derives it.
const APP_RULE_KEY_PREFIX: &str = "app";

/// List kind and platform values stored in the rule tables.
const CATEGORY_LIST_KIND: &str = "category";
const DESKTOP_PLATFORM: &str = "desktop";
const MOBILE_PLATFORM: &str = "mobile";

/// Why an edit was refused. The serialized code is what Settings maps to a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuleRefusal {
    /// The rule named by the edit does not exist.
    Missing,
    /// The target cannot be changed this way.
    InvalidTarget,
    /// The category id is not a built-in category.
    InvalidCategory,
    /// A custom category or usage limit name is blank, or a usage limit with several entries
    /// has no name.
    InvalidName,
    /// No valid host was given.
    InvalidHosts,
    /// Another custom category has the same name.
    DuplicateName,
    /// The app name or package is not valid.
    InvalidApp,
    /// The desktop app is protected and cannot be blocked or limited.
    ProtectedSource,
    /// A budget is outside its range.
    InvalidMinutes,
    /// A usage limit has neither a daily nor a weekly budget.
    InvalidBudget,
    /// A limit entry has no source or an invalid id.
    InvalidSources,
    /// Two limit entries share an id or a source.
    DuplicateSource,
    /// The rules would exceed what one vault keeps or one replicated value carries.
    TooLarge,
}

impl RuleRefusal {
    /// The serialized code.
    pub fn code(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::InvalidTarget => "invalid-target",
            Self::InvalidCategory => "invalid-category",
            Self::InvalidName => "invalid-name",
            Self::InvalidHosts => "invalid-hosts",
            Self::DuplicateName => "duplicate-name",
            Self::InvalidApp => "invalid-app",
            Self::ProtectedSource => "protected-source",
            Self::InvalidMinutes => "invalid-minutes",
            Self::InvalidBudget => "invalid-budget",
            Self::InvalidSources => "invalid-sources",
            Self::DuplicateSource => "duplicate-source",
            Self::TooLarge => "too-large",
        }
    }
}

/// A refused edit or a storage failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RulesError {
    Refused(RuleRefusal),
    Storage(String),
}

impl fmt::Display for RulesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(refusal) => {
                write!(formatter, "distraction rule refused: {}", refusal.code())
            }
            Self::Storage(message) => write!(formatter, "distraction rules storage: {message}"),
        }
    }
}

impl std::error::Error for RulesError {}

impl From<RuleRefusal> for RulesError {
    fn from(refusal: RuleRefusal) -> Self {
        Self::Refused(refusal)
    }
}

fn storage(context: &'static str) -> impl FnOnce(sqlx::Error) -> RulesError {
    move |error| RulesError::Storage(format!("{context}: {error}"))
}

/// A switch of the settings row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SettingFlag {
    Enabled,
    BlockDuringFocus,
    BlockDuringShortBreaks,
    BlockDuringLongBreaks,
    PauseDuringFocusPause,
    DesktopEnabled,
    DesktopBlockDuringFocus,
    DesktopBlockDuringShortBreaks,
    DesktopBlockDuringLongBreaks,
    DesktopPauseDuringFocusPause,
    MobileEnabled,
    MobileBlockDuringFocus,
    MobileBlockDuringShortBreaks,
    MobileBlockDuringLongBreaks,
    MobilePauseDuringFocusPause,
    LimitsEnabled,
}

impl SettingFlag {
    /// Every switch, for resets.
    const ALL: [Self; 16] = [
        Self::Enabled,
        Self::BlockDuringFocus,
        Self::BlockDuringShortBreaks,
        Self::BlockDuringLongBreaks,
        Self::PauseDuringFocusPause,
        Self::DesktopEnabled,
        Self::DesktopBlockDuringFocus,
        Self::DesktopBlockDuringShortBreaks,
        Self::DesktopBlockDuringLongBreaks,
        Self::DesktopPauseDuringFocusPause,
        Self::MobileEnabled,
        Self::MobileBlockDuringFocus,
        Self::MobileBlockDuringShortBreaks,
        Self::MobileBlockDuringLongBreaks,
        Self::MobilePauseDuringFocusPause,
        Self::LimitsEnabled,
    ];

    /// The settings column; never built from input.
    fn column(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::BlockDuringFocus => "block_during_focus",
            Self::BlockDuringShortBreaks => "block_during_short_breaks",
            Self::BlockDuringLongBreaks => "block_during_long_breaks",
            Self::PauseDuringFocusPause => "pause_during_focus_pause",
            Self::DesktopEnabled => "desktop_enabled",
            Self::DesktopBlockDuringFocus => "desktop_block_during_focus",
            Self::DesktopBlockDuringShortBreaks => "desktop_block_during_short_breaks",
            Self::DesktopBlockDuringLongBreaks => "desktop_block_during_long_breaks",
            Self::DesktopPauseDuringFocusPause => "desktop_pause_during_focus_pause",
            Self::MobileEnabled => "mobile_enabled",
            Self::MobileBlockDuringFocus => "mobile_block_during_focus",
            Self::MobileBlockDuringShortBreaks => "mobile_block_during_short_breaks",
            Self::MobileBlockDuringLongBreaks => "mobile_block_during_long_breaks",
            Self::MobilePauseDuringFocusPause => "mobile_pause_during_focus_pause",
            Self::LimitsEnabled => "limits_enabled",
        }
    }
}

/// A global host list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HostList {
    Blocked,
    Exception,
    Allowed,
}

impl HostList {
    fn kind(self) -> &'static str {
        match self {
            Self::Blocked => "blocked",
            Self::Exception => "exception",
            Self::Allowed => "allowed",
        }
    }
}

/// The rule a toggle or removal names.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RuleTarget {
    Setting { setting: SettingFlag },
    Category { id: String },
    CustomCategory { id: String },
    Host { list: HostList, host: String },
    DesktopApp { name: String },
    MobileApp { package_name: String },
    UsageLimit { id: String },
}

/// A limit entry as the usage limit editor submits it; blank fields mean no source.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitEntryDraft {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub color: Option<i64>,
    #[serde(default)]
    pub website_host: String,
    #[serde(default)]
    pub mobile_app_name: String,
    #[serde(default)]
    pub mobile_app_package: String,
    #[serde(default)]
    pub desktop_app_name: String,
    #[serde(default)]
    pub desktop_app_match_names: Vec<String>,
}

/// A usage limit as the editor submits it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitDraft {
    #[serde(default)]
    pub name: String,
    pub minutes_per_day: Option<i64>,
    pub minutes_per_week: Option<i64>,
    pub entries: Vec<LimitEntryDraft>,
}

/// One change to the rules.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleEdit {
    /// Switches between blocking by list and allowing by list.
    SetMode(DistractionsMode),
    /// Turns a setting, category, custom category, host, app, or usage limit on or off. A
    /// toggle of a rule that no longer exists changes nothing.
    SetEnabled { target: RuleTarget, enabled: bool },
    /// Adds hosts to a global list; hosts already listed keep their state.
    AddHosts { list: HostList, hosts: Vec<String> },
    /// Blocks a desktop app; an app already blocked keeps its state.
    AddDesktopApp {
        name: String,
        match_names: Vec<String>,
    },
    /// Blocks an Android app; an app already blocked keeps its state.
    AddMobileApp { name: String, package_name: String },
    /// Removes a custom category, host, app, or usage limit.
    Remove(RuleTarget),
    /// Creates a custom category when `id` is `None`, otherwise renames it and replaces its
    /// hosts; hosts it keeps keep their state.
    SaveCustomCategory {
        id: Option<String>,
        name: String,
        hosts: Vec<String>,
    },
    /// Creates a usage limit when `id` is `None`, otherwise replaces its name, budget, and
    /// entries.
    SaveUsageLimit {
        id: Option<String>,
        draft: LimitDraft,
    },
    /// Restores every default: settings switches on, categories at their defaults, and no
    /// custom categories, hosts, apps, or usage limits.
    Reset,
}

/// Applies `edit` in one transaction and returns the rules as they stand after it.
/// `now_ms` stamps rules the edit creates.
pub async fn apply(
    pool: &SqlitePool,
    edit: &RuleEdit,
    now_ms: i64,
) -> Result<DistractionsConfig, RulesError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(storage("begin distraction rule edit"))?;
    apply_edit(&mut tx, edit, now_ms.max(1)).await?;
    let config = load(&mut tx).await?;
    tx.commit()
        .await
        .map_err(storage("commit distraction rule edit"))?;
    Ok(config)
}

/// Reads the rules from `pool`.
pub async fn read(pool: &SqlitePool) -> Result<DistractionsConfig, RulesError> {
    let mut conn = pool
        .acquire()
        .await
        .map_err(storage("acquire distraction rules connection"))?;
    load(&mut conn).await
}

async fn apply_edit(
    conn: &mut SqliteConnection,
    edit: &RuleEdit,
    now_ms: i64,
) -> Result<(), RulesError> {
    match edit {
        RuleEdit::SetMode(mode) => set_mode(conn, *mode).await,
        RuleEdit::SetEnabled { target, enabled } => set_enabled(conn, target, *enabled).await,
        RuleEdit::AddHosts { list, hosts } => {
            let hosts = normalize_hosts(hosts)
                .filter(|hosts| !hosts.is_empty())
                .ok_or(RuleRefusal::InvalidHosts)?;
            insert_hosts(conn, list.kind(), "", None, &hosts, now_ms).await
        }
        RuleEdit::AddDesktopApp { name, match_names } => {
            add_desktop_app(conn, name, match_names, now_ms).await
        }
        RuleEdit::AddMobileApp { name, package_name } => {
            add_mobile_app(conn, name, package_name, now_ms).await
        }
        RuleEdit::Remove(target) => remove(conn, target).await,
        RuleEdit::SaveCustomCategory { id, name, hosts } => {
            save_custom_category(conn, id.as_deref(), name, hosts, now_ms).await
        }
        RuleEdit::SaveUsageLimit { id, draft } => {
            save_usage_limit(conn, id.as_deref(), draft, now_ms).await
        }
        RuleEdit::Reset => reset(conn).await,
    }
}

async fn set_mode(conn: &mut SqliteConnection, mode: DistractionsMode) -> Result<(), RulesError> {
    execute(
        conn,
        "UPDATE distractions_settings SET mode = ? WHERE id = ?",
        &[mode.as_str(), SETTINGS_KEY],
        "set distraction mode",
    )
    .await
    .map(|_| ())
}

async fn execute(
    conn: &mut SqliteConnection,
    sql: &'static str,
    binds: &[&str],
    context: &'static str,
) -> Result<u64, RulesError> {
    let mut query = sqlx::query(sql);
    for bind in binds {
        query = query.bind(*bind);
    }
    query
        .execute(conn)
        .await
        .map(|result| result.rows_affected())
        .map_err(storage(context))
}

async fn set_enabled(
    conn: &mut SqliteConnection,
    target: &RuleTarget,
    enabled: bool,
) -> Result<(), RulesError> {
    let flag = i64::from(enabled);
    match target {
        RuleTarget::Setting { setting } => {
            // The column comes from the fixed `SettingFlag` table, never from input.
            let sql = format!(
                "UPDATE distractions_settings SET {} = ? WHERE id = ?",
                setting.column()
            );
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(flag)
                .bind(SETTINGS_KEY)
                .execute(conn)
                .await
                .map_err(storage("toggle distraction setting"))?;
        }
        RuleTarget::Category { id } => {
            if !is_built_in_category(id) {
                return Err(RuleRefusal::InvalidCategory.into());
            }
            sqlx::query(
                "INSERT INTO distractions_category_rules (id, enabled) VALUES (?, ?)
                 ON CONFLICT(id) DO UPDATE SET enabled = excluded.enabled",
            )
            .bind(id)
            .bind(flag)
            .execute(conn)
            .await
            .map_err(storage("toggle distraction category"))?;
        }
        RuleTarget::CustomCategory { id } => {
            sqlx::query("UPDATE distractions_custom_categories SET enabled = ? WHERE id = ?")
                .bind(flag)
                .bind(id)
                .execute(conn)
                .await
                .map_err(storage("toggle custom distraction category"))?;
        }
        RuleTarget::Host { list, host } => {
            let Some(host) = normalize_host(host) else {
                return Ok(());
            };
            sqlx::query(
                "UPDATE distractions_host_rules SET enabled = ?
                 WHERE list_kind = ? AND scope = '' AND host = ?",
            )
            .bind(flag)
            .bind(list.kind())
            .bind(host)
            .execute(conn)
            .await
            .map_err(storage("toggle distraction host"))?;
        }
        RuleTarget::DesktopApp { name } => {
            if is_protected_desktop_app_name(name) {
                return Ok(());
            }
            sqlx::query(
                "UPDATE distractions_app_rules SET enabled = ?
                 WHERE platform = ? AND identity = ?",
            )
            .bind(flag)
            .bind(DESKTOP_PLATFORM)
            .bind(app_name_key(name))
            .execute(conn)
            .await
            .map_err(storage("toggle desktop distraction app"))?;
        }
        RuleTarget::MobileApp { package_name } => {
            sqlx::query(
                "UPDATE distractions_app_rules SET enabled = ?
                 WHERE platform = ? AND lower(identity) = lower(?)",
            )
            .bind(flag)
            .bind(MOBILE_PLATFORM)
            .bind(package_name.trim())
            .execute(conn)
            .await
            .map_err(storage("toggle mobile distraction app"))?;
        }
        RuleTarget::UsageLimit { id } => {
            sqlx::query("UPDATE distractions_usage_limits SET enabled = ? WHERE id = ?")
                .bind(flag)
                .bind(id)
                .execute(conn)
                .await
                .map_err(storage("toggle usage limit"))?;
        }
    }
    Ok(())
}

async fn remove(conn: &mut SqliteConnection, target: &RuleTarget) -> Result<(), RulesError> {
    match target {
        RuleTarget::Setting { .. } | RuleTarget::Category { .. } => {
            return Err(RuleRefusal::InvalidTarget.into());
        }
        RuleTarget::CustomCategory { id } => {
            execute(
                conn,
                "DELETE FROM distractions_custom_categories WHERE id = ?",
                &[id],
                "remove custom distraction category",
            )
            .await?;
        }
        RuleTarget::Host { list, host } => {
            let Some(host) = normalize_host(host) else {
                return Ok(());
            };
            execute(
                conn,
                "DELETE FROM distractions_host_rules
                 WHERE list_kind = ? AND scope = '' AND host = ?",
                &[list.kind(), &host],
                "remove distraction host",
            )
            .await?;
        }
        RuleTarget::DesktopApp { name } => {
            execute(
                conn,
                "DELETE FROM distractions_app_rules WHERE platform = ? AND identity = ?",
                &[DESKTOP_PLATFORM, &app_name_key(name)],
                "remove desktop distraction app",
            )
            .await?;
        }
        RuleTarget::MobileApp { package_name } => {
            execute(
                conn,
                "DELETE FROM distractions_app_rules
                 WHERE platform = ? AND lower(identity) = lower(?)",
                &[MOBILE_PLATFORM, package_name.trim()],
                "remove mobile distraction app",
            )
            .await?;
        }
        RuleTarget::UsageLimit { id } => {
            execute(
                conn,
                "DELETE FROM distractions_usage_limits WHERE id = ?",
                &[id],
                "remove usage limit",
            )
            .await?;
        }
    }
    Ok(())
}

fn host_rule_key(list_kind: &str, scope: &str, host: &str) -> Result<String, RulesError> {
    derived_key(HOST_RULE_KEY_PREFIX, &[list_kind, scope, host])
        .map(|key| key.to_string())
        .map_err(|error| RulesError::Storage(format!("derive host rule key: {error:?}")))
}

fn app_rule_key(platform: &str, identity: &str) -> Result<String, RulesError> {
    derived_key(APP_RULE_KEY_PREFIX, &[platform, identity])
        .map(|key| key.to_string())
        .map_err(|error| RulesError::Storage(format!("derive app rule key: {error:?}")))
}

/// Inserts normalized hosts in order; a host already in the list keeps its row.
async fn insert_hosts(
    conn: &mut SqliteConnection,
    list_kind: &str,
    scope: &str,
    category_id: Option<&str>,
    hosts: &[String],
    now_ms: i64,
) -> Result<(), RulesError> {
    for (index, host) in hosts.iter().enumerate() {
        let created_at_ms = now_ms.saturating_add(i64::try_from(index).unwrap_or(i64::MAX));
        sqlx::query(
            "INSERT INTO distractions_host_rules
                 (id, list_kind, scope, host, category_id, enabled, created_at_ms)
             VALUES (?, ?, ?, ?, ?, 1, ?)
             ON CONFLICT(id) DO NOTHING",
        )
        .bind(host_rule_key(list_kind, scope, host)?)
        .bind(list_kind)
        .bind(scope)
        .bind(host)
        .bind(category_id)
        .bind(created_at_ms)
        .execute(&mut *conn)
        .await
        .map_err(storage("add distraction host"))?;
    }
    Ok(())
}

async fn add_desktop_app(
    conn: &mut SqliteConnection,
    name: &str,
    match_names: &[String],
    now_ms: i64,
) -> Result<(), RulesError> {
    let name = normalize_app_name(name).ok_or(RuleRefusal::InvalidApp)?;
    if is_protected_desktop_app_name(&name) {
        return Err(RuleRefusal::ProtectedSource.into());
    }
    let match_names = normalize_match_names(&name, match_names);
    if match_names.len() > MAX_MATCH_NAMES {
        return Err(RuleRefusal::TooLarge.into());
    }
    let match_names_json = serde_json::to_string(&match_names)
        .map_err(|error| RulesError::Storage(format!("encode match names: {error}")))?;
    let identity = app_name_key(&name);
    sqlx::query(
        "INSERT INTO distractions_app_rules
             (id, platform, identity, display_name, match_names_json, enabled, created_at_ms)
         VALUES (?, ?, ?, ?, ?, 1, ?)
         ON CONFLICT(id) DO NOTHING",
    )
    .bind(app_rule_key(DESKTOP_PLATFORM, &identity)?)
    .bind(DESKTOP_PLATFORM)
    .bind(&identity)
    .bind(&name)
    .bind(match_names_json)
    .bind(now_ms)
    .execute(conn)
    .await
    .map_err(storage("add desktop distraction app"))?;
    Ok(())
}

/// Android package names are case-sensitive identities, so the package is stored as given;
/// a package already blocked under another case keeps its rule, as Settings has always matched.
async fn add_mobile_app(
    conn: &mut SqliteConnection,
    name: &str,
    package_name: &str,
    now_ms: i64,
) -> Result<(), RulesError> {
    let name = normalize_name(name, MAX_NAME_CHARS).ok_or(RuleRefusal::InvalidApp)?;
    let package_name = normalize_package(package_name).ok_or(RuleRefusal::InvalidApp)?;
    let existing: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM distractions_app_rules
         WHERE platform = ? AND lower(identity) = lower(?)",
    )
    .bind(MOBILE_PLATFORM)
    .bind(&package_name)
    .fetch_one(&mut *conn)
    .await
    .map_err(storage("find mobile distraction app"))?;
    if existing > 0 {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO distractions_app_rules
             (id, platform, identity, display_name, enabled, created_at_ms)
         VALUES (?, ?, ?, ?, 1, ?)
         ON CONFLICT(id) DO NOTHING",
    )
    .bind(app_rule_key(MOBILE_PLATFORM, &package_name)?)
    .bind(MOBILE_PLATFORM)
    .bind(&package_name)
    .bind(&name)
    .bind(now_ms)
    .execute(conn)
    .await
    .map_err(storage("add mobile distraction app"))?;
    Ok(())
}

async fn row_exists(
    conn: &mut SqliteConnection,
    sql: &'static str,
    id: &str,
    context: &'static str,
) -> Result<bool, RulesError> {
    let count: i64 = sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(conn)
        .await
        .map_err(storage(context))?;
    Ok(count > 0)
}

async fn new_rule_id(conn: &mut SqliteConnection) -> Result<String, RulesError> {
    sqlx::query_scalar("SELECT lower(hex(randomblob(16)))")
        .fetch_one(conn)
        .await
        .map_err(storage("generate distraction rule id"))
}

async fn save_custom_category(
    conn: &mut SqliteConnection,
    id: Option<&str>,
    name: &str,
    hosts: &[String],
    now_ms: i64,
) -> Result<(), RulesError> {
    let name = normalize_name(name, MAX_CATEGORY_NAME_CHARS).ok_or(RuleRefusal::InvalidName)?;
    let hosts = normalize_hosts(hosts)
        .filter(|hosts| !hosts.is_empty())
        .ok_or(RuleRefusal::InvalidHosts)?;
    let duplicate: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM distractions_custom_categories
         WHERE lower(name) = lower(?) AND id IS NOT ?",
    )
    .bind(&name)
    .bind(id)
    .fetch_one(&mut *conn)
    .await
    .map_err(storage("find custom distraction category name"))?;
    if duplicate > 0 {
        return Err(RuleRefusal::DuplicateName.into());
    }
    let id = match id {
        Some(id) => {
            let exists = row_exists(
                conn,
                "SELECT count(*) FROM distractions_custom_categories WHERE id = ?",
                id,
                "find custom distraction category",
            )
            .await?;
            if !exists {
                return Err(RuleRefusal::Missing.into());
            }
            execute(
                conn,
                "UPDATE distractions_custom_categories SET name = ? WHERE id = ?",
                &[&name, id],
                "rename custom distraction category",
            )
            .await?;
            id.to_string()
        }
        None => {
            let id = new_rule_id(conn).await?;
            sqlx::query(
                "INSERT INTO distractions_custom_categories (id, created_at_ms, name, enabled)
                 VALUES (?, ?, ?, 1)",
            )
            .bind(&id)
            .bind(now_ms)
            .bind(&name)
            .execute(&mut *conn)
            .await
            .map_err(storage("add custom distraction category"))?;
            id
        }
    };
    let existing: Vec<String> = sqlx::query_scalar(
        "SELECT host FROM distractions_host_rules WHERE list_kind = ? AND scope = ?",
    )
    .bind(CATEGORY_LIST_KIND)
    .bind(&id)
    .fetch_all(&mut *conn)
    .await
    .map_err(storage("read custom distraction category hosts"))?;
    let kept: HashSet<&str> = hosts.iter().map(String::as_str).collect();
    for host in existing.iter().filter(|host| !kept.contains(host.as_str())) {
        execute(
            conn,
            "DELETE FROM distractions_host_rules WHERE list_kind = ? AND scope = ? AND host = ?",
            &[CATEGORY_LIST_KIND, &id, host],
            "remove custom distraction category host",
        )
        .await?;
    }
    insert_hosts(conn, CATEGORY_LIST_KIND, &id, Some(&id), &hosts, now_ms).await
}

/// A limit entry ready to store.
struct NormalizedEntry {
    rule: LimitEntryRule,
    match_names_json: String,
}

impl NormalizedEntry {
    /// Upper bound of the bytes the generic owned-table codec writes for this row: one field
    /// count byte, and for each of the eight columns a tag, an 8-byte integer or a 4-byte length,
    /// and the text.
    fn encoded_bound(&self) -> usize {
        const ROW: usize = 1;
        const FIELD: usize = 1 + 8;
        let rule = &self.rule;
        let texts = [
            Some(rule.id.as_str()),
            rule.name.as_deref(),
            rule.website_host.as_deref(),
            rule.mobile_app_name.as_deref(),
            rule.mobile_app_package.as_deref(),
            rule.desktop_app_name.as_deref(),
            Some(self.match_names_json.as_str()),
        ];
        ROW + FIELD * 8 + texts.iter().flatten().map(|text| text.len()).sum::<usize>()
    }
}

fn optional_source<T>(input: &str, normalize: impl FnOnce(&str) -> Option<T>) -> Option<T> {
    if input.trim().is_empty() {
        None
    } else {
        normalize(input)
    }
}

fn website_display_name(host: &str) -> String {
    let host = host.strip_prefix("www.").unwrap_or(host);
    let label = host.split('.').next().unwrap_or(host);
    let mut chars = label.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => host.to_string(),
    }
}

fn source_keys(entry: &LimitEntryRule) -> Vec<String> {
    let mut keys = Vec::new();
    if let Some(host) = &entry.website_host {
        keys.push(format!("website:{host}"));
    }
    if let Some(app) = entry
        .mobile_app_package
        .as_ref()
        .or(entry.mobile_app_name.as_ref())
    {
        keys.push(format!("mobile-app:{}", app.to_lowercase()));
    }
    if let Some(name) = &entry.desktop_app_name {
        if entry.desktop_app_match_names.is_empty() {
            keys.push(format!("desktop-app:{}", app_name_key(name)));
        }
        for match_name in &entry.desktop_app_match_names {
            keys.push(format!("desktop-app:{}", app_name_key(match_name)));
        }
    }
    keys
}

fn normalize_entry(draft: &LimitEntryDraft) -> Result<NormalizedEntry, RuleRefusal> {
    let id = draft.id.trim();
    if !is_rule_id(id) {
        return Err(RuleRefusal::InvalidSources);
    }
    let website_host = optional_source(&draft.website_host, normalize_host);
    let mobile_app_name = optional_source(&draft.mobile_app_name, |name| {
        normalize_name(name, MAX_NAME_CHARS)
    });
    let mobile_app_package = optional_source(&draft.mobile_app_package, normalize_package);
    if mobile_app_package.is_some() && mobile_app_name.is_none() {
        return Err(RuleRefusal::InvalidSources);
    }
    let desktop_app_name = optional_source(&draft.desktop_app_name, normalize_app_name);
    if website_host.is_none() && mobile_app_name.is_none() && desktop_app_name.is_none() {
        return Err(RuleRefusal::InvalidSources);
    }
    if desktop_app_name
        .as_deref()
        .is_some_and(is_protected_desktop_app_name)
    {
        return Err(RuleRefusal::ProtectedSource);
    }
    let desktop_app_match_names = desktop_app_name
        .as_deref()
        .map(|name| normalize_match_names(name, &draft.desktop_app_match_names))
        .unwrap_or_default();
    if desktop_app_match_names.len() > MAX_MATCH_NAMES {
        return Err(RuleRefusal::TooLarge);
    }
    let match_names_json =
        serde_json::to_string(&desktop_app_match_names).map_err(|_| RuleRefusal::InvalidSources)?;
    Ok(NormalizedEntry {
        rule: LimitEntryRule {
            id: id.to_string(),
            name: normalize_name(&draft.name, MAX_NAME_CHARS),
            color: draft.color.filter(|color| (0..=MAX_COLOR).contains(color)),
            website_host,
            mobile_app_name,
            mobile_app_package,
            desktop_app_name,
            desktop_app_match_names,
        },
        match_names_json,
    })
}

fn checked_minutes(minutes: Option<i64>, max: i64) -> Result<Option<i64>, RuleRefusal> {
    match minutes {
        Some(minutes) if !(1..=max).contains(&minutes) => Err(RuleRefusal::InvalidMinutes),
        minutes => Ok(minutes),
    }
}

/// A usage limit draft checked as the editor checks it.
struct NormalizedLimit {
    name: String,
    minutes_per_day: Option<i64>,
    minutes_per_week: Option<i64>,
    entries: Vec<NormalizedEntry>,
}

fn normalize_limit(draft: &LimitDraft) -> Result<NormalizedLimit, RuleRefusal> {
    let minutes_per_day = checked_minutes(draft.minutes_per_day, MAX_MINUTES_PER_DAY)?;
    let minutes_per_week = checked_minutes(draft.minutes_per_week, MAX_MINUTES_PER_WEEK)?;
    if minutes_per_day.is_none() && minutes_per_week.is_none() {
        return Err(RuleRefusal::InvalidBudget);
    }
    if draft.entries.is_empty() {
        return Err(RuleRefusal::InvalidSources);
    }
    if draft.entries.len() > MAX_LIMIT_ENTRIES {
        return Err(RuleRefusal::TooLarge);
    }
    let mut ids = HashSet::new();
    let mut sources = HashSet::new();
    let mut entries = Vec::with_capacity(draft.entries.len());
    for draft in &draft.entries {
        let entry = normalize_entry(draft)?;
        if !ids.insert(entry.rule.id.clone()) {
            return Err(RuleRefusal::DuplicateSource);
        }
        for key in source_keys(&entry.rule) {
            if !sources.insert(key) {
                return Err(RuleRefusal::DuplicateSource);
            }
        }
        entries.push(entry);
    }
    let encoded = 2 + entries
        .iter()
        .map(NormalizedEntry::encoded_bound)
        .sum::<usize>();
    if encoded > MAX_BLOB_FIELD_BYTES {
        return Err(RuleRefusal::TooLarge);
    }
    let explicit = normalize_name(&draft.name, MAX_NAME_CHARS);
    if entries.len() > 1 && explicit.is_none() {
        return Err(RuleRefusal::InvalidName);
    }
    let first = &entries[0].rule;
    let name = explicit
        .or_else(|| first.name.clone())
        .or_else(|| first.mobile_app_name.clone())
        .or_else(|| first.desktop_app_name.clone())
        .or_else(|| first.website_host.as_deref().map(website_display_name))
        .ok_or(RuleRefusal::InvalidName)?;
    Ok(NormalizedLimit {
        name,
        minutes_per_day,
        minutes_per_week,
        entries,
    })
}

async fn save_usage_limit(
    conn: &mut SqliteConnection,
    id: Option<&str>,
    draft: &LimitDraft,
    now_ms: i64,
) -> Result<(), RulesError> {
    let limit = normalize_limit(draft)?;
    let (limit_count, entry_count): (i64, i64) = sqlx::query_as(
        "SELECT
             (SELECT count(*) FROM distractions_usage_limits WHERE id IS NOT ?1),
             (SELECT count(*) FROM distractions_usage_limit_entries WHERE limit_id IS NOT ?1)",
    )
    .bind(id)
    .fetch_one(&mut *conn)
    .await
    .map_err(storage("count usage limits"))?;
    let other_limits = usize::try_from(limit_count).unwrap_or(usize::MAX);
    let other_entries = usize::try_from(entry_count).unwrap_or(usize::MAX);
    if other_limits >= MAX_USAGE_LIMITS
        || other_entries.saturating_add(limit.entries.len()) > MAX_LIMIT_ENTRIES
    {
        return Err(RuleRefusal::TooLarge.into());
    }
    let id = match id {
        Some(id) => {
            let updated = sqlx::query(
                "UPDATE distractions_usage_limits
                 SET name = ?, minutes_per_day = ?, minutes_per_week = ? WHERE id = ?",
            )
            .bind(&limit.name)
            .bind(limit.minutes_per_day)
            .bind(limit.minutes_per_week)
            .bind(id)
            .execute(&mut *conn)
            .await
            .map_err(storage("update usage limit"))?;
            if updated.rows_affected() == 0 {
                return Err(RuleRefusal::Missing.into());
            }
            id.to_string()
        }
        None => {
            let id = new_rule_id(conn).await?;
            sqlx::query(
                "INSERT INTO distractions_usage_limits
                     (id, created_at_ms, name, enabled, minutes_per_day, minutes_per_week)
                 VALUES (?, ?, ?, 1, ?, ?)",
            )
            .bind(&id)
            .bind(now_ms)
            .bind(&limit.name)
            .bind(limit.minutes_per_day)
            .bind(limit.minutes_per_week)
            .execute(&mut *conn)
            .await
            .map_err(storage("add usage limit"))?;
            id
        }
    };
    let stored = read_entries(conn, Some(&id)).await?;
    let current = stored.get(&id).map(Vec::as_slice).unwrap_or_default();
    let unchanged = current.len() == limit.entries.len()
        && current
            .iter()
            .zip(&limit.entries)
            .all(|(stored, entry)| stored == &entry.rule);
    if unchanged {
        return Ok(());
    }
    execute(
        conn,
        "DELETE FROM distractions_usage_limit_entries WHERE limit_id = ?",
        &[&id],
        "replace usage limit entries",
    )
    .await?;
    for (ordinal, entry) in limit.entries.iter().enumerate() {
        let rule = &entry.rule;
        sqlx::query(
            "INSERT INTO distractions_usage_limit_entries
                 (limit_id, ordinal, id, name, color, website_host, mobile_app_name,
                  mobile_app_package, desktop_app_name, desktop_app_match_names_json)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(i64::try_from(ordinal).unwrap_or(i64::MAX))
        .bind(&rule.id)
        .bind(&rule.name)
        .bind(rule.color)
        .bind(&rule.website_host)
        .bind(&rule.mobile_app_name)
        .bind(&rule.mobile_app_package)
        .bind(&rule.desktop_app_name)
        .bind(&entry.match_names_json)
        .execute(&mut *conn)
        .await
        .map_err(storage("add usage limit entry"))?;
    }
    Ok(())
}

async fn reset(conn: &mut SqliteConnection) -> Result<(), RulesError> {
    set_mode(conn, DistractionsMode::default()).await?;
    for setting in SettingFlag::ALL {
        set_enabled(conn, &RuleTarget::Setting { setting }, true).await?;
    }
    for sql in [
        "DELETE FROM distractions_category_rules",
        "DELETE FROM distractions_host_rules",
        "DELETE FROM distractions_custom_categories",
        "DELETE FROM distractions_app_rules",
        "DELETE FROM distractions_usage_limits",
    ] {
        sqlx::query(sql)
            .execute(&mut *conn)
            .await
            .map_err(storage("reset distraction rules"))?;
    }
    Ok(())
}

fn flag(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<bool, RulesError> {
    row.try_get::<i64, _>(column)
        .map(|value| value != 0)
        .map_err(|error| RulesError::Storage(format!("read {column}: {error}")))
}

fn text(row: &sqlx::sqlite::SqliteRow, column: &str) -> Result<String, RulesError> {
    row.try_get(column)
        .map_err(|error| RulesError::Storage(format!("read {column}: {error}")))
}

fn optional_text(
    row: &sqlx::sqlite::SqliteRow,
    column: &str,
) -> Result<Option<String>, RulesError> {
    row.try_get(column)
        .map_err(|error| RulesError::Storage(format!("read {column}: {error}")))
}

fn optional_integer(
    row: &sqlx::sqlite::SqliteRow,
    column: &str,
) -> Result<Option<i64>, RulesError> {
    row.try_get(column)
        .map_err(|error| RulesError::Storage(format!("read {column}: {error}")))
}

/// Assembles the rules. Rows another device left incomplete are skipped.
pub async fn load(conn: &mut SqliteConnection) -> Result<DistractionsConfig, RulesError> {
    let mut config = DistractionsConfig::default();
    load_settings(conn, &mut config).await?;
    load_categories(conn, &mut config).await?;
    load_hosts(conn, &mut config).await?;
    load_apps(conn, &mut config).await?;
    load_limits(conn, &mut config).await?;
    Ok(config)
}

async fn load_settings(
    conn: &mut SqliteConnection,
    config: &mut DistractionsConfig,
) -> Result<(), RulesError> {
    let Some(row) = sqlx::query("SELECT * FROM distractions_settings WHERE id = ?")
        .bind(SETTINGS_KEY)
        .fetch_optional(&mut *conn)
        .await
        .map_err(storage("read distraction settings"))?
    else {
        return Ok(());
    };
    config.mode = DistractionsMode::from_stored(&text(&row, "mode")?);
    config.enabled = flag(&row, "enabled")?;
    config.block_during_focus = flag(&row, "block_during_focus")?;
    config.block_during_short_breaks = flag(&row, "block_during_short_breaks")?;
    config.block_during_long_breaks = flag(&row, "block_during_long_breaks")?;
    config.pause_during_focus_pause = flag(&row, "pause_during_focus_pause")?;
    let desktop = &mut config.desktop;
    desktop.enabled = flag(&row, "desktop_enabled")?;
    desktop.block_during_focus = flag(&row, "desktop_block_during_focus")?;
    desktop.block_during_short_breaks = flag(&row, "desktop_block_during_short_breaks")?;
    desktop.block_during_long_breaks = flag(&row, "desktop_block_during_long_breaks")?;
    desktop.pause_during_focus_pause = flag(&row, "desktop_pause_during_focus_pause")?;
    let mobile = &mut config.mobile;
    mobile.enabled = flag(&row, "mobile_enabled")?;
    mobile.block_during_focus = flag(&row, "mobile_block_during_focus")?;
    mobile.block_during_short_breaks = flag(&row, "mobile_block_during_short_breaks")?;
    mobile.block_during_long_breaks = flag(&row, "mobile_block_during_long_breaks")?;
    mobile.pause_during_focus_pause = flag(&row, "mobile_pause_during_focus_pause")?;
    config.limits.enabled = flag(&row, "limits_enabled")?;
    Ok(())
}

async fn load_categories(
    conn: &mut SqliteConnection,
    config: &mut DistractionsConfig,
) -> Result<(), RulesError> {
    let rows = sqlx::query("SELECT id, enabled FROM distractions_category_rules")
        .fetch_all(&mut *conn)
        .await
        .map_err(storage("read distraction categories"))?;
    let mut stored = HashMap::new();
    for row in &rows {
        stored.insert(text(row, "id")?, flag(row, "enabled")?);
    }
    config.blocked_categories = built_in_categories()
        .iter()
        .map(|category| CategoryRule {
            id: category.id.clone(),
            enabled: stored
                .get(&category.id)
                .copied()
                .unwrap_or(category.default_enabled),
        })
        .collect();
    Ok(())
}

async fn load_hosts(
    conn: &mut SqliteConnection,
    config: &mut DistractionsConfig,
) -> Result<(), RulesError> {
    let rows = sqlx::query(
        "SELECT list_kind, scope, host, enabled FROM distractions_host_rules
         ORDER BY created_at_ms, host",
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(storage("read distraction hosts"))?;
    let mut category_hosts: HashMap<String, Vec<HostRule>> = HashMap::new();
    for row in &rows {
        let host = text(row, "host")?;
        if !is_rule_host(&host) {
            continue;
        }
        let rule = HostRule {
            host,
            enabled: flag(row, "enabled")?,
        };
        let scope = text(row, "scope")?;
        match (text(row, "list_kind")?.as_str(), scope.is_empty()) {
            ("blocked", true) => config.blocked_hosts.push(rule),
            ("exception", true) => config.exception_hosts.push(rule),
            ("allowed", true) => config.allowed_hosts.push(rule),
            (CATEGORY_LIST_KIND, false) => category_hosts.entry(scope).or_default().push(rule),
            _ => {}
        }
    }
    let rows = sqlx::query(
        "SELECT id, name, enabled FROM distractions_custom_categories
         ORDER BY created_at_ms, id",
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(storage("read custom distraction categories"))?;
    for row in &rows {
        let id = text(row, "id")?;
        let Some(hosts) = category_hosts.remove(&id) else {
            continue;
        };
        config.custom_category_stacks.push(CustomCategory {
            id,
            name: text(row, "name")?,
            enabled: flag(row, "enabled")?,
            hosts,
        });
    }
    Ok(())
}

async fn load_apps(
    conn: &mut SqliteConnection,
    config: &mut DistractionsConfig,
) -> Result<(), RulesError> {
    let rows = sqlx::query(
        "SELECT platform, identity, display_name, match_names_json, enabled
         FROM distractions_app_rules ORDER BY created_at_ms, identity",
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(storage("read distraction apps"))?;
    for row in &rows {
        let name = text(row, "display_name")?;
        let enabled = flag(row, "enabled")?;
        match text(row, "platform")?.as_str() {
            DESKTOP_PLATFORM => {
                let stored: Vec<String> =
                    serde_json::from_str(&text(row, "match_names_json")?).unwrap_or_default();
                config.desktop.blocked_apps.push(DesktopAppRule {
                    match_names: normalize_match_names(&name, &stored),
                    name,
                    enabled,
                });
            }
            MOBILE_PLATFORM => config.mobile.blocked_apps.push(MobileAppRule {
                name,
                package_name: text(row, "identity")?,
                enabled,
            }),
            _ => {}
        }
    }
    Ok(())
}

/// The stored entries of one limit, or of every limit when `limit_id` is `None`, in order.
async fn read_entries(
    conn: &mut SqliteConnection,
    limit_id: Option<&str>,
) -> Result<HashMap<String, Vec<LimitEntryRule>>, RulesError> {
    let rows = sqlx::query(
        "SELECT limit_id, id, name, color, website_host, mobile_app_name, mobile_app_package,
                desktop_app_name, desktop_app_match_names_json
         FROM distractions_usage_limit_entries
         WHERE ?1 IS NULL OR limit_id = ?1
         ORDER BY limit_id, ordinal",
    )
    .bind(limit_id)
    .fetch_all(&mut *conn)
    .await
    .map_err(storage("read usage limit entries"))?;
    let mut entries: HashMap<String, Vec<LimitEntryRule>> = HashMap::new();
    for row in &rows {
        let match_names: Vec<String> =
            serde_json::from_str(&text(row, "desktop_app_match_names_json")?).unwrap_or_default();
        entries
            .entry(text(row, "limit_id")?)
            .or_default()
            .push(LimitEntryRule {
                id: text(row, "id")?,
                name: optional_text(row, "name")?,
                color: optional_integer(row, "color")?,
                website_host: optional_text(row, "website_host")?,
                mobile_app_name: optional_text(row, "mobile_app_name")?,
                mobile_app_package: optional_text(row, "mobile_app_package")?,
                desktop_app_name: optional_text(row, "desktop_app_name")?,
                desktop_app_match_names: match_names,
            });
    }
    Ok(entries)
}

/// Whether an assembled entry is one usage accounting can read: a valid distinct id and a source.
fn is_usable_entry(entry: &LimitEntryRule, ids: &mut HashSet<String>) -> bool {
    is_rule_id(&entry.id)
        && (entry.website_host.is_some()
            || entry.mobile_app_name.is_some()
            || entry.mobile_app_package.is_some()
            || entry.desktop_app_name.is_some())
        && entry.desktop_app_match_names.len() <= MAX_MATCH_NAMES
        && ids.insert(entry.id.clone())
}

async fn load_limits(
    conn: &mut SqliteConnection,
    config: &mut DistractionsConfig,
) -> Result<(), RulesError> {
    let mut entries = read_entries(conn, None).await?;
    let rows = sqlx::query(
        "SELECT id, name, enabled, minutes_per_day, minutes_per_week
         FROM distractions_usage_limits ORDER BY created_at_ms, id",
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(storage("read usage limits"))?;
    let mut entry_total = 0usize;
    for row in &rows {
        let id = text(row, "id")?;
        let minutes_per_day = optional_integer(row, "minutes_per_day")?;
        let minutes_per_week = optional_integer(row, "minutes_per_week")?;
        let mut ids = HashSet::new();
        let limit_entries: Vec<LimitEntryRule> = entries
            .remove(&id)
            .unwrap_or_default()
            .into_iter()
            .filter(|entry| is_usable_entry(entry, &mut ids))
            .collect();
        let complete = is_rule_id(&id)
            && !limit_entries.is_empty()
            && (minutes_per_day.is_some() || minutes_per_week.is_some());
        if !complete
            || config.limits.items.len() >= MAX_USAGE_LIMITS
            || entry_total + limit_entries.len() > MAX_LIMIT_ENTRIES
        {
            continue;
        }
        entry_total += limit_entries.len();
        config.limits.items.push(LimitRule {
            id,
            name: text(row, "name")?,
            enabled: flag(row, "enabled")?,
            minutes_per_day,
            minutes_per_week,
            entries: limit_entries,
        });
    }
    Ok(())
}
