#!/bin/sh
# All four reference clips: yaw + held keys (see README.md). Run from the repo root.
set -e
D=docs/reference/wf-clips
T=$(mktemp -d)
for spec in "1630_parity_rwd_e:_track:top:Off" "1634_parity_rwd_f:_track:top:Off" \
            "1637_parity_rwd_f_b:_keys:bottom:Off" "1639_parity_rwd_f_tcs_half:_keys:bottom:Half"; do
  IFS=: read -r n k l tcs <<EOS
$spec
EOS
  python3 -I tools/ref-clips/yaw_from_clip.py "$D/2026-10-21_$n.mp4" "$D/2026-10-21_$n$k.json" "$l" "$T/$n.json"
  python3 -I tools/ref-clips/held_keys.py "$T/$n.json" "$D/2026-10-21_${n}_yaw.json" "$tcs"
done
rm -rf "$T"
