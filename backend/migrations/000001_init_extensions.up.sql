-- Enable extensions used across the schema.
-- pgcrypto provides gen_random_uuid() for UUID primary keys.
-- citext gives case-insensitive text for emails.
CREATE EXTENSION IF NOT EXISTS pgcrypto;
CREATE EXTENSION IF NOT EXISTS citext;
