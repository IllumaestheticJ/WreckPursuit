-- Progression: economy accounts (garage, mods, bounty, cop rank live in the PlayerAccount JSON),
-- the append-only economy event log, police/heat history, crews and territory, race archive.

-- Latest wp_economy::PlayerAccount per account. Hot fields are copied out for queries.
CREATE TABLE econ_accounts (
    account_id  INTEGER PRIMARY KEY REFERENCES accounts(id),
    json        TEXT    NOT NULL,
    cash        INTEGER NOT NULL,
    rep         INTEGER NOT NULL,
    bounty      INTEGER NOT NULL,
    cop_rank    INTEGER NOT NULL,
    next_seq    INTEGER NOT NULL,
    updated_ms  INTEGER NOT NULL
);

-- wp_economy::EconomyEvent archive. Gap-free seq per account; never updated or deleted.
CREATE TABLE econ_events (
    account_id   INTEGER NOT NULL REFERENCES accounts(id),
    seq          INTEGER NOT NULL,
    kind         INTEGER NOT NULL,
    json         TEXT    NOT NULL,
    recorded_ms  INTEGER NOT NULL,
    PRIMARY KEY (account_id, seq)
);
CREATE TRIGGER econ_events_no_update BEFORE UPDATE ON econ_events
BEGIN SELECT RAISE(ABORT, 'econ_events is append-only'); END;
CREATE TRIGGER econ_events_no_delete BEFORE DELETE ON econ_events
BEGIN SELECT RAISE(ABORT, 'econ_events is append-only'); END;

-- Race results reported by instances (append-only archive). report_key makes reports idempotent.
CREATE TABLE race_events (
    id           INTEGER PRIMARY KEY,
    report_key   TEXT    NOT NULL UNIQUE,
    instance_id  INTEGER NOT NULL,
    route_id     TEXT    NOT NULL,
    race_type    TEXT    NOT NULL,
    mode         TEXT    NOT NULL,
    bracket      TEXT    NOT NULL,
    recorded_ms  INTEGER NOT NULL,
    json         TEXT    NOT NULL
);
CREATE TABLE race_results (
    race_id         INTEGER NOT NULL REFERENCES race_events(id),
    account_id      INTEGER NOT NULL REFERENCES accounts(id),
    position        INTEGER NOT NULL,
    finish_time_ms  INTEGER,
    clean           INTEGER NOT NULL,
    rating_before   REAL    NOT NULL,
    rating_after    REAL    NOT NULL,
    PRIMARY KEY (race_id, account_id)
);
CREATE TRIGGER race_events_no_update BEFORE UPDATE ON race_events
BEGIN SELECT RAISE(ABORT, 'race_events is append-only'); END;
CREATE TRIGGER race_events_no_delete BEFORE DELETE ON race_events
BEGIN SELECT RAISE(ABORT, 'race_events is append-only'); END;

-- Pursuit outcomes (bounty / heat history). Append-only.
CREATE TABLE pursuit_events (
    id           INTEGER PRIMARY KEY,
    report_key   TEXT    NOT NULL UNIQUE,
    instance_id  INTEGER NOT NULL,
    racer_id     INTEGER NOT NULL REFERENCES accounts(id),
    max_wanted   INTEGER NOT NULL,
    bounty       INTEGER NOT NULL,
    outcome      TEXT    NOT NULL,
    captured_by  INTEGER,
    recorded_ms  INTEGER NOT NULL,
    json         TEXT    NOT NULL
);
CREATE INDEX pursuit_events_racer ON pursuit_events(racer_id);
CREATE TRIGGER pursuit_events_no_update BEFORE UPDATE ON pursuit_events
BEGIN SELECT RAISE(ABORT, 'pursuit_events is append-only'); END;
CREATE TRIGGER pursuit_events_no_delete BEFORE DELETE ON pursuit_events
BEGIN SELECT RAISE(ABORT, 'pursuit_events is append-only'); END;

-- Clean-race rating (persistent, Elo-style; see src/leaderboards.rs).
CREATE TABLE ratings (
    account_id  INTEGER PRIMARY KEY REFERENCES accounts(id),
    clean_rating REAL   NOT NULL,
    races       INTEGER NOT NULL
);

-- Crews and the districts they hold.
CREATE TABLE crews (
    id          INTEGER PRIMARY KEY,
    name        TEXT    NOT NULL UNIQUE,
    tag         TEXT    NOT NULL UNIQUE,
    leader_id   INTEGER NOT NULL REFERENCES accounts(id),
    created_ms  INTEGER NOT NULL
);
CREATE TABLE crew_members (
    account_id  INTEGER PRIMARY KEY REFERENCES accounts(id),
    crew_id     INTEGER NOT NULL REFERENCES crews(id),
    joined_ms   INTEGER NOT NULL
);
CREATE INDEX crew_members_crew ON crew_members(crew_id);
-- district_id is a string until the contract's DistrictId lands (economy request #6).
CREATE TABLE territories (
    city        TEXT    NOT NULL,
    district_id TEXT    NOT NULL,
    crew_id     INTEGER REFERENCES crews(id),
    since_ms    INTEGER NOT NULL,
    PRIMARY KEY (city, district_id)
);
