//! Trust scope durations a person grants a contact.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// How long a contact may act unsolicited in one trust scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustKind {
    /// The contact may not act without asking.
    NotAllowed,
    /// The first use consumes the permission.
    Once,
    /// The permission lasts seven days from the grant.
    SevenDays,
    /// The permission lasts thirty days from the grant.
    ThirtyDays,
    /// The permission lasts until the person revokes it.
    UntilRevoked,
}

impl TrustKind {
    /// Persisted and wire name of the kind.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotAllowed => "not_allowed",
            Self::Once => "once",
            Self::SevenDays => "seven_days",
            Self::ThirtyDays => "thirty_days",
            Self::UntilRevoked => "until_revoked",
        }
    }

    /// Parses a persisted or wire name.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "not_allowed" => Some(Self::NotAllowed),
            "once" => Some(Self::Once),
            "seven_days" => Some(Self::SevenDays),
            "thirty_days" => Some(Self::ThirtyDays),
            "until_revoked" => Some(Self::UntilRevoked),
            _ => None,
        }
    }

    /// Expiry instant for a grant made at `granted_at`, or `None` when the kind has no fixed end.
    pub fn expires_at(self, granted_at: DateTime<Utc>) -> Option<DateTime<Utc>> {
        match self {
            Self::SevenDays => Some(granted_at + Duration::days(7)),
            Self::ThirtyDays => Some(granted_at + Duration::days(30)),
            Self::NotAllowed | Self::Once | Self::UntilRevoked => None,
        }
    }

    /// Whether the permission is active at `now` given its stored expiry.
    pub fn is_active(self, expires_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
        match self {
            Self::NotAllowed => false,
            Self::Once | Self::UntilRevoked => true,
            Self::SevenDays | Self::ThirtyDays => expires_at.is_some_and(|until| until > now),
        }
    }
}
