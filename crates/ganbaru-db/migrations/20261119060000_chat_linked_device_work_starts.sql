-- Work a linked device creates for this device dispatches without a local acceptance, since a
-- vault links one person's devices. Dispatch jobs carry no acceptance digest, and no work waits
-- for acceptance with an execution target kept aside.
ALTER TABLE chat_assignment_dispatch_jobs DROP COLUMN consented_digest;

DROP TABLE IF EXISTS chat_assignment_input_requested_targets;

DROP TABLE IF EXISTS chat_assignment_requested_targets;
