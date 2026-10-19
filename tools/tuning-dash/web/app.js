// Tuning dashboard. Plain JS, no build step, no dependencies. Talks to tuning_dash/server.py.
"use strict";

const MAX_SERIES = 8; // categorical palette size; more lines must be pinned, never new hues
const $ = (sel) => document.querySelector(sel);
const NS = "http://www.w3.org/2000/svg";

const S = {
  sweeps: [],
  sweep: null,
  xDim: null,
  dims: {},       // dim name -> "lines" | "pin:<key>"
  picked: [],     // metric base names shown as charts
  speed: null,    // which "@speed" suffix to use for flat barrier metrics
  selected: null, // point id
  A: null,
  B: null,
  sort: { key: "id", dir: 1 },
};

// ---------- helpers ----------
function esc(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
}
function fmt(v) {
  if (v === null || v === undefined || Number.isNaN(v)) return "—";
  if (typeof v !== "number") return String(v);
  const a = Math.abs(v);
  if (Number.isInteger(v)) return v.toLocaleString("en-US");
  if (a >= 10000) return v.toLocaleString("en-US", { maximumFractionDigits: 0 });
  if (a >= 100) return v.toLocaleString("en-US", { maximumFractionDigits: 1 });
  if (a === 0) return "0";
  return Number(v.toPrecision(4)).toString();
}
function fmtAxis(v) {
  const a = Math.abs(v);
  if (a >= 1e6) return (v / 1e6).toFixed(1).replace(/\.0$/, "") + "M";
  if (a >= 1e3) return (v / 1e3).toFixed(1).replace(/\.0$/, "") + "K";
  return Number(v.toPrecision(3)).toString();
}
const keyOf = (v) => (v === undefined ? "·" : JSON.stringify(v));
const labelOf = (k) => (k === "·" ? "base" : (() => { const v = JSON.parse(k); return typeof v === "string" ? v : fmt(v); })());
async function api(path, body) {
  const opts = body === undefined ? {} : { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) };
  const r = await fetch(path, opts);
  return r.json();
}
function el(tag, attrs = {}, parent = null) {
  const e = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
  if (parent) parent.appendChild(e);
  return e;
}
function niceTicks(lo, hi, n = 5) {
  if (lo === hi) { const d = Math.abs(lo) * 0.1 || 1; lo -= d; hi += d; }
  const span = hi - lo;
  const step0 = span / n;
  const mag = Math.pow(10, Math.floor(Math.log10(step0)));
  const step = [1, 2, 2.5, 5, 10].map((m) => m * mag).find((s) => span / s <= n) || 10 * mag;
  const t0 = Math.floor(lo / step) * step;
  const ticks = [];
  for (let t = t0; t <= hi + step * 0.5; t += step) ticks.push(Number(t.toPrecision(12)));
  return ticks;
}
function statusHtml(p) {
  if (!p.ok) return `<span class="status err"><span class="ic">!</span>error</span>`;
  if (p.passed === true) return `<span class="status pass"><span class="ic">✓</span>pass</span>`;
  if (p.passed === false) {
    const n = p.checks.filter((c) => !c.passed && !c.pending).length;
    return `<span class="status fail"><span class="ic">✕</span>fail (${n})</span>`;
  }
  return `<span class="status none">no checks</span>`;
}

// ---------- dimensions ----------
function paramDefs() { return S.sweep ? S.sweep.params : []; }
function dimNames() {
  const d = [];
  if (S.sweep.variants.length > 1) d.push("variant");
  for (const p of paramDefs()) d.push(p.label);
  return d;
}
function dimValue(p, dim) { return dim === "variant" ? JSON.stringify(p.variant) : keyOf(p.params[dim]); }
function dimKeys(dim) {
  if (dim === "variant") return S.sweep.variants.map((v) => JSON.stringify(v));
  const def = paramDefs().find((q) => q.label === dim);
  const keys = def.values.map(keyOf);
  if (S.sweep.combine === "each") keys.push("·");
  return keys;
}
function hasSeries() { return S.sweep.points.some((p) => p.series); }
function isEach() { return S.sweep.combine === "each"; }

function xOptions() {
  const o = paramDefs().map((p) => p.label);
  if (hasSeries()) o.push("speed_kmh");
  if (S.sweep.variants.length > 1 || !o.length) o.push("variant");
  return o;
}

function neutralKey(dim) {
  if (dim === "variant") return dimKeys(dim)[0];
  const def = paramDefs().find((q) => q.label === dim);
  if (isEach()) return "·";
  const neutral = def.op === "scale" ? 1 : def.op === "add" ? 0 : null;
  const k = def.values.map(keyOf);
  const hit = neutral === null ? -1 : def.values.findIndex((v) => v === neutral);
  return k[hit >= 0 ? hit : Math.floor(k.length / 2)];
}

function defaultDims() {
  S.dims = {};
  let count = 1;
  for (const d of dimNames()) {
    if (d === S.xDim) continue;
    if (isEach() && d !== "variant" && S.xDim !== "speed_kmh" && S.xDim !== "variant") continue; // one param at a time
    const n = new Set(S.sweep.points.map((p) => dimValue(p, d))).size;
    if (count * n <= MAX_SERIES) { S.dims[d] = "lines"; count *= n; } else S.dims[d] = "pin:" + neutralKey(d);
  }
}

// Points on screen and their series key.
function visiblePoints() {
  return S.sweep.points.filter((p) => {
    if (!p.ok) return false;
    if (S.xDim !== "speed_kmh" && S.xDim !== "variant" && !(S.xDim in p.params)) return false;
    for (const [d, mode] of Object.entries(S.dims)) {
      if (mode.startsWith("pin:") && dimValue(p, d) !== mode.slice(4)) return false;
    }
    return true;
  });
}
function lineDims() { return Object.entries(S.dims).filter(([, m]) => m === "lines").map(([d]) => d); }
function seriesKey(p) { return lineDims().map((d) => dimValue(p, d)).join("|") || "all"; }
function seriesLabel(key) {
  if (key === "all") return S.sweep.name;
  const ds = lineDims();
  return key.split("|").map((k, i) => (ds[i] === "variant" ? labelOf(k) : `${ds[i]} ${labelOf(k)}`)).join(" · ");
}
// Colour follows the entity: index in the full product of line-dim values, not in what is shown.
function seriesColorIndex(key) {
  if (key === "all") return 0;
  const ds = lineDims();
  let idx = 0;
  key.split("|").forEach((k, i) => {
    const ks = dimKeys(ds[i]);
    idx = idx * ks.length + Math.max(0, ks.indexOf(k));
  });
  return idx;
}
const color = (i) => `var(--s${(i % MAX_SERIES) + 1})`;

// ---------- metrics ----------
function flatKeys() {
  const ks = new Set();
  for (const p of S.sweep.points) for (const [k, v] of Object.entries(p.metrics)) if (typeof v === "number" || v === null) ks.add(k);
  return [...ks].sort();
}
function baseMetrics() {
  if (S.xDim === "speed_kmh") {
    const ks = new Set();
    for (const p of S.sweep.points) if (p.series) Object.keys(p.series.y).forEach((k) => ks.add(k));
    return [...ks].sort();
  }
  return [...new Set(flatKeys().map((k) => k.split("@")[0]))].sort();
}
function speeds() {
  const s = new Set();
  for (const k of flatKeys()) if (k.includes("@")) s.add(k.split("@")[1]);
  return [...s].sort((a, b) => Number(a) - Number(b));
}
function flatKey(base) { return S.speed && flatKeys().includes(`${base}@${S.speed}`) ? `${base}@${S.speed}` : base; }

function defaultPicked() {
  const avail = baseMetrics();
  const fromSpec = (S.sweep.plots || []).map((m) => m.split("@")[0]).filter((m) => avail.includes(m));
  const pref = ["crush_m", "health", "failed_components", "repair_total"];
  let picked = [...new Set(fromSpec)];
  if (!picked.length) picked = pref.filter((m) => avail.includes(m));
  if (!picked.length) picked = avail.filter((m) => !m.endsWith(".passed")).slice(0, 4);
  return picked.slice(0, 6);
}

// ---------- sweep loading ----------
async function loadSweeps(selectName) {
  S.sweeps = await api("/api/sweeps");
  const sel = $("#sweep-select");
  sel.innerHTML = S.sweeps.map((s) => `<option value="${esc(s.name)}">${esc(s.name)} (${s.points})</option>`).join("");
  const want = selectName || new URLSearchParams(location.hash.slice(1)).get("sweep") || (S.sweeps[0] && S.sweeps[0].name);
  if (want && S.sweeps.some((s) => s.name === want)) {
    sel.value = want;
    await loadSweep(want);
  } else if (!S.sweeps.length) {
    $("#sweep-head").innerHTML = `<p class="muted">No sweeps yet. Pick a preset and press <b>Run sweep</b>, or run <code>tools/tuning-dash/tune.py sweep &lt;preset&gt;</code>.</p>`;
  }
}

async function loadSweep(name) {
  S.sweep = await api(`/api/sweep/${encodeURIComponent(name)}`);
  history.replaceState(null, "", `#sweep=${encodeURIComponent(name)}`);
  const opts = xOptions();
  S.xDim = opts[0];
  const sp = speeds();
  const plotSpeed = (S.sweep.plots || []).map((m) => m.split("@")[1]).find(Boolean);
  S.speed = sp.length ? (sp.includes(plotSpeed) ? plotSpeed : sp.includes("56") ? "56" : sp[sp.length - 1]) : null;
  defaultDims();
  S.picked = defaultPicked();
  S.selected = null;
  pickDefaultAB();
  renderAll();
}

function pickDefaultAB() {
  const pts = S.sweep.points.filter((p) => p.ok);
  const vs = S.sweep.variants;
  if (vs.length > 1) {
    const base = (v) => pts.find((p) => p.variant === v && p.is_base) || pts.find((p) => p.variant === v);
    S.A = base(vs[0])?.id || null;
    S.B = base(vs[1])?.id || null;
  } else {
    S.A = pts[0]?.id || null;
    S.B = pts.length > 1 ? pts[pts.length - 1].id : null;
  }
}

function renderAll() {
  renderOverview();
  renderControls();
  renderCharts();
  renderTable();
  renderDetail();
  renderCompare();
}

// ---------- overview ----------
function renderOverview() {
  const sw = S.sweep;
  const pts = sw.points;
  const pass = pts.filter((p) => p.passed === true).length;
  const fail = pts.filter((p) => p.passed === false && p.ok).length;
  const err = pts.filter((p) => !p.ok).length;
  const when = sw.finished_unix ? new Date(sw.finished_unix * 1000).toLocaleString() : "";
  $("#sweep-head").innerHTML = `
    <h2 style="margin-bottom:4px">${esc(sw.name)}</h2>
    <div class="muted">${esc(sw.description || "")}</div>
    <div class="muted small">${esc(sw.probe_kind)} probe · combine ${esc(sw.combine)} · spec <code>${esc(sw.spec_path || "?")}</code> · data <code>${esc(sw.data)}</code> · ${esc(when)}</div>`;
  const anyChecks = pts.some((p) => p.passed !== null && p.passed !== undefined);
  const tiles = [["Points", pts.length], ["Passing", anyChecks ? pass : "no checks"], ["Failing checks", fail], ["Errors", err], ["Sweep time", `${sw.wall_s} s`], ["Workers", sw.workers]];
  $("#tiles").innerHTML = tiles.map(([l, v]) => `<div class="tile"><div class="label">${l}</div><div class="value">${esc(v)}</div></div>`).join("");
  const errs = pts.filter((p) => !p.ok);
  if (errs.length) {
    $("#tiles").insertAdjacentHTML("afterend", "");
    $("#sweep-head").insertAdjacentHTML("beforeend", `<div class="result" style="margin-top:8px">${errs.slice(0, 5).map((p) => `${esc(p.id)}: ${esc(p.error)}`).join("\n")}</div>`);
  }
}

// ---------- controls ----------
function renderControls() {
  const c = $("#plot-controls");
  const xo = xOptions();
  let html = `<label>X axis <select id="x-dim">${xo.map((o) => `<option ${o === S.xDim ? "selected" : ""} value="${esc(o)}">${esc(o === "speed_kmh" ? "closing speed (km/h)" : o)}</option>`).join("")}</select></label>`;
  for (const [d, mode] of Object.entries(S.dims)) {
    const ks = [...new Set(S.sweep.points.map((p) => dimValue(p, d)))];
    const order = dimKeys(d).filter((k) => ks.includes(k));
    html += `<label>${esc(d)} <select data-dim="${esc(d)}"><option value="lines" ${mode === "lines" ? "selected" : ""}>one line each</option>${order
      .map((k) => `<option value="pin:${esc(k)}" ${mode === "pin:" + k ? "selected" : ""}>= ${esc(labelOf(k))}</option>`)
      .join("")}</select></label>`;
  }
  const sp = speeds();
  if (sp.length) {
    html += `<label>${S.xDim === "speed_kmh" ? "Table at" : "Metrics at"} <select id="speed-sel">${sp.map((s) => `<option ${s === S.speed ? "selected" : ""}>${s}</option>`).join("")}</select> km/h</label>`;
  }
  c.innerHTML = html;
  $("#x-dim").onchange = (e) => { S.xDim = e.target.value; defaultDims(); const avail = baseMetrics(); S.picked = S.picked.filter((m) => avail.includes(m)); if (!S.picked.length) S.picked = defaultPicked(); renderControls(); renderCharts(); renderTable(); };
  c.querySelectorAll("select[data-dim]").forEach((s) => (s.onchange = (e) => { S.dims[e.target.dataset.dim] = e.target.value; renderCharts(); renderTable(); }));
  if ($("#speed-sel")) $("#speed-sel").onchange = (e) => { S.speed = e.target.value; renderCharts(); renderTable(); };

  const mp = $("#metric-picker");
  mp.innerHTML = baseMetrics().map((m) => `<span class="chip" role="button" tabindex="0" aria-pressed="${S.picked.includes(m)}" data-m="${esc(m)}">${esc(m)}</span>`).join("");
  mp.querySelectorAll(".chip").forEach((ch) => {
    const toggle = () => {
      const m = ch.dataset.m;
      S.picked = S.picked.includes(m) ? S.picked.filter((x) => x !== m) : [...S.picked, m];
      ch.setAttribute("aria-pressed", S.picked.includes(m));
      renderCharts();
      renderTable();
    };
    ch.onclick = toggle;
    ch.onkeydown = (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); toggle(); } };
  });
}

// ---------- charts ----------
function buildSeries(metric) {
  const pts = visiblePoints();
  const groups = new Map();
  if (S.xDim === "speed_kmh") {
    // Each point is one line over its inner speed series.
    for (const p of pts) {
      if (!p.series || !p.series.y[metric]) continue;
      const key = seriesKey(p) + (lineDims().length ? "" : "");
      if (!groups.has(key)) groups.set(key, []);
      groups.get(key).push(p);
    }
    const out = [];
    for (const [key, ps] of groups) {
      for (const p of ps) {
        out.push({
          key: ps.length > 1 ? `${key}#${p.id}` : key,
          label: seriesLabel(key) + (ps.length > 1 ? ` (${p.id})` : ""),
          colorIdx: seriesColorIndex(key),
          pts: p.series.x.map((x, i) => ({ x, y: p.series.y[metric][i], p })),
        });
      }
    }
    return out;
  }
  const fk = flatKey(metric);
  if (S.xDim === "variant") {
    // One bar per variant (other dimensions are pinned).
    return S.sweep.variants
      .map((v, i) => ({ v, i, p: pts.find((q) => q.variant === v) }))
      .filter((r) => r.p)
      .map((r) => ({ key: r.v, label: r.v, colorIdx: r.i, pts: [{ x: r.i, y: r.p.metrics[fk] ?? null, p: r.p }] }));
  }
  for (const p of pts) {
    const key = seriesKey(p);
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push({ x: p.params[S.xDim], y: p.metrics[fk] ?? null, p });
  }
  return [...groups].map(([key, list]) => ({ key, label: seriesLabel(key), colorIdx: seriesColorIndex(key), pts: list.sort((a, b) => a.x - b.x) }));
}

function renderLegend(series) {
  const lg = $("#legend");
  if (series.length < 2) { lg.innerHTML = ""; return; }
  lg.innerHTML = series.slice(0, MAX_SERIES).map((s) => `<span class="key"><span class="sw" style="background:${color(s.colorIdx)}"></span>${esc(s.label)}</span>`).join("") +
    `<span class="key"><span class="dot" style="border:2px solid var(--text-muted)"></span>hollow = fails checks</span>`;
}

function renderCharts() {
  const box = $("#charts");
  box.innerHTML = "";
  $("#plot-note").textContent = "";
  if (!S.sweep) return;
  if (!S.picked.length) { box.innerHTML = `<p class="muted">Pick metrics above.</p>`; renderLegend([]); return; }
  let legendDone = false;
  for (const m of S.picked) {
    let series = buildSeries(m);
    if (series.length > MAX_SERIES) {
      $("#plot-note").textContent = `${series.length} lines match; showing the first ${MAX_SERIES}. Pin a dimension above to see the rest.`;
      series = series.filter((s) => s.colorIdx < MAX_SERIES).slice(0, MAX_SERIES);
    }
    if (!legendDone) { renderLegend(series); legendDone = true; }
    const wrap = document.createElement("div");
    wrap.className = "chart";
    const unit = S.xDim !== "speed_kmh" && S.speed && flatKeys().includes(`${m}@${S.speed}`) ? ` @ ${S.speed} km/h` : "";
    wrap.innerHTML = `<div class="title">${esc(m)}${esc(unit)}</div>`;
    box.appendChild(wrap);
    if (S.xDim === "variant") barChart(wrap, series, m);
    else lineChart(wrap, series, S.xDim === "speed_kmh" ? "km/h" : S.xDim);
  }
}

function lineChart(wrap, series, xLabel, opts = {}) {
  const W = 360, H = 220;
  const endLabels = series.length > 1 && series.length <= 4;
  const M = { l: 46, r: endLabels ? 74 : 14, t: 10, b: 30 };
  const all = series.flatMap((s) => s.pts).filter((q) => q.y !== null && q.y !== undefined && typeof q.x === "number");
  const svg = el("svg", { viewBox: `0 0 ${W} ${H}`, role: "img", "aria-label": wrap.textContent });
  wrap.appendChild(svg);
  if (!all.length) { el("text", { x: W / 2, y: H / 2, "text-anchor": "middle" }, svg).textContent = "no numeric values"; return; }
  const xs = [...new Set(all.map((q) => q.x))].sort((a, b) => a - b);
  const [x0, x1] = xs.length > 1 ? [xs[0], xs[xs.length - 1]] : [xs[0] - 1, xs[0] + 1];
  const yt = niceTicks(Math.min(...all.map((q) => q.y)), Math.max(...all.map((q) => q.y)));
  const [y0, y1] = [yt[0], yt[yt.length - 1]];
  const sx = (x) => M.l + ((x - x0) / (x1 - x0)) * (W - M.l - M.r);
  const sy = (y) => H - M.b - ((y - y0) / (y1 - y0 || 1)) * (H - M.t - M.b);
  for (const t of yt) {
    el("line", { x1: M.l, x2: W - M.r, y1: sy(t), y2: sy(t), class: "gridline" }, svg);
    el("text", { x: M.l - 6, y: sy(t) + 4, "text-anchor": "end" }, svg).textContent = fmtAxis(t);
  }
  el("line", { x1: M.l, x2: W - M.r, y1: H - M.b, y2: H - M.b, class: "axisline" }, svg);
  const xt = xs.length <= 8 ? xs : niceTicks(x0, x1, 6).filter((t) => t >= x0 && t <= x1);
  for (const t of xt) el("text", { x: sx(t), y: H - M.b + 14, "text-anchor": "middle" }, svg).textContent = fmtAxis(t);
  el("text", { x: (M.l + W - M.r) / 2, y: H - 2, "text-anchor": "middle" }, svg).textContent = xLabel;

  const cross = el("line", { y1: M.t, y2: H - M.b, class: "crosshair", visibility: "hidden" }, svg);
  const placed = [];
  for (const s of series) {
    let d = "";
    let pen = false;
    for (const q of s.pts) {
      if (q.y === null || q.y === undefined) { pen = false; continue; }
      d += `${pen ? "L" : "M"}${sx(q.x).toFixed(1)},${sy(q.y).toFixed(1)}`;
      pen = true;
    }
    el("path", { d, fill: "none", stroke: color(s.colorIdx), "stroke-width": 2, "stroke-linejoin": "round", "stroke-linecap": "round" }, svg);
  }
  for (const s of series) {
    for (const q of s.pts) {
      if (q.y === null || q.y === undefined) continue;
      const fails = q.p.passed === false;
      const sel = !opts.noSelect && q.p.id === S.selected;
      el("circle", {
        cx: sx(q.x), cy: sy(q.y), r: sel ? 6 : 4,
        fill: fails ? "var(--surface-1)" : color(s.colorIdx),
        stroke: fails ? color(s.colorIdx) : "var(--surface-1)", "stroke-width": 2,
      }, svg);
    }
    const last = [...s.pts].reverse().find((q) => q.y !== null && q.y !== undefined);
    if (endLabels && last) {
      const y = sy(last.y) + 4;
      if (!placed.some((py) => Math.abs(py - y) < 12)) {
        placed.push(y);
        const t = el("text", { x: sx(last.x) + 8, y, class: "endlabel" }, svg);
        t.textContent = s.label.length > 12 ? s.label.slice(0, 11) + "…" : s.label;
      }
    }
  }
  // Hover layer: nearest x → crosshair + tooltip; click selects the nearest marker.
  const hit = el("rect", { x: M.l, y: M.t, width: W - M.l - M.r, height: H - M.t - M.b, fill: "transparent" }, svg);
  const tip = $("#tooltip");
  const nearestX = (evt) => {
    const pt = svg.createSVGPoint();
    pt.x = evt.clientX; pt.y = evt.clientY;
    const loc = pt.matrixTransform(svg.getScreenCTM().inverse());
    let best = xs[0];
    for (const x of xs) if (Math.abs(sx(x) - loc.x) < Math.abs(sx(best) - loc.x)) best = x;
    return { x: best, y: loc.y };
  };
  hit.addEventListener("mousemove", (evt) => {
    const { x } = nearestX(evt);
    cross.setAttribute("x1", sx(x)); cross.setAttribute("x2", sx(x)); cross.setAttribute("visibility", "visible");
    const rows = series.map((s) => ({ s, q: s.pts.find((q) => q.x === x) })).filter((r) => r.q);
    tip.innerHTML = `<div class="muted">${esc(xLabel)} = ${esc(fmt(x))}</div>` + rows.map(({ s, q }) =>
      `<div class="row"><span class="dot" style="width:8px;height:8px;border-radius:50%;background:${color(s.colorIdx)}"></span>${esc(s.label)} <span class="muted">${esc(q.p.id)}${q.p.passed === false ? " ✕" : ""}</span><b>${esc(fmt(q.y))}</b></div>`).join("");
    tip.hidden = false;
    const r = tip.getBoundingClientRect();
    tip.style.left = Math.min(evt.clientX + 14, innerWidth - r.width - 8) + "px";
    tip.style.top = Math.min(evt.clientY + 14, innerHeight - r.height - 8) + "px";
  });
  hit.addEventListener("mouseleave", () => { cross.setAttribute("visibility", "hidden"); tip.hidden = true; });
  if (!opts.noSelect) {
    hit.style.cursor = "pointer";
    hit.addEventListener("click", (evt) => {
      const { x, y } = nearestX(evt);
      let best = null;
      for (const s of series) for (const q of s.pts) if (q.x === x && q.y !== null && (!best || Math.abs(sy(q.y) - y) < Math.abs(sy(best.y) - y))) best = q;
      if (best) selectPoint(best.p.id);
    });
  }
}

function barChart(wrap, series, metric) {
  // Categorical (x = variant): one horizontal bar per variant, 4 px rounded data end.
  const rows = series.map((s) => ({ s, q: s.pts[0] })).filter((r) => r.q);
  const fk = flatKey(metric);
  const vals = rows.map((r) => r.q.p.metrics[fk]).filter((v) => typeof v === "number");
  const W = 360, rowH = 34, M = { l: 110, r: 56, t: 6, b: 22 };
  const H = M.t + M.b + rowH * rows.length;
  const svg = el("svg", { viewBox: `0 0 ${W} ${H}`, role: "img" });
  wrap.appendChild(svg);
  if (!vals.length) { el("text", { x: W / 2, y: H / 2, "text-anchor": "middle" }, svg).textContent = "no numeric values"; return; }
  const ticks = niceTicks(Math.min(0, ...vals), Math.max(0, ...vals), 4);
  const [v0, v1] = [ticks[0], ticks[ticks.length - 1]];
  const sx = (v) => M.l + ((v - v0) / (v1 - v0 || 1)) * (W - M.l - M.r);
  for (const t of ticks) {
    el("line", { x1: sx(t), x2: sx(t), y1: M.t, y2: H - M.b, class: "gridline" }, svg);
    el("text", { x: sx(t), y: H - 6, "text-anchor": "middle" }, svg).textContent = fmtAxis(t);
  }
  rows.forEach(({ s, q }, i) => {
    const v = q.p.metrics[fk];
    const yc = M.t + rowH * i + rowH / 2;
    el("text", { x: M.l - 8, y: yc + 4, "text-anchor": "end", class: "endlabel" }, svg).textContent = s.label.length > 16 ? s.label.slice(0, 15) + "…" : s.label;
    if (typeof v !== "number") { el("text", { x: sx(0) + 4, y: yc + 4 }, svg).textContent = "—"; return; }
    const a = sx(0), b = sx(v), h = 18, r = Math.min(4, Math.abs(b - a));
    const dir = b >= a ? 1 : -1;
    const d = `M${a},${yc - h / 2} L${b - dir * r},${yc - h / 2} Q${b},${yc - h / 2} ${b},${yc - h / 2 + r} L${b},${yc + h / 2 - r} Q${b},${yc + h / 2} ${b - dir * r},${yc + h / 2} L${a},${yc + h / 2} Z`;
    const bar = el("path", { d, fill: color(s.colorIdx) }, svg);
    bar.style.cursor = "pointer";
    bar.addEventListener("click", () => selectPoint(q.p.id));
    el("text", { x: b + dir * 6, y: yc + 4, "text-anchor": dir > 0 ? "start" : "end", class: "endlabel" }, svg).textContent = fmt(v);
  });
  el("line", { x1: sx(0), x2: sx(0), y1: M.t, y2: H - M.b, class: "axisline" }, svg);
}

// ---------- table ----------
function tableMetricKeys() { return S.picked.map((m) => flatKey(m)); }

function thumbSvg(thumb, w = 64, h = 40) {
  const ents = Object.entries(thumb || {});
  if (!ents.length) return "";
  const all = ents.flatMap(([, pts]) => pts);
  const xs = all.map((q) => q[0]), ys = all.map((q) => q[1]);
  const [x0, x1, y0, y1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
  const span = Math.max(x1 - x0, y1 - y0, 1);
  const k = (Math.min(w, h) - 6) / span;
  const ox = (w - (x1 - x0) * k) / 2, oy = (h - (y1 - y0) * k) / 2;
  const paths = ents.map(([, pts], i) => `<path stroke="${color(i)}" d="${pts.map((q, j) => `${j ? "L" : "M"}${(ox + (q[0] - x0) * k).toFixed(1)},${(h - oy - (q[1] - y0) * k).toFixed(1)}`).join("")}"/>`).join("");
  return `<svg class="thumb" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}" aria-label="top-down path">${paths}</svg>`;
}

// ---------- frames ----------
function artifactUrl(a) {
  return `/api/artifact/${encodeURIComponent(S.sweep.name)}/${a.split("/").map(encodeURIComponent).join("/")}`;
}

// The frame that tells the story: the first hit if there was one, else the last frame.
function keyFrame(p) {
  const fr = p.frames || [];
  if (!fr.length) return null;
  const hit = ((p.info || {}).frames || []).findIndex((n) => String(n).includes("first hit"));
  return hit >= 0 && hit < fr.length ? fr[hit] : fr[fr.length - 1];
}

function frameImg(a, w, h) {
  return `<img class="frame" src="${artifactUrl(a)}" width="${w}" height="${h}" alt="top-down frame ${esc(a.split("/").pop())}" loading="lazy">`;
}

function thumbCell(p) {
  const k = keyFrame(p);
  return k ? frameImg(k, 96, 60) : thumbSvg(p.thumb);
}

function renderTable() {
  const t = $("#points-table");
  const sw = S.sweep;
  const params = paramDefs().map((p) => p.label);
  const mkeys = tableMetricKeys();
  const showVar = sw.variants.length > 1;
  const showThumb = sw.points.some((p) => Object.keys(p.thumb || {}).length || (p.frames || []).length);
  const cols = [
    { k: "ab", h: "", num: false },
    { k: "id", h: "point" },
    ...(showVar ? [{ k: "variant", h: "variant" }] : []),
    ...params.map((p) => ({ k: "param:" + p, h: p, num: true })),
    { k: "status", h: "checks" },
    ...(showThumb ? [{ k: "thumb", h: "frame" }] : []),
    ...mkeys.map((m) => ({ k: "m:" + m, h: m, num: true })),
  ];
  const val = (p, k) => {
    if (k === "id") return p.id;
    if (k === "variant") return p.variant;
    if (k === "status") return p.ok ? (p.passed === true ? 2 : p.passed === false ? 0 : 1) : -1;
    if (k.startsWith("param:")) return p.params[k.slice(6)];
    if (k.startsWith("m:")) return p.metrics[k.slice(2)];
    return null;
  };
  const rows = [...sw.points].sort((a, b) => {
    const va = val(a, S.sort.key), vb = val(b, S.sort.key);
    if (va === vb) return a.id < b.id ? -1 : 1;
    if (va === undefined || va === null) return 1;
    if (vb === undefined || vb === null) return -1;
    return (va < vb ? -1 : 1) * S.sort.dir;
  });
  t.innerHTML = `<thead><tr>${cols.map((c) => `<th class="${c.num ? "num" : ""}" data-k="${esc(c.k)}">${esc(c.h)}${S.sort.key === c.k ? (S.sort.dir > 0 ? " ▲" : " ▼") : ""}</th>`).join("")}</tr></thead><tbody>${rows
    .map((p) => `<tr data-id="${esc(p.id)}" class="${p.id === S.selected ? "selected" : ""}">${cols
      .map((c) => {
        if (c.k === "ab") return `<td><button class="ab" data-ab="A" aria-pressed="${S.A === p.id}">A</button> <button class="ab" data-ab="B" aria-pressed="${S.B === p.id}">B</button></td>`;
        if (c.k === "status") return `<td>${statusHtml(p)}</td>`;
        if (c.k === "thumb") return `<td>${thumbCell(p)}</td>`;
        const v = val(p, c.k);
        return `<td class="${c.num ? "num" : ""}">${esc(c.k.startsWith("param:") && v === undefined ? "base" : fmt(v))}</td>`;
      })
      .join("")}</tr>`)
    .join("")}</tbody>`;
  t.querySelectorAll("th").forEach((th) => (th.onclick = () => {
    const k = th.dataset.k;
    if (k === "ab" || k === "thumb") return;
    S.sort = { key: k, dir: S.sort.key === k ? -S.sort.dir : 1 };
    renderTable();
  }));
  t.querySelectorAll("tbody tr").forEach((tr) => (tr.onclick = (e) => {
    const ab = e.target.closest("button.ab");
    if (ab) { S[ab.dataset.ab] = tr.dataset.id; renderTable(); renderCompare(); return; }
    selectPoint(tr.dataset.id);
  }));
}

function selectPoint(id) {
  S.selected = id;
  renderCharts();
  renderTable();
  renderDetail();
  $("#detail").scrollIntoView({ behavior: "smooth", block: "nearest" });
}

// ---------- detail + promote ----------
function point(id) { return S.sweep.points.find((p) => p.id === id); }

function renderDetail() {
  const body = $("#detail-body");
  const p = S.selected && point(S.selected);
  if (!p) { body.className = "muted"; body.textContent = "Select a point (table row or chart marker)."; return; }
  body.className = "";
  const params = Object.entries(p.params).map(([k, v]) => `${esc(k)} = <b>${esc(fmt(v))}</b>`).join(", ") || "base values";
  const changed = p.applied.filter((a) => JSON.stringify(a.old) !== JSON.stringify(a.new));
  const files = [...new Set(changed.filter((a) => a.file !== "scenario").map((a) => a.file))];
  const checks = p.checks.map((c) => `<div>${c.passed ? '<span class="status pass"><span class="ic">✓</span></span>' : c.pending ? '<span class="status none">pending</span>' : '<span class="status fail"><span class="ic">✕</span></span>'} <code>${esc(c.name)}</code> ${c.actual ? `<span class="muted">→ ${esc(c.actual)}</span>` : ""}</div>`).join("") || '<span class="muted">none</span>';
  const appliedRows = changed.slice(0, 40).map((a) => `<tr><td><code>${esc(a.file)}</code></td><td><code>${esc(a.pointer)}</code></td><td class="num">${esc(fmt(a.old))}</td><td class="num">${esc(fmt(a.new))}</td></tr>`).join("");
  const infoVal = (v) => {
    if (Array.isArray(v) && v.length && v.every((x) => x && typeof x === "object" && !Array.isArray(x))) {
      // A list of records (pursuit hits): a small table.
      const keys = [...new Set(v.flatMap((x) => Object.keys(x)))];
      return `<div class="table-wrap" style="max-height:220px"><table><thead><tr>${keys.map((k) => `<th>${esc(k)}</th>`).join("")}</tr></thead><tbody>${v.map((x) => `<tr>${keys.map((k) => `<td class="${typeof x[k] === "number" ? "num" : ""}">${esc(typeof x[k] === "object" ? JSON.stringify(x[k]) : fmt(x[k]))}</td>`).join("")}</tr>`).join("")}</tbody></table></div>`;
    }
    if (Array.isArray(v)) return v.length ? v.map((x) => esc(typeof x === "object" ? JSON.stringify(x) : x)).join(", ") : "—";
    if (v && typeof v === "object") return Object.entries(v).map(([k, x]) => `${esc(k)}: ${infoVal(x)}`).join("<br>");
    return esc(v);
  };
  const info = Object.entries(p.info || {}).map(([k, v]) => `<dt>${esc(k)}</dt><dd>${infoVal(v)}</dd>`).join("");
  const arts = p.artifacts.filter((a) => !(p.frames || []).includes(a)).map((a) => `<a href="${artifactUrl(a)}" target="_blank">${esc(a.split("/").pop())}</a>`).join(" · ");
  const frames = (p.frames || []).map((a) => `<a href="${artifactUrl(a)}" target="_blank">${frameImg(a, 240, 150)}</a>`).join("");
  body.innerHTML = `
    <dl class="kv"><dt>point</dt><dd><b>${esc(p.id)}</b> ${S.sweep.variants.length > 1 ? "· " + esc(p.variant) : ""}</dd>
    <dt>params</dt><dd>${params}</dd><dt>checks</dt><dd>${statusHtml(p)}</dd>
    ${p.wall_s ? `<dt>probe time</dt><dd>${esc(p.wall_s)} s${p.reused ? " (cached)" : ""}</dd>` : ""}
    ${p.error ? `<dt>error</dt><dd>${esc(p.error)}</dd>` : ""}</dl>
    ${frames ? `<h3>Frames (top-down)</h3><div class="frames">${frames}</div>` : Object.keys(p.thumb || {}).length ? `<h3>Path (top-down)</h3>${thumbSvg(p.thumb, 220, 140)}` : ""}
    <h3>Checks</h3>${checks}
    <h3>Values this point changes (${changed.length})</h3>
    ${changed.length ? `<div class="table-wrap" style="max-height:220px"><table><thead><tr><th>file</th><th>pointer</th><th class="num">file</th><th class="num">point</th></tr></thead><tbody>${appliedRows}</tbody></table></div>${changed.length > 40 ? `<div class="muted small">…and ${changed.length - 40} more</div>` : ""}` : '<span class="muted">none (base values)</span>'}
    ${info ? `<h3>Info</h3><dl class="kv">${info}</dl>` : ""}
    ${arts ? `<h3>simcli outputs</h3><div class="small">${arts}</div>` : ""}
    <div class="promote">
      <b>Promote these values into the data files</b>
      ${files.length ? `<div class="row">${files.map((f) => `<label class="check"><input type="checkbox" class="pf" value="${esc(f)}" checked> <code>${esc(f)}</code></label>`).join("")}</div>
      <input type="text" id="promote-note" placeholder="Why (goes into docs/TUNING_CHANGELOG.md)">
      <div class="row"><button id="promote-preview">Preview</button><button class="primary" id="promote-go">Promote</button>
      <label class="check"><input type="checkbox" id="promote-force"> even if the file changed since the sweep</label></div>` : '<span class="muted small">Nothing to promote: this point uses the data files\' values' + (changed.some((a) => a.file === "scenario") ? " (scenario overrides are never promoted)" : "") + ".</span>"}
      <div id="promote-result"></div>
    </div>`;
  if (files.length) {
    const send = async (dry) => {
      const chosen = [...body.querySelectorAll(".pf:checked")].map((c) => c.value);
      if (!chosen.length) { $("#promote-result").innerHTML = `<div class="result">Pick at least one file.</div>`; return; }
      if (!dry && !confirm(`Write ${p.id}'s values into ${chosen.join(", ")}?`)) return;
      const r = await api("/api/promote", { sweep: S.sweep.name, point: p.id, note: $("#promote-note").value, files: chosen, dry_run: dry, force: $("#promote-force").checked });
      $("#promote-result").innerHTML = r.ok
        ? `<div class="result">${dry ? "Would write" : "Wrote"} ${esc(r.files.join(", "))}\n${esc(r.log_lines.join("\n"))}${dry ? "" : "\n\nRe-run the sweep so its points compare against the new values."}</div>`
        : `<div class="result">Refused: ${esc(r.error)}</div>`;
    };
    $("#promote-preview").onclick = () => send(true);
    $("#promote-go").onclick = () => send(false);
  }
}

// ---------- compare ----------
function renderCompare() {
  const body = $("#compare-body");
  const a = S.A && point(S.A), b = S.B && point(S.B);
  if (!a || !b) { body.className = "muted"; body.textContent = "Pick A and B in the points table."; return; }
  body.className = "";
  const name = (p) => `${p.id}${S.sweep.variants.length > 1 ? " · " + p.variant : ""}${Object.keys(p.params).length ? " · " + Object.entries(p.params).map(([k, v]) => `${k}=${fmt(v)}`).join(", ") : ""}`;
  const keys = [...new Set([...Object.keys(a.metrics), ...Object.keys(b.metrics)])].sort();
  const rows = keys.map((k) => {
    const va = a.metrics[k], vb = b.metrics[k];
    const d = typeof va === "number" && typeof vb === "number" ? vb - va : null;
    const rel = d !== null && va !== 0 ? (d / Math.abs(va)) * 100 : null;
    return `<tr><td><code>${esc(k)}</code></td><td class="num">${esc(fmt(va))}</td><td class="num">${esc(fmt(vb))}</td><td class="num">${d === null ? "—" : (d > 0 ? "+" : "") + esc(fmt(d))}</td><td class="num">${rel === null ? "—" : (rel > 0 ? "+" : "") + rel.toFixed(1) + " %"}</td></tr>`;
  }).join("");
  body.innerHTML = `<dl class="kv"><dt>A</dt><dd>${esc(name(a))} ${statusHtml(a)}</dd><dt>B</dt><dd>${esc(name(b))} ${statusHtml(b)}</dd></dl>
    <div id="compare-chart"></div>
    <h3>Metrics</h3><div class="table-wrap" style="max-height:360px"><table><thead><tr><th>metric</th><th class="num">A</th><th class="num">B</th><th class="num">B − A</th><th class="num">%</th></tr></thead><tbody>${rows}</tbody></table></div>`;
  // Inner series (barrier speeds) overlay: A vs B for the first picked metric.
  if (a.series && b.series) {
    const m = (S.picked.map((x) => x.split("@")[0]).find((x) => a.series.y[x]) || Object.keys(a.series.y)[0]);
    const wrap = document.createElement("div");
    wrap.className = "chart compact";
    wrap.innerHTML = `<div class="title">${esc(m)} vs closing speed — <span class="muted">A blue, B orange</span></div>`;
    $("#compare-chart").appendChild(wrap);
    const mk = (p, label, idx) => ({ key: label, label, colorIdx: idx, pts: p.series.x.map((x, i) => ({ x, y: p.series.y[m] ? p.series.y[m][i] : null, p })) });
    lineChart(wrap, [mk(a, "A " + a.id, 0), mk(b, "B " + b.id, 1)], "km/h", { noSelect: true });
  }
}

// ---------- data diff ----------
async function initDiff() {
  const files = await api("/api/datafiles");
  const opts = files.map((f) => `<option>${esc(f)}</option>`).join("");
  $("#diff-a").innerHTML = opts;
  $("#diff-b").innerHTML = opts;
  if (files.includes("modes/arcade.toml")) $("#diff-a").value = "modes/arcade.toml";
  if (files.includes("modes/sim.toml")) $("#diff-b").value = "modes/sim.toml";
  $("#diff-btn").onclick = () => runDiff($("#diff-a").value, $("#diff-b").value);
  document.querySelectorAll("[data-diff]").forEach((b) => (b.onclick = () => {
    const [x, y] = b.dataset.diff.split("|");
    $("#diff-a").value = x; $("#diff-b").value = y;
    runDiff(x, y);
  }));
}
async function runDiff(a, b) {
  const box = $("#diff-body");
  box.innerHTML = `<p class="muted">diffing…</p>`;
  const d = await api(`/api/diff?a=${encodeURIComponent(a)}&b=${encodeURIComponent(b)}`);
  if (!d.rows) { box.innerHTML = `<div class="result">${esc(d.error || "diff failed")}</div>`; return; }
  const shown = d.rows.slice(0, 300);
  box.innerHTML = `<p class="muted small"><code>${esc(d.a)}</code> vs <code>${esc(d.b)}</code>: ${d.count} of ${d.compared} fields differ${d.count > shown.length ? ` (first ${shown.length} shown)` : ""}.</p>
    <div class="table-wrap"><table><thead><tr><th>field</th><th class="num">A</th><th class="num">B</th><th class="num">B vs A</th></tr></thead><tbody>${shown
      .map((r) => `<tr><td><code>${esc(r.pointer)}</code></td><td class="num">${esc(fmt(r.a))}</td><td class="num">${esc(fmt(r.b))}</td><td class="num">${r.rel === null || r.rel === undefined ? "—" : (r.rel > 0 ? "+" : "") + (r.rel * 100).toFixed(1) + " %"}</td></tr>`)
      .join("")}</tbody></table></div>`;
}

// ---------- presets / background runs ----------
async function initPresets() {
  const ps = await api("/api/presets");
  $("#preset-select").innerHTML = ps.map((p) => `<option value="${esc(p.name)}" ${p.error ? "disabled" : ""}>${esc(p.name)}${p.points ? ` (${p.points})` : ""}</option>`).join("");
  $("#run-btn").onclick = async () => {
    const r = await api("/api/run", { preset: $("#preset-select").value, force: $("#run-force").checked });
    if (!r.ok) { $("#job-status").textContent = r.error; return; }
    pollJob();
  };
  pollJob(true);
}
async function pollJob(boot = false) {
  const j = await api("/api/jobs");
  const st = $("#job-status");
  $("#run-btn").disabled = !!j.running;
  if (j.running) {
    st.textContent = `running ${j.name}: ${j.done}/${j.total}`;
    setTimeout(pollJob, 600);
  } else if (j.name) {
    st.textContent = j.error ? `${j.name} failed: ${j.error.split("\n")[0]}` : `${j.name} finished`;
    if (boot) S._lastJob = j.name + j.done;
    else if (!j.error && S._lastJob !== j.name + j.done) { S._lastJob = j.name + j.done; await loadSweeps(j.name); }
  }
}

// ---------- boot ----------
$("#sweep-select").onchange = (e) => loadSweep(e.target.value);
window.addEventListener("resize", () => { if (S.sweep) renderCharts(); });
loadSweeps().then(initPresets).then(initDiff);
