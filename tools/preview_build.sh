#!/bin/bash
# Build and stage the testbed preview for an Artifact publish.
# Usage: OUT=/path/to/stage tools/preview_build.sh <version>   (stages into $OUT/preview)
# Then publish $OUT/preview/index.html with root=$OUT/preview and a files map of the changed
# assets (.ron as text/plain, .wasm as application/wasm; null for old hashed assets).
# One publish is limited to 255 files / 64 MB, so send only what changed after the first.
set -e
V=$1; S=${OUT:?set OUT to a staging dir}
R=$(cd "$(dirname "$0")/.." && pwd); W=$R/apps/testbed-web
export PATH=$HOME/.cargo/bin:$PATH CARGO_INCREMENTAL=0 CARGO_PROFILE_RELEASE_DEBUG=0
cd $W && npm run -s wasm >/tmp/claude-0/pv-wasm.log 2>&1 && npx tsc --noEmit -p . && rm -rf dist && npx vite build --base ./ >/tmp/claude-0/pv-vite.log 2>&1
# Strip the wasm name section (artifact per-file limit 15 MB).
python3 - "$W/dist/assets" <<'PY'
import sys,glob,os
for f in glob.glob(sys.argv[1]+'/*.wasm'):
    b=open(f,'rb').read(); out=bytearray(b[:8]); i=8
    def leb(i):
        r=s=0
        while True:
            x=b[i]; i+=1; r|=(x&0x7f)<<s; s+=7
            if x<0x80: return r,i
    while i<len(b):
        sid=b[i]; sz,j=leb(i+1); end=j+sz
        if sid==0:
            nl,k=leb(j); name=b[k:k+nl].decode('utf8','replace')
            if name=='name' or name.startswith('.debug'): i=end; continue
        out+=b[i:end]; i=end
    open(f,'wb').write(out); print(os.path.basename(f), len(b)//1024, '->', len(out)//1024, 'KiB')
PY
rm -rf $S/preview && mkdir -p $S/preview && cp -r $W/dist/* $S/preview/
P=$S/preview; mkdir -p $P/data/city $P/data/panels $P/data/audio $P/scenarios
cp $R/data/city/*.greybox.b64.txt $P/data/city/ 2>/dev/null || true
# Cities shipped as parts (vesper_v2, ashpeak: city-import `parts`) list them here.
cp $R/data/city/*.greybox.parts.json $P/data/city/ 2>/dev/null || true
# Split greyboxes over the 15 MB per-file cap into 12 MB parts plus a count manifest
# (scene.ts fetchGreyboxB64 joins them).
for f in $P/data/city/*.greybox.b64.txt; do
  if [ $(stat -c %s "$f") -gt 14000000 ]; then
    b=${f%.txt}; split -b 12000000 -d -a 1 "$f" "$b.part"
    n=0; for p in "$b".part[0-9]; do mv "$p" "$p.txt"; n=$((n+1)); done
    echo $n > "$b.parts.txt"; rm "$f"; echo "split $(basename $f) into $n parts"
  fi
done
cp -r $R/data/panels/meshes $P/data/panels/
cp -r $R/data/audio/packs $P/data/audio/ && find $P/data/audio -name "*.json" -delete
(cd $R/apps/simcli/scenarios && find . -name '*.ron' | sed 's#^\./##' | sort) > /tmp/claude-0/pv-ron.txt
(cd $R/apps/simcli/scenarios && cp --parents $(cat /tmp/claude-0/pv-ron.txt) $P/scenarios/)
python3 -c "import json;print(json.dumps([l.strip() for l in open('/tmp/claude-0/pv-ron.txt') if l.strip()]))" > $P/scenarios/index.json
python3 - $P/index.html "$V" <<'PY'
import sys,re
p,v=sys.argv[1],sys.argv[2]; s=open(p).read()
s=re.sub(r'<title>.*?</title>','<title>WreckPursuit Preview</title>',s,1)
s=re.sub(r'<div id="wp-version".*?</div>\n?','',s)
s=s.replace('</body>','<div id="wp-version" style="position:fixed;right:max(6px,env(safe-area-inset-right,0px));bottom:max(4px,env(safe-area-inset-bottom,0px));z-index:60;font:11px/1.2 ui-monospace,monospace;color:#9aa3b5;background:rgba(0,0,0,0.45);padding:2px 6px;border-radius:3px;pointer-events:none">v'+v+' preview</div>\n</body>',1)
open(p,'w').write(s)
PY
du -sh $P; find $P -size +15M
