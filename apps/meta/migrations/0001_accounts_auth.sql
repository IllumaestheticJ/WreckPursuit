-- Accounts, login identities and sessions.
-- Portable SQL: INTEGER/TEXT/REAL only, no SQLite-only types, so the same files port to Postgres
-- with `INTEGER PRIMARY KEY` -> `BIGSERIAL PRIMARY KEY` (see README "Postgres").

-- One row per player. `id` is the durable account id (the economy's PlayerId until the
-- contract's AccountId lands, see docs/integration/meta.md MC-1).
CREATE TABLE accounts (
    id              INTEGER PRIMARY KEY,
    display_name    TEXT    NOT NULL UNIQUE,
    created_ms      INTEGER NOT NULL,
    -- NULL = not banned. 9223372036854775807 = permanent.
    banned_until_ms INTEGER,
    ban_reason      TEXT
);

-- Ways to log in to an account. provider = 'password' (subject = normalised email) today;
-- OAuth providers add rows with secret_hash NULL.
CREATE TABLE identities (
    provider    TEXT    NOT NULL,
    subject     TEXT    NOT NULL,
    account_id  INTEGER NOT NULL REFERENCES accounts(id),
    secret_hash TEXT,
    created_ms  INTEGER NOT NULL,
    PRIMARY KEY (provider, subject)
);
CREATE INDEX identities_account ON identities(account_id);

-- Session tokens. Only the SHA-256 of the token is stored.
CREATE TABLE sessions (
    token_hash  TEXT    PRIMARY KEY,
    account_id  INTEGER NOT NULL REFERENCES accounts(id),
    created_ms  INTEGER NOT NULL,
    expires_ms  INTEGER NOT NULL,
    revoked     INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX sessions_account ON sessions(account_id);

-- Account-linking requests (stub: recorded, never completed until an OAuth provider exists).
CREATE TABLE link_requests (
    id          INTEGER PRIMARY KEY,
    account_id  INTEGER NOT NULL REFERENCES accounts(id),
    provider    TEXT    NOT NULL,
    created_ms  INTEGER NOT NULL
);
