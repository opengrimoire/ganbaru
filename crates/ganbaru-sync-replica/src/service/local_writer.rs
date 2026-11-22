//! The local writer of a session: sealing captures in bounded rounds, keeping a writer ready
//! with its space keys, successors and rotation, sealing and applying with rotation when the
//! space stops accepting the writer, and recovery from forks of this installation's chains.

use super::host::SyncHost;
use super::{
    Applied, KeyRelease, MAX_APPLY_ROUNDS, MAX_SEAL_ROUNDS, Session, acquire, blocking, table_names,
};
use crate::client::ForkPoint;
use crate::keys::SpaceKeys;
use crate::now_ms;
use crate::outbox::Outbox;
use crate::writer::{
    ActiveWriter, SuccessorPlan, WriterCheck, check_writer, create_writer,
    ensure_device_key as ensure_stored_device_key, read_stored_writer, remove_retired_keys,
};
use ganbaru_sync::{ChangeNotice, SealReport, SyncError, hold, local};
use std::collections::BTreeSet;
use std::sync::Arc;

impl<H: SyncHost> Session<'_, H> {
    /// Seals pending captures in bounded transactions until none is ready. A seal that leaves
    /// captures in place is not repeated until the number of captures changes or a delete
    /// hold expires.
    pub(super) async fn seal_pending(
        &self,
        writer: &mut Option<ActiveWriter>,
        stalled: &mut Option<u64>,
    ) -> Result<(), String> {
        let pending = self.pending_captures().await?;
        let now = now_ms()?;
        let hold_expired = self.next_hold_expiry().await?.is_some_and(|at| at <= now);
        if pending == 0 || (*stalled == Some(pending) && !hold_expired) {
            if pending == 0 {
                *stalled = None;
            }
            return Ok(());
        }
        if !self.ensure_writer(writer).await? {
            return Ok(());
        }
        self.seal_batches(writer).await?;
        let remaining = self.pending_captures().await?;
        *stalled = (remaining > 0).then_some(remaining);
        Ok(())
    }

    /// Seals pending captures in bounded transactions until no more is ready.
    async fn seal_batches(&self, writer: &mut Option<ActiveWriter>) -> Result<(), String> {
        let mut invalid = 0;
        for _ in 0..MAX_SEAL_ROUNDS {
            let report = self.seal(writer).await?;
            invalid = report.invalid_rows.len();
            if !report.more {
                break;
            }
        }
        if invalid > 0 {
            eprintln!("sync: {invalid} local rows cannot be sealed");
        }
        Ok(())
    }

    /// When the earliest delete hold expires, so a pass can seal it then.
    pub(super) async fn next_hold_expiry(&self) -> Result<Option<u64>, String> {
        let mut conn = acquire(&self.pool).await?;
        hold::next_hold_expiry(&mut conn)
            .await
            .map_err(|error| format!("read sync delete holds: {error}"))
    }

    pub(super) async fn pending_captures(&self) -> Result<u64, String> {
        let mut conn = acquire(&self.pool).await?;
        local::pending_captures(&mut conn)
            .await
            .map_err(|error| format!("read pending sync changes: {error}"))
    }

    /// Makes sure a writer is ready, checking the recorded one against the database and the
    /// device key and creating a successor when needed. Returns whether a writer is ready.
    pub(super) async fn ensure_writer(
        &self,
        writer: &mut Option<ActiveWriter>,
    ) -> Result<bool, String> {
        if writer.is_some() {
            self.attach_space_keys(writer).await?;
            return Ok(true);
        }
        let files = self.files.clone();
        let keys = self.keys.clone();
        let (device_key, stored) = blocking(move || {
            let device = ensure_stored_device_key(keys.as_ref(), &files.vault_id)?;
            let stored = read_stored_writer(&files, keys.as_ref())?;
            Ok((device.public_key(), stored))
        })
        .await?;
        let check = {
            let mut conn = acquire(&self.pool).await?;
            check_writer(
                &self.engine,
                &mut conn,
                &self.ctx,
                &self.files,
                &device_key,
                stored,
            )
            .await?
        };
        *writer = match check {
            WriterCheck::Ready(ready) => {
                let files = self.files.clone();
                let keys = self.keys.clone();
                Some(
                    blocking(move || {
                        let mut ready = *ready;
                        remove_retired_keys(&mut ready, &files, keys.as_ref())?;
                        Ok(ready)
                    })
                    .await?,
                )
            }
            WriterCheck::Successor(plan) => self.create_successor(plan).await?,
        };
        if let Some(active) = writer.as_mut() {
            let mut conn = acquire(&self.pool).await?;
            let charter = self
                .engine
                .authorization(&mut conn, &self.ctx)
                .await
                .map_err(|error| format!("read sync authorization: {error}"))?
                .map(|view| view.charter);
            active.settle_sequenced(charter)?;
        }
        self.attach_space_keys(writer).await?;
        Ok(writer.is_some())
    }

    /// Gives a writer without space keys the installation's device encryption key, created once,
    /// and the epoch keys it opens from the selected log. Plaintext epoch keys stay in memory.
    async fn attach_space_keys(&self, writer: &mut Option<ActiveWriter>) -> Result<(), String> {
        let Some(active) = writer.as_mut() else {
            return Ok(());
        };
        if active.space_keys().is_some() {
            return Ok(());
        }
        let files = self.files.clone();
        let keys = self.keys.clone();
        let space_keys =
            Arc::new(blocking(move || SpaceKeys::load(keys.as_ref(), &files.vault_id)).await?);
        let mut conn = acquire(&self.pool).await?;
        space_keys
            .open_epochs(&self.engine, &mut conn, &self.ctx)
            .await?;
        active.attach_keys(space_keys);
        Ok(())
    }

    /// Publishes this installation's device encryption key while the selected log lacks it and,
    /// on an anchor that may sequence, seals the key epoch a removal waits for, then reopens the
    /// epoch keys. A member queues its device key in the outbox for the anchor. Returns whether
    /// the writer sealed an entry.
    pub(super) async fn maintain_space_keys(
        &self,
        writer: &mut Option<ActiveWriter>,
        may_sequence: bool,
    ) -> Result<bool, String> {
        use crate::authority::{DeviceKeyPublication, complete_rotation, publish_device_key};
        let Some(active) = writer.as_mut() else {
            return Ok(false);
        };
        let Some(space_keys) = active.space_keys().cloned() else {
            return Ok(false);
        };
        let mut conn = acquire(&self.pool).await?;
        let Some(view) = self
            .engine
            .authorization(&mut conn, &self.ctx)
            .await
            .map_err(|error| format!("read sync authorization: {error}"))?
        else {
            return Ok(false);
        };
        let anchors = view.anchor_device_id == active.device_id();
        if anchors && !may_sequence {
            return Ok(false);
        }
        let path = self.files.outbox_path();
        let mut outbox = blocking(move || Outbox::load(&path)).await?;
        let mut sealed = false;
        match publish_device_key(
            &self.engine,
            &mut conn,
            &self.ctx,
            &mut active.local(),
            space_keys.as_ref(),
            &mut outbox,
            now_ms()?,
        )
        .await?
        {
            DeviceKeyPublication::Sequenced(_) => sealed = true,
            DeviceKeyPublication::Rejected(rejection) => {
                eprintln!("sync: publish the device encryption key: {rejection}");
            }
            DeviceKeyPublication::Current | DeviceKeyPublication::Queued => {}
        }
        if anchors {
            match complete_rotation(
                &self.engine,
                &mut conn,
                &self.ctx,
                &mut active.local(),
                now_ms()?,
            )
            .await?
            {
                Ok(Some(_)) => sealed = true,
                Ok(None) => {}
                Err(rejection) => eprintln!("sync: seal the waiting key epoch: {rejection}"),
            }
            if self
                .start_key_epochs(&mut conn, &mut active.local(), &view)
                .await?
            {
                sealed = true;
            }
        }
        if sealed {
            // The record must name what sequencing sealed, or a restored database behind
            // those entries would still match the writer.
            let head = self
                .engine
                .stored_vector(&mut conn, &self.ctx)
                .await
                .map_err(|error| format!("read sync stored vector: {error}"))?
                .get(&active.id());
            active.committed(head)?;
        }
        space_keys
            .open_epochs(&self.engine, &mut conn, &self.ctx)
            .await?;
        Ok(sealed)
    }

    /// On the anchor of a space without a key epoch, sequences the recovery key entry of the kit
    /// the vault database keeps, signed by this installation's admin key, which opens the first
    /// key epoch. `view` is the selected log read before this pass published the device key, so
    /// the entry is attempted only where the kit can start the epochs. Returns whether it sealed.
    async fn start_key_epochs(
        &self,
        conn: &mut sqlx::SqliteConnection,
        writer: &mut ganbaru_sync::LocalWriter<'_>,
        view: &ganbaru_sync::AuthorizationView,
    ) -> Result<bool, String> {
        use crate::authority::{
            read_admin_key as read_stored_admin_key, start_key_epochs, starts_key_epochs,
        };
        let Some(kit) = local::recovery_kit_keys(conn)
            .await
            .map_err(|error| format!("read the sync recovery kit: {error}"))?
        else {
            return Ok(false);
        };
        if !starts_key_epochs(view, &writer.certificate.certificate.device_id, &kit) {
            return Ok(false);
        }
        let keys = self.keys.clone();
        let vault_id = self.files.vault_id.clone();
        let Some(admin) = blocking(move || read_stored_admin_key(keys.as_ref(), &vault_id)).await?
        else {
            return Ok(false);
        };
        match start_key_epochs(
            &self.engine,
            conn,
            &self.ctx,
            &admin,
            writer,
            &kit,
            now_ms()?,
        )
        .await?
        {
            Ok(sealed) => Ok(sealed.is_some()),
            Err(rejection) => {
                eprintln!("sync: start the key epochs: {rejection}");
                Ok(false)
            }
        }
    }

    /// Creates a successor writer certified by this installation's device key. A device without
    /// the person key also asks the coordinator for it, which Contacts signing needs.
    async fn create_successor(&self, plan: SuccessorPlan) -> Result<Option<ActiveWriter>, String> {
        if self.key_release == KeyRelease::Request
            && self.host.person_key(&self.ctx.person_key).await?.is_none()
        {
            self.host.request_person_key();
        }
        let device_id = self.host.device_id()?;
        let files = self.files.clone();
        let keys = self.keys.clone();
        let now = now_ms()?;
        blocking(move || {
            let device = ensure_stored_device_key(keys.as_ref(), &files.vault_id)?;
            create_writer(&files, keys.as_ref(), plan, &device, &device_id, now).map(Some)
        })
        .await
    }

    /// Replaces a writer the space stopped accepting.
    async fn rotate(&self, writer: &mut Option<ActiveWriter>) -> Result<bool, String> {
        let plan = writer
            .as_ref()
            .map(SuccessorPlan::after)
            .unwrap_or_default();
        *writer = self.create_successor(plan).await?;
        self.attach_space_keys(writer).await?;
        Ok(writer.is_some())
    }

    /// Replaces the writer of an anchor whose database was restored behind its log, keeping the
    /// high-water marked as restored since `restored_at_ms` unless it already is.
    #[cfg(desktop)]
    pub(super) async fn rotate_restored(
        &self,
        writer: &mut Option<ActiveWriter>,
        restored_at_ms: u64,
    ) -> Result<bool, String> {
        let plan = writer
            .as_ref()
            .map(|active| SuccessorPlan::after_restore(active, restored_at_ms))
            .unwrap_or_default();
        *writer = self.create_successor(plan).await?;
        self.attach_space_keys(writer).await?;
        Ok(writer.is_some())
    }

    /// Seals with the writer, rotating once when the space no longer accepts it.
    async fn seal(&self, writer: &mut Option<ActiveWriter>) -> Result<SealReport, String> {
        for attempt in 0..2 {
            let Some(active) = writer.as_mut() else {
                break;
            };
            let result = {
                let mut conn = acquire(&self.pool).await?;
                self.engine
                    .seal(&mut conn, &self.ctx, &mut active.local(), now_ms()?)
                    .await
            };
            match result {
                Ok(report) => {
                    if let Some(seq) = report.last_seq {
                        active.committed(seq)?;
                    }
                    self.report_seal(&report).await;
                    return Ok(report);
                }
                Err(SyncError::WriterInactive(_)) if attempt == 0 => {
                    if !self.rotate(writer).await? {
                        break;
                    }
                }
                // Captures stay pending until the exchange brings this device's enrollment.
                Err(SyncError::NotEnrolled) => break,
                Err(error) => return Err(format!("seal sync changes: {error}")),
            }
        }
        Ok(SealReport::default())
    }

    /// Seals the writer's genesis on its first pass, and an acknowledgement or keepalive when
    /// one is due, rotating once when the space no longer accepts the writer. Without a writer,
    /// or before this device is enrolled, it seals nothing.
    pub(super) async fn acknowledge(
        &self,
        writer: &mut Option<ActiveWriter>,
    ) -> Result<SealReport, String> {
        for attempt in 0..2 {
            let Some(active) = writer.as_mut() else {
                break;
            };
            let result = {
                let mut conn = acquire(&self.pool).await?;
                self.engine
                    .acknowledge(&mut conn, &self.ctx, &mut active.local(), now_ms()?)
                    .await
            };
            match result {
                Ok(report) => {
                    if let Some(seq) = report.last_seq {
                        active.committed(seq)?;
                    }
                    return Ok(report);
                }
                Err(SyncError::WriterInactive(_)) if attempt == 0 => {
                    if !self.rotate(writer).await? {
                        break;
                    }
                }
                Err(SyncError::NotEnrolled) => break,
                Err(error) => return Err(format!("seal sync acknowledgement: {error}")),
            }
        }
        Ok(SealReport::default())
    }

    /// Applies ready operations, re-sealing and rotating the local writer when the space
    /// requires it. An apply that stops on local captures it cannot seal in one transaction is
    /// retried after sealing them in batches, while those seals drain captures.
    pub(super) async fn apply_ready(
        &self,
        writer: &mut Option<ActiveWriter>,
    ) -> Result<Applied, String> {
        let mut tables = BTreeSet::new();
        let mut notices = BTreeSet::new();
        let done =
            |tables: BTreeSet<String>, notices: BTreeSet<ChangeNotice>, blocked_at| Applied {
                tables: tables.into_iter().collect(),
                notices: notices.into_iter().collect(),
                blocked_at,
            };
        for _ in 0..MAX_APPLY_ROUNDS {
            let report = {
                let mut conn = acquire(&self.pool).await?;
                let mut local = writer.as_mut().map(ActiveWriter::local);
                self.engine
                    .apply(&mut conn, &self.ctx, local.as_mut(), now_ms()?)
                    .await
                    .map_err(|error| format!("apply sync operations: {error}"))?
            };
            tables.extend(table_names(&report.changed_tables));
            notices.extend(report.notices.iter().cloned());
            if let Some(seal) = report.seal.as_ref() {
                tables.extend(table_names(&seal.changed_tables));
                notices.extend(seal.notices.iter().cloned());
            }
            if let (Some(active), Some(seq)) = (
                writer.as_mut(),
                report.seal.as_ref().and_then(|seal| seal.last_seq),
            ) {
                active.committed(seq)?;
            }
            if !self.reseal_own_forks(writer).await? {
                return Ok(done(tables, notices, None));
            }
            if !self.retire_compromised(writer).await? {
                return Ok(done(tables, notices, None));
            }
            if report.local_writer_frozen {
                if !self.rotate(writer).await? {
                    return Ok(done(tables, notices, None));
                }
            } else if report.pending_captures {
                let before = self.pending_captures().await?;
                self.seal_batches(writer).await?;
                let after = self.pending_captures().await?;
                if after >= before {
                    return Ok(done(tables, notices, Some(after)));
                }
            } else if !report.more {
                return Ok(done(tables, notices, None));
            }
        }
        Err("sync apply did not finish".to_string())
    }

    /// Re-seals every chain of this device that stored fork evidence proves forked while this
    /// installation still holds its copy past the shared prefix. Evidence reaches a hub, the
    /// anchor included, without an exchange of its own, so each apply checks. Returns false
    /// while the person key is unavailable.
    async fn reseal_own_forks(&self, writer: &mut Option<ActiveWriter>) -> Result<bool, String> {
        let own = {
            let mut conn = acquire(&self.pool).await?;
            self.engine
                .own_forks(&mut conn, &self.ctx)
                .await
                .map_err(|error| format!("read fork evidence: {error}"))?
        };
        for (forked, keep_through) in own {
            let fork = ForkPoint {
                writer: forked,
                keep_through,
            };
            if !self.recover_fork(writer, fork).await? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Rotates the active writer once the record store holds a twin of it, provable or not, or
    /// the selected chain removed its device, see [`ganbaru_sync::WriterRetirement`]. The successor is
    /// certified by the unchanged device key and names the retired writer as predecessor.
    /// Returns false while the person key is unavailable.
    async fn retire_compromised(&self, writer: &mut Option<ActiveWriter>) -> Result<bool, String> {
        let Some(current) = writer.as_ref().map(ActiveWriter::id) else {
            return Ok(true);
        };
        let retirement = {
            let mut conn = acquire(&self.pool).await?;
            self.engine
                .writer_retirement(&mut conn, &self.ctx, current)
                .await
                .map_err(|error| format!("check sync writer retirement: {error}"))?
        };
        if retirement.is_none() {
            return Ok(true);
        }
        self.rotate(writer).await
    }

    /// Recovers from a fork of one of this installation's chains: the hub holds other
    /// operations at the same sequences. The local copy above the fork point moves to a fresh
    /// writer, so both histories survive. Returns false while the person key is unavailable. A
    /// re-seal the old chain refuses fails before the rotation, so it consumes no writer.
    pub(super) async fn recover_fork(
        &self,
        writer: &mut Option<ActiveWriter>,
        fork: ForkPoint,
    ) -> Result<bool, String> {
        {
            let mut conn = acquire(&self.pool).await?;
            self.engine
                .check_reseal_fork(&mut conn, &self.ctx, fork.writer, fork.keep_through)
                .await
                .map_err(|error| format!("re-seal forked sync operations: {error}"))?;
        }
        let current = writer.as_ref().map(ActiveWriter::id);
        if (current == Some(fork.writer) || current.is_none()) && !self.rotate(writer).await? {
            return Ok(false);
        }
        let Some(active) = writer.as_mut() else {
            return Ok(false);
        };
        let resealed = {
            let mut conn = acquire(&self.pool).await?;
            self.engine
                .reseal_fork(
                    &mut conn,
                    &self.ctx,
                    fork.writer,
                    fork.keep_through,
                    &mut active.local(),
                    now_ms()?,
                )
                .await
                .map_err(|error| format!("re-seal forked sync operations: {error}"))?
        };
        if let Some(seq) = resealed.seal.last_seq {
            active.committed(seq)?;
        }
        self.report_seal(&resealed.seal).await;
        Ok(true)
    }
}
