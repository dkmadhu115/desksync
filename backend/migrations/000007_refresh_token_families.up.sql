-- Group refresh tokens into per-login families.
--
-- A rotation chain starts at sign-in and every rotation inherits the family id,
-- so responding to a stolen token means revoking one family — one device's
-- session — instead of every token the account has. Before this, a single
-- suspicious refresh from any client signed the user out everywhere.

ALTER TABLE refresh_tokens ADD COLUMN family_id UUID;

-- Existing chains predate the column, so each surviving token becomes its own
-- family. That is deliberately conservative: revoking such a family revokes
-- only that token, and every new sign-in gets a real family.
UPDATE refresh_tokens SET family_id = id WHERE family_id IS NULL;

ALTER TABLE refresh_tokens ALTER COLUMN family_id SET NOT NULL;

CREATE INDEX idx_refresh_tokens_family_id ON refresh_tokens(family_id);
