-- Skill matching for racers (see src/matchmaking.rs).
-- tickets.rating: the clean rating the ticket queued with (party: members' mean). NULL on
-- tickets from before this migration = the initial rating.
ALTER TABLE tickets ADD COLUMN rating REAL;
-- Racers the instance listed in its last heartbeat: count and summed current clean rating.
ALTER TABLE instances ADD COLUMN racer_rating_sum REAL NOT NULL DEFAULT 0;
ALTER TABLE instances ADD COLUMN racer_rated INTEGER NOT NULL DEFAULT 0;
