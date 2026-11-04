-- Bound the contact card revision far from the integer limit, so card rotation always has room to
-- stay in integers.
DROP TRIGGER sync_values_contact_card_insert;
DROP TRIGGER sync_values_contact_card_update;

CREATE TRIGGER sync_values_contact_card_insert
BEFORE INSERT ON contact_card
WHEN ((typeof(NEW.key) = 'text' AND instr(CAST(NEW.key AS BLOB), x'00') = 0 AND trim(NEW.key) <> '' AND length(CAST(NEW.key AS BLOB)) <= 128)
    AND (typeof(NEW.card_nonce) = 'text' AND instr(CAST(NEW.card_nonce AS BLOB), x'00') = 0 AND length(NEW.card_nonce) <= 22 AND length(CAST(NEW.card_nonce AS BLOB)) <= 262144)
    AND (typeof(NEW.card_revision) = 'integer' AND NEW.card_revision BETWEEN 1 AND 1000000000)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_contact_card_update
BEFORE UPDATE OF card_nonce, card_revision, updated_at ON contact_card
WHEN ((typeof(NEW.card_nonce) = 'text' AND instr(CAST(NEW.card_nonce AS BLOB), x'00') = 0 AND length(NEW.card_nonce) <= 22 AND length(CAST(NEW.card_nonce AS BLOB)) <= 262144)
    AND (typeof(NEW.card_revision) = 'integer' AND NEW.card_revision BETWEEN 1 AND 1000000000)
    AND (typeof(NEW.updated_at) = 'text' AND instr(CAST(NEW.updated_at AS BLOB), x'00') = 0 AND trim(NEW.updated_at) <> '' AND length(CAST(NEW.updated_at AS BLOB)) <= 64)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;
