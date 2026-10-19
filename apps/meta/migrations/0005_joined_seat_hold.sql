-- A joined player holds their seat until a heartbeat lists them (`reported = 1`), so the
-- matcher cannot hand the seat out again between join verification and the next heartbeat.
-- Legacy joined tickets (joined_ms NULL) are treated as already reported.
ALTER TABLE tickets ADD COLUMN joined_ms INTEGER;
ALTER TABLE tickets ADD COLUMN reported INTEGER NOT NULL DEFAULT 1;
