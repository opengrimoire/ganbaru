-- A dispatch job records the acceptance digest of what its post or Start agreed to run. A job
-- without one holds no consent and returns its assignment to Start here.
ALTER TABLE chat_assignment_dispatch_jobs
ADD COLUMN consented_digest TEXT CHECK (
    consented_digest IS NULL OR length(consented_digest) = 64
);
