-- Leaderboards, feature flags, audit log.

-- period: 'all', 'd:YYYY-MM-DD' (UTC day) or 'w:YYYY-MM-DD' (UTC Monday of the week).
-- A new period key is a reset; old periods stay for history.
CREATE TABLE lb_entries (
    board       TEXT    NOT NULL,
    period      TEXT    NOT NULL,
    account_id  INTEGER NOT NULL REFERENCES accounts(id),
    score       REAL    NOT NULL,
    updated_ms  INTEGER NOT NULL,
    PRIMARY KEY (board, period, account_id)
);
CREATE INDEX lb_entries_rank ON lb_entries(board, period, score);

CREATE TABLE feature_flags (
    key         TEXT    PRIMARY KEY,
    value_json  TEXT    NOT NULL,
    updated_ms  INTEGER NOT NULL,
    updated_by  TEXT    NOT NULL
);
INSERT INTO feature_flags (key, value_json, updated_ms, updated_by) VALUES
    ('illegal_day_race_frequency', '0.15', 0, 'migration'),
    ('player_police_enabled', 'true', 0, 'migration'),
    ('simulation_mode_enabled', 'true', 0, 'migration');

CREATE TABLE audit_log (
    id          INTEGER PRIMARY KEY,
    at_ms       INTEGER NOT NULL,
    actor       TEXT    NOT NULL,
    action      TEXT    NOT NULL,
    target      TEXT    NOT NULL,
    details     TEXT    NOT NULL
);
CREATE TRIGGER audit_log_no_update BEFORE UPDATE ON audit_log
BEGIN SELECT RAISE(ABORT, 'audit_log is append-only'); END;
CREATE TRIGGER audit_log_no_delete BEFORE DELETE ON audit_log
BEGIN SELECT RAISE(ABORT, 'audit_log is append-only'); END;
