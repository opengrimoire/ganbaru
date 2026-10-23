//! Stability: the operations every counted device holds, derived on the coordinator from the
//! acknowledged contexts of the devices' writers.
//!
//! A device acknowledges the context of the newest stored operation with a known context of
//! its newest active writer, with that writer's own entry raised to the operation. A device
//! without an active writer, or whose writer has no operation with a known context,
//! acknowledges nothing. Retired, revoked, and forked writers never acknowledge. A dormant
//! device is not counted, and neither is a device whose link time is unknown: it must be linked
//! again before it holds stability. Nothing here is stored: the coordinator recomputes it every
//! pass.

use ganbaru_sync_contracts::{Seq, VersionVector, WriterId};
use sqlx::{Connection, Row, SqliteConnection};

use crate::error::{SyncResult, corrupt};
use crate::log::{self, decode_context, stored_u64, writer_id};
use crate::{Engine, SpaceContext};

/// Time without any stored operation of a device after which the coordinator stops counting
/// it, so a device that went away cannot hold stability back forever.
pub const DORMANCY_MS: u64 = 90 * 24 * 60 * 60 * 1000;

/// A linked device the coordinator counts unless it is dormant or its link time is unknown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CountedDevice {
    /// Device id its writers' certificates name.
    pub device_id: String,
    /// When the device was linked, or `None` when the link record has no valid time. Dormancy
    /// counts from here while the device has no operation.
    pub linked_at_ms: Option<u64>,
}

/// Whether the coordinator counts a linked device toward stability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceStanding {
    /// The device is counted.
    Counted,
    /// The device stored nothing for longer than [`DORMANCY_MS`] and is not counted until it
    /// returns.
    Dormant,
    /// The link record has no valid time, which is unsupported state: the device is neither
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
    let acknowledged = match newest_active_writer(conn, ctx, &device.device_id).await? {
        Some(writer) => writer_acknowledgement(conn, ctx, writer).await?,
        None => VersionVector::new(),
    };
    Ok(DeviceAcknowledgement {
        device_id: device.device_id.clone(),
        acknowledged,
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
/// operation itself.
async fn writer_acknowledgement(
    conn: &mut SqliteConnection,
    ctx: &SpaceContext,
    writer: WriterId,
) -> SyncResult<VersionVector> {
    let row = sqlx::query(
        "SELECT seq, context FROM sync_ops
         WHERE space_id = ? AND writer_id = ? AND length(context) > 0
         ORDER BY seq DESC LIMIT 1",
    )
    .bind(ctx.space.as_bytes().as_slice())
    .bind(writer.as_bytes().as_slice())
    .fetch_optional(&mut *conn)
    .await?;
    let Some(row) = row else {
        return Ok(VersionVector::new());
    };
    let seq: Seq = stored_u64(row.try_get(0)?, "sequence")?;
    let bytes: Vec<u8> = row.try_get(1)?;
    let mut context =
        decode_context(&bytes)?.ok_or_else(|| corrupt("known context decodes as unknown"))?;
    context.set(writer, seq);
    Ok(context)
}
