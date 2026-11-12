-- The review queue and the channel attention aggregate read open assignments by state across
-- every reply thread, newest first, and skip assignments a later one continues. The partial
-- index covers the four states that wait for the person, and the successor index answers the
-- probe for a later assignment naming this one. Indexes are local and never replicate.
CREATE INDEX IF NOT EXISTS idx_chat_work_assignments_attention
ON chat_work_assignments(state, updated_at DESC, id)
WHERE state IN ('waiting_for_answer', 'waiting_for_approval', 'ready_for_review', 'failed');

CREATE INDEX IF NOT EXISTS idx_chat_work_assignments_previous
ON chat_work_assignments(previous_assignment_id)
WHERE previous_assignment_id IS NOT NULL;
