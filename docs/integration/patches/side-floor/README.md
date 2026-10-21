# lead/side-floor as patches

The round12-ready spin side-force floor (straight launches on cambered roads), built on
`origin/lead/round12` @ 687f8af2. Delivered as `git am` patches because this session pushes only
`lead/tyre-slip`. To create the branch:

    git fetch origin lead/round12 lead/tyre-slip
    git checkout -b lead/side-floor origin/lead/round12
    git am path/to/docs/integration/patches/side-floor/*.patch

0001 = the floor + roster camber-launch test (own commit), 0002 = [bless] of the 50 simcli
goldens + 8 replays + 2 pinned hashes it moves (reasons in docs/integration/side-floor.md),
0003 = gate results vs round12 (two new tuning-sensitive wp_race police-race reds, listed there).
