//! Stability: the operations every counted device holds, derived on the coordinator from the
//! acknowledged contexts of the devices' writers.
//!
//! A device acknowledges the context of the newest stored operation with a known context of
//! its newest active writer, with that writer's own entry raised to the operation. A device
//! without an active writer, or whose writer has no operation with a known context,
//! acknowledges nothing. Retired, removed, and forked writers never acknowledge. The counted set
//! is the devices enrolled and not removed in the selected authorization log. A dormant device
//! is not counted, and neither is a device whose enrollment time is unknown: it must be linked
//! again before it holds stability. The anchor counts like any member and is never dormant.
//! Nothing here is stored: the coordinator recomputes it every pass.

use ganbaru_sync_contracts::{Seq, VersionVector, WriterId};
use sqlx::{Connection, Row, SqliteConnection};

use crate::authorization::{load_charter, select};
use crate::error::{SyncResult, corrupt};
use crate::log::{self, decode_context, stored_u64, writer_id};
use crate::{Engine, SpaceContext};

/// Time without any stored operation of a device after which the coordinator stops counting
/// it, so a device that went away cannot hold stability back forever.
pub const DORMANCY_MS: u64 = 90 * 24 * 60 * 60 * 1000;

/// An enrolled device the coordinator counts unless it is dormant or its enrollment time is
/// unknown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CountedDevice {
    /// Device id its writers' certificates name.
    pub device_id: String,
    /// When the device was enrolled, or `None` when that time is unknown. Dormancy counts from
    /// here while the device has no operation.
    pub linked_at_ms: Option<u64>,
    /// Whether the device is the anchor of the selected chain, which is never dormant.
    pub anchor: bool,
}

/// Whether the coordinator counts a linked device toward stability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceStanding {
    /// The device is counted.
    Counted,
    /// The device stored nothing for longer than [`DORMANCY_MS`] and is not counted until it
    /// returns.
    Dormant,
    /// The enrollment time is unknown, which is unsupported state: the device is neither
    /// counted nor dormant until it is linked again.
    RelinkRequired,
}

/// What one counted device acknowledges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceAcknowledgement {
    /// Device id.
    pub device_id: String,
    /// Operations the device acknowledged holding.
    pub acknowledged: VersionVector,
    /// When the operation behind `acknowledged` was stored here, or `None` when the device
    /// acknowledges nothing.
    pub acknowledged_at_ms: Option<u64>,
    /// When the newest operation of any of the device's writers was stored here.
    pub last_stored_ms: Option<u64>,
    /// Whether the device is counted.
    pub standing: DeviceStanding,
}

/// Stability of the space at one moment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stability {
    /// Operations every counted device and the coordinator hold.
    pub stable: VersionVector,
    /// Per-device acknowledgements, in the order given.
    pub devices: Vec<DeviceAcknowledgement>,
}

impl Engine {
    /// The counted set seen from `local_device_id`: every device enrolled and not removed in
    /// the selected chain besides the local one, by device id, each with the time this replica
    /// stored the entry that enrolled it. The anchor enrolled by the charter takes the charter's
    /// creation time. Empty without a charter.
    pub async fn counted_devices(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        local_device_id: &str,
    ) -> SyncResult<Vec<CountedDevice>> {
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        let Some(selection) = select(&mut tx, ctx.space).await? else {
            return Ok(Vec::new());
        };
        let charter_ms = load_charter(&mut tx, ctx.space)
            .await?
            .and_then(|charter| u64::try_from(charter.charter.created_at_ms).ok());
        let rows = sqlx::query(
            "SELECT d.device_id, d.enrolled_revision, o.stored_at_ms
             FROM sync_devices d
             LEFT JOIN sync_auth_selected e
                ON e.space_id = d.space_id AND e.revision = d.enrolled_revision
             LEFT JOIN sync_auth_envelopes o
                ON o.space_id = e.space_id AND o.role = 'record'
                AND o.envelope_hash = e.envelope_hash
             WHERE d.space_id = ? AND d.removed_revision IS NULL AND d.device_id <> ?
             ORDER BY d.device_id",
        )
        .bind(ctx.space.as_bytes().as_slice())
        .bind(local_device_id)
        .fetch_all(&mut *tx)
        .await?;
        let anchor_device_id = selection.state.anchor_device_id();
        rows.into_iter()
            .map(|row| {
                let device_id: String = row.try_get(0)?;
                let revision = stored_u64(row.try_get(1)?, "enrollment revision")?;
                let stored: Option<i64> = row.try_get(2)?;
                let linked_at_ms = match stored {
                    Some(at) => Some(stored_u64(at, "operation storage time")?),
                    None if revision == 0 => charter_ms,
                    None => None,
                };
                Ok(CountedDevice {
                    anchor: device_id == anchor_device_id,
                    device_id,
                    linked_at_ms,
                })
            })
            .collect()
    }

    /// Computes stability on the coordinator: the meet of its applied vector and what every
    /// counted device acknowledged.
    pub async fn stability(
        &self,
        conn: &mut SqliteConnection,
        ctx: &SpaceContext,
        devices: &[CountedDevice],
        now_ms: u64,
    ) -> SyncResult<Stability> {
        let mut tx = conn.begin().await?;
        log::ensure_space(&mut tx, ctx.space).await?;
        let mut stable = log::applied_vector(&mut tx, ctx.space).await?;
        let mut acknowledgements = Vec::with_capacity(devices.len());
        for device in devices {
            let acknowledgement = device_acknowledgement(&mut tx, ctx, device, now_ms).await?;
            if acknowledgement.standing == DeviceStanding::Counted {
                stable.meet(&acknowledgement.acknowledged);
            }
            acknowledgements.push(acknowledgement);
        }
        Ok(Stability {
            stable,
            devices: acknowledgements,
        })
    }
}

async fn device_acknowledgement(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    device: &CountedDevice,
    now_ms: u64,
) -> SyncResult<DeviceAcknowledgement> {
    let last_stored: Option<i64> = sqlx::query_scalar(
        "SELECT max(o.stored_at_ms) FROM sync_ops o
         JOIN sync_writers w ON w.space_id = o.space_id AND w.writer_id = o.writer_id
         WHERE o.space_id = ? AND w.device_id = ?",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(device.device_id.as_str())
    .fetch_one(&mut *conn)
    .await?;
    let last_stored_ms = last_stored
        .map(|at| stored_u64(at, "operation storage time"))
        .transpose()?;
    let standing = match device.linked_at_ms {
        _ if device.anchor => DeviceStanding::Counted,
        None => DeviceStanding::RelinkRequired,
        Some(linked_at_ms) => {
            let active_since = last_stored_ms.unwrap_or(linked_at_ms);
            if now_ms.saturating_sub(active_since) > DORMANCY_MS {
                DeviceStanding::Dormant
            } else {
                DeviceStanding::Counted
            }
        }
    };
    let (acknowledged, acknowledged_at_ms) =
        match newest_active_writer(conn, ctx, &device.device_id).await? {
            Some(writer) => writer_acknowledgement(conn, ctx, writer).await?,
            None => (VersionVector::new(), None),
        };
    Ok(DeviceAcknowledgement {
        device_id: device.device_id.clone(),
        acknowledged,
        acknowledged_at_ms,
        last_stored_ms,
        standing,
    })
}

async fn newest_active_writer(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    device_id: &str,
) -> SyncResult<Option<WriterId>> {
    let id: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT writer_id FROM sync_writers
         WHERE space_id = ? AND device_id = ? AND state = 'active'
         ORDER BY created_at_ms DESC, writer_id DESC LIMIT 1",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(device_id)
    .fetch_optional(&mut *conn)
    .await?;
    id.map(|bytes| writer_id(&bytes)).transpose()
}

/// Context of the newest stored operation of `writer` with a known context, including the
/// operation itself, and when that operation was stored here.
async fn writer_acknowledgement(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    writer: WriterId,
) -> SyncResult<(VersionVector, Option<u64>)> {
    let row = sqlx::query(
        "SELECT seq, context, stored_at_ms FROM sync_ops
         WHERE space_id = ? AND writer_id = ? AND length(context) > 0
         ORDER BY seq DESC LIMIT 1",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .fetch_optional(&mut *conn)
    .await?;
    let Some(row) = row else {
        return Ok((VersionVector::new(), None));
    };
    let seq: Seq = stored_u64(row.try_get(0)?, "sequence")?;
    let bytes: Vec<u8> = row.try_get(1)?;
    let stored_at_ms = stored_u64(row.try_get(2)?, "operation storage time")?;
    let mut context =
        decode_context(&bytes)?.ok_or_else(|| corrupt("known context decodes as unknown"))?;
    context.set(writer, seq);
    Ok((context, Some(stored_at_ms)))
}
