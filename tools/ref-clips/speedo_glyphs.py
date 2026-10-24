# Segment speedometer digit glyphs from a 960x540 clip at 30 fps.
import sys, subprocess, numpy as np, json
clip, out = sys.argv[1], sys.argv[2]
X0, Y0, W, H = 646, 461, 32, 18
raw = subprocess.run(['ffmpeg','-loglevel','error','-i',clip,'-vf',f'fps=30,crop={W}:{H}:{X0}:{Y0},format=gray','-f','rawvideo','-'],capture_output=True).stdout
fr = np.frombuffer(raw, np.uint8).reshape(-1, H, W)
res = []
for k, f in enumerate(fr):
    b = f > 150
    cols = b.sum(0) > 0
    # runs of columns
    runs = []; i = 0
    while i < W:
        if cols[i]:
            j = i
            while j < W and cols[j]: j += 1
            runs.append((i, j)); i = j
        else: i += 1
    glyphs = []
    for (a, c) in runs:
        g = b[:, a:c]
        rows = np.where(g.sum(1) > 0)[0]
        if len(rows) == 0: continue
        g = g[rows[0]:rows[-1]+1]
        if g.shape[0] < 8 or g.shape[1] > 10: continue  # digit height ~11-13
        # normalise to 12x8
        yy = (np.arange(12) * g.shape[0] / 12).astype(int); xx = (np.arange(8) * g.shape[1] / 8).astype(int)
        glyphs.append(g[yy][:, xx].astype(np.uint8).ravel().tolist() + [g.shape[1]])
    res.append(glyphs)
json.dump(res, open(out, 'w'))
print(len(fr), 'frames;', sum(len(g) for g in res), 'glyphs')
