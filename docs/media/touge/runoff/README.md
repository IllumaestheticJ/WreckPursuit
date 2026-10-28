# Touge run-offs (2026-10-28)

Owner: "touge tracks need more run off so we can slow down after we finish". Each pass and each
end, before (`main` at 7fa64d8a: 40 m from the finish line to a wall) and after (340 m of paved
run-off, then a 45 m railed gravel bed with a wall behind it):

- `<pass>_<base|summit>_high_<before|after>.jpg`: high view over the finish line, the run-off
  and the bed.
- `<pass>_<base|summit>_low_<before|after>.jpg`: 20 m before the finish line, 4 m up, looking
  along the run-off (the base shots include the uphill start grid's cars).

`base` is the downhill finish, `summit` the uphill one. Made with
`apps/testbed-web/e2e/runoff_shots.mjs` (`TAG=before|after`, camera spots in
`e2e/runoff_spots.json`; the course is unchanged, so the same spots work on both builds).
