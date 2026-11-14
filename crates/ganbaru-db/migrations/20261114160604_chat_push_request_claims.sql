-- Device-local claims that let one push of a push request run at a time. A confirmation or a
-- standing push claims the request before any Git work by inserting or taking over this row,
-- and only the claimant pushes. `claim_token` names the Ganbaru process that holds the claim,
-- and is null once the claim was released after a failure; a row whose token names another
-- process was left by a process that stopped, so the next attempt takes it over. `object_id` is
-- the commit the claimant pushes, recorded before the push, so an attempt that takes over an
-- interrupted push asks the remote whether that commit arrived. A `pushed` row keeps the commit
-- that reached the remote, so a retry posts the confirmation reply without pushing again.
CREATE TABLE chat_push_request_claims (
  request_item_id TEXT PRIMARY KEY NOT NULL REFERENCES chat_conversation_items(id)
      ON UPDATE CASCADE ON DELETE CASCADE,
  state TEXT NOT NULL CHECK (state IN ('pushing', 'pushed')),
  claim_token TEXT CHECK (claim_token IS NULL OR length(claim_token) BETWEEN 1 AND 128),
  object_id TEXT CHECK (
      object_id IS NULL OR (length(object_id) IN (40, 64) AND object_id NOT GLOB '*[^0-9a-f]*')
  ),
  claimed_at TEXT NOT NULL CHECK (length(claimed_at) >= 20),
  updated_at TEXT NOT NULL CHECK (length(updated_at) >= 20),
  CHECK (state = 'pushing' OR object_id IS NOT NULL)
) STRICT, WITHOUT ROWID;
