-- Fork evidence settlement.
--
-- `settled` is 0 for evidence stored since the last engine write, which excludes both branches
-- of the forked writer and selects the authorization chain again, since a fork of an anchor's
-- writer freezes its epoch. Device-local engine state that never replicates.

ALTER TABLE sync_fork_evidence ADD COLUMN settled INTEGER NOT NULL DEFAULT 0
    CHECK (settled IN (0, 1));
