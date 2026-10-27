//! The device-local peer book: where the other members of a vault last listened.
//!
//! The book only holds address hints. Which devices are members, and which certificate each must
//! present, always comes from the selected authorization log, so the book is rebuilt against the
//! log on every pass and a stale or tampered entry can at worst make a dial fail.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use ganbaru_atomic_files::{
    FileAccess, FinalSymlink, create_private_directories, read_regular_file_capped,
    write_file_atomically,
};
use ganbaru_handoff::protocol::{
    MAX_COORDINATOR_ENDPOINTS, MAX_MEMBER_ENDPOINTS, MemberEndpoint, validate_coordinator_endpoint,
    validate_identifier,
};
use serde::{Deserialize, Serialize};

const PEER_BOOK_SCHEMA_VERSION: u32 = 1;

/// Most endpoints the book keeps across all members, the most a sync state reports.
pub const MAX_PEER_ENDPOINTS: usize = MAX_MEMBER_ENDPOINTS;

/// Most endpoints the book keeps for one member, the most one listener reports.
pub const MAX_ENDPOINTS_PER_PEER: usize = MAX_COORDINATOR_ENDPOINTS;

/// Largest peer book file this installation writes or reads.
pub const MAX_PEER_BOOK_BYTES: usize = 16 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PeerBookFile {
    schema_version: u32,
    endpoints: Vec<PeerBookEntry>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PeerBookEntry {
    device_id: String,
    endpoint: String,
    observed_ms: u64,
}

/// The member endpoints of one vault, keyed by device and endpoint with the newest observation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PeerEndpoints {
    entries: BTreeMap<(String, String), u64>,
}

impl PeerEndpoints {
    /// Records that `device_id` listened on `endpoints` at `observed_ms`. Endpoints that fail
    /// the dial policy are dropped; an older observation never replaces a newer one. Returns
    /// whether the book changed.
    pub fn observe(&mut self, device_id: &str, endpoints: &[String], observed_ms: u64) -> bool {
        if validate_identifier("peer device id", device_id).is_err() {
            return false;
        }
        let mut changed = false;
        for endpoint in endpoints.iter().take(MAX_ENDPOINTS_PER_PEER) {
            if validate_coordinator_endpoint(endpoint).is_err() {
                continue;
            }
            let slot = self
                .entries
                .entry((device_id.to_string(), endpoint.clone()))
                .or_insert(0);
            if observed_ms > *slot {
                *slot = observed_ms;
                changed = true;
            }
        }
        if changed {
            self.trim();
        }
        changed
    }

    /// Records every endpoint a sync state reported. Returns whether the book changed.
    pub fn observe_reported(&mut self, members: &[MemberEndpoint]) -> bool {
        let mut changed = false;
        for member in members {
            changed |= self.observe(
                &member.device_id,
                std::slice::from_ref(&member.endpoint),
                member.observed_ms,
            );
        }
        changed
    }

    /// Keeps only the endpoints of `members`, the devices the selected log enrolled and did not
    /// remove. Returns whether the book changed.
    pub fn retain_members(&mut self, members: &BTreeSet<String>) -> bool {
        let before = self.entries.len();
        self.entries
            .retain(|(device_id, _), _| members.contains(device_id));
        self.entries.len() != before
    }

    /// The endpoints of `device_id`, newest first.
    pub fn endpoints(&self, device_id: &str) -> Vec<String> {
        let mut found: Vec<(&String, u64)> = self
            .entries
            .iter()
            .filter(|((device, _), _)| device == device_id)
            .map(|((_, endpoint), observed)| (endpoint, *observed))
            .collect();
        found.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(right.0)));
        found
            .into_iter()
            .map(|(endpoint, _)| endpoint.clone())
            .collect()
    }

    /// The members the book holds endpoints for.
    pub fn devices(&self) -> BTreeSet<String> {
        self.entries
            .keys()
            .map(|(device_id, _)| device_id.clone())
            .collect()
    }

    /// The endpoints a hub reports in its sync state: newest first, at most
    /// [`MAX_PEER_ENDPOINTS`].
    pub fn report(&self) -> Vec<MemberEndpoint> {
        self.newest_first()
            .into_iter()
            .take(MAX_PEER_ENDPOINTS)
            .map(|((device_id, endpoint), observed_ms)| MemberEndpoint {
                device_id: device_id.clone(),
                endpoint: endpoint.clone(),
                observed_ms,
            })
            .collect()
    }

    /// Whether the book holds no endpoint.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn newest_first(&self) -> Vec<(&(String, String), u64)> {
        let mut all: Vec<(&(String, String), u64)> = self
            .entries
            .iter()
            .map(|(key, observed)| (key, *observed))
            .collect();
        all.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(right.0)));
        all
    }

    /// Drops the oldest endpoints beyond the per member and total bounds.
    fn trim(&mut self) {
        let mut per_device: BTreeMap<String, usize> = BTreeMap::new();
        let mut keep = BTreeSet::new();
        for (key, _) in self.newest_first() {
            if keep.len() >= MAX_PEER_ENDPOINTS {
                break;
            }
            let count = per_device.entry(key.0.clone()).or_default();
            if *count < MAX_ENDPOINTS_PER_PEER {
                *count += 1;
                keep.insert(key.clone());
            }
        }
        self.entries.retain(|key, _| keep.contains(key));
    }
}

/// The peer book file of one vault with the endpoints it holds.
#[derive(Debug, Clone)]
pub struct PeerBook {
    path: PathBuf,
    endpoints: PeerEndpoints,
}

impl PeerBook {
    /// Reads the peer book at `path`, empty when the file does not exist. Refuses a file that is
    /// too large, does not parse, has another schema version, or holds too many or malformed
    /// entries.
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = match read_regular_file_capped(
            path,
            u64::try_from(MAX_PEER_BOOK_BYTES).unwrap_or(u64::MAX),
            FinalSymlink::Refuse,
        ) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self {
                    path: path.to_path_buf(),
                    endpoints: PeerEndpoints::default(),
                });
            }
            Err(error) => return Err(format!("read sync peer book: {error}")),
        };
        let file: PeerBookFile = serde_json::from_slice(&bytes)
            .map_err(|error| format!("parse sync peer book: {error}"))?;
        if file.schema_version != PEER_BOOK_SCHEMA_VERSION {
            return Err("sync peer book has an unsupported schema version".to_string());
        }
        if file.endpoints.len() > MAX_PEER_ENDPOINTS {
            return Err("sync peer book holds too many endpoints".to_string());
        }
        let mut entries = BTreeMap::new();
        let mut per_device: BTreeMap<&str, usize> = BTreeMap::new();
        for entry in &file.endpoints {
            validate_identifier("peer device id", &entry.device_id)
                .and_then(|()| validate_coordinator_endpoint(&entry.endpoint).map(|_| ()))
                .map_err(|error| format!("sync peer book holds a malformed entry: {error}"))?;
            let count = per_device.entry(entry.device_id.as_str()).or_default();
            *count += 1;
            if *count > MAX_ENDPOINTS_PER_PEER {
                return Err("sync peer book holds too many endpoints for one device".to_string());
            }
            if entries
                .insert(
                    (entry.device_id.clone(), entry.endpoint.clone()),
                    entry.observed_ms,
                )
                .is_some()
            {
                return Err("sync peer book repeats an endpoint".to_string());
            }
        }
        Ok(Self {
            path: path.to_path_buf(),
            endpoints: PeerEndpoints { entries },
        })
    }

    /// The endpoints the book holds.
    pub fn endpoints(&self) -> &PeerEndpoints {
        &self.endpoints
    }

    /// Replaces the endpoints and writes the file when they changed.
    pub fn store(&mut self, endpoints: PeerEndpoints) -> Result<(), String> {
        if endpoints == self.endpoints {
            return Ok(());
        }
        let file = PeerBookFile {
            schema_version: PEER_BOOK_SCHEMA_VERSION,
            endpoints: endpoints
                .entries
                .iter()
                .map(|((device_id, endpoint), observed_ms)| PeerBookEntry {
                    device_id: device_id.clone(),
                    endpoint: endpoint.clone(),
                    observed_ms: *observed_ms,
                })
                .collect(),
        };
        let bytes = serde_json::to_vec_pretty(&file)
            .map_err(|error| format!("encode sync peer book: {error}"))?;
        if let Some(parent) = self.path.parent() {
            create_private_directories(parent)
                .map_err(|error| format!("create sync peer book directory: {error}"))?;
        }
        write_file_atomically(&self.path, &bytes, FileAccess::OwnerOnly)
            .map_err(|error| format!("write sync peer book: {error}"))?;
        self.endpoints = endpoints;
        Ok(())
    }
}
