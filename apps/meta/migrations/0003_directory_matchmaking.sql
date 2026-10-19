-- Instance directory, parties, matchmaking tickets, reconnect tokens, kicks.

CREATE TABLE instances (
    id                 INTEGER PRIMARY KEY,
    token_hash         TEXT    NOT NULL UNIQUE,
    name               TEXT    NOT NULL,
    region             TEXT    NOT NULL,
    city               TEXT    NOT NULL,
    mode               TEXT    NOT NULL,
    bracket            TEXT    NOT NULL,
    address            TEXT    NOT NULL,
    -- JSON RoleCounts: maximum players per role.
    slots_json         TEXT    NOT NULL,
    -- JSON RoleCounts: last reported population.
    population_json    TEXT    NOT NULL,
    -- 'up' | 'draining' | 'down'
    state              TEXT    NOT NULL,
    registered_ms      INTEGER NOT NULL,
    last_heartbeat_ms  INTEGER NOT NULL,
    tick_ms            REAL    NOT NULL DEFAULT 0
);

CREATE TABLE parties (
    id          INTEGER PRIMARY KEY,
    leader_id   INTEGER NOT NULL REFERENCES accounts(id),
    code_hash   TEXT    NOT NULL,
    created_ms  INTEGER NOT NULL
);
CREATE TABLE party_members (
    account_id  INTEGER PRIMARY KEY REFERENCES accounts(id),
    party_id    INTEGER NOT NULL REFERENCES parties(id),
    joined_ms   INTEGER NOT NULL
);

-- One ticket per account per queue entry. state: queued | assigned | joined | cancelled | expired
CREATE TABLE tickets (
    id               INTEGER PRIMARY KEY,
    account_id       INTEGER NOT NULL REFERENCES accounts(id),
    party_id         INTEGER,
    role             TEXT    NOT NULL,
    mode             TEXT    NOT NULL,
    bracket          TEXT    NOT NULL,
    region           TEXT    NOT NULL,
    city             TEXT,
    state            TEXT    NOT NULL,
    instance_id      INTEGER,
    join_token_hash  TEXT,
    join_expires_ms  INTEGER,
    created_ms       INTEGER NOT NULL,
    assigned_ms      INTEGER
);
CREATE INDEX tickets_state ON tickets(state, instance_id);
CREATE INDEX tickets_account ON tickets(account_id);

CREATE TABLE reconnect_tokens (
    token_hash   TEXT    PRIMARY KEY,
    account_id   INTEGER NOT NULL REFERENCES accounts(id),
    instance_id  INTEGER NOT NULL,
    role         TEXT    NOT NULL,
    expires_ms   INTEGER NOT NULL
);

-- Kicks queued by admins; delivered to the instance in its next heartbeat response.
CREATE TABLE kicks (
    id            INTEGER PRIMARY KEY,
    account_id    INTEGER NOT NULL REFERENCES accounts(id),
    reason        TEXT    NOT NULL,
    created_ms    INTEGER NOT NULL,
    delivered_ms  INTEGER
);
