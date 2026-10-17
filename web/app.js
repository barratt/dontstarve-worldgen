"use strict";
// Client-side seed search for Don't Starve and Don't Starve Together. The
// WebAssembly generator, with the game data built in (web/build-standalone.ps1),
// runs in one Web Worker per core.

const $ = (id) => document.getElementById(id);

function el(tag, attrs = {}, ...children) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v == null || v === false) continue;
    if (k === "class") e.className = v;
    else if (k.startsWith("on")) e.addEventListener(k.slice(2), v);
    else e.setAttribute(k, v === true ? "" : v);
  }
  for (const c of children.flat()) if (c != null) e.append(c instanceof Node ? c : document.createTextNode(String(c)));
  return e;
}

const prettify = (s) => s.replace(/_/g, " ").toLowerCase().replace(/^./, (c) => c.toUpperCase());

// ---------------------------------------------------------------- workers

// Downloads and compiles the WebAssembly module once; every worker gets the
// compiled module instead of fetching its own copy.
let wasmModule = null;
async function compileModule() {
  if (wasmModule) return wasmModule;
  const res = await fetch("ds_worldgen_web.wasm");
  if (!res.ok) throw new Error(`ds_worldgen_web.wasm: ${res.status}`);
  const total = Number(res.headers.get("content-length")) || 0;
  const reader = res.body.getReader();
  const chunks = [];
  let got = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    chunks.push(value);
    got += value.length;
    $("loading").firstElementChild.textContent = `Loading the generator… ${(got / 1e6).toFixed(1)} MB${total ? ` of ${(total / 1e6).toFixed(1)} MB` : ""}`;
  }
  $("loading").firstElementChild.textContent = "Starting the generator…";
  wasmModule = await WebAssembly.compile(await new Blob(chunks).arrayBuffer());
  return wasmModule;
}

class Pool {
  constructor(n) {
    this.workers = [];
    this.idle = [];
    this.queue = [];
    this.next = 1;
    this.pending = new Map();
    // Resolves to what game data the module has built in.
    this.ready = Promise.all(Array.from({ length: n }, () => this.spawn())).then((infos) => infos[0]);
  }
  spawn() {
    const w = new Worker("worker.js");
    w.onmessage = (e) => {
      const p = this.pending.get(e.data.id);
      if (!p) return;
      this.pending.delete(e.data.id);
      p.resolve(e.data);
      if (p.release) this.release(w);
    };
    this.workers.push(w);
    return this.send(w, { type: "init", module: wasmModule }).then((reply) => {
      this.release(w);
      return reply.info;
    });
  }
  send(w, msg, release = false) {
    const id = this.next++;
    return new Promise((resolve) => {
      this.pending.set(id, { resolve, release });
      w.postMessage({ ...msg, id });
    });
  }
  release(w) {
    const job = this.queue.shift();
    if (job) job(w);
    else this.idle.push(w);
  }
  run(msg) {
    return new Promise((resolve) => {
      const start = (w) => this.send(w, msg, true).then(resolve);
      const w = this.idle.pop();
      if (w) start(w);
      else this.queue.push(start);
    });
  }
  get size() { return this.workers.length; }
  terminate() { this.workers.forEach((w) => w.terminate()); }
}

let pool = null;
let embedded = null;
const defaultWorkers = () => Math.max(1, Math.min(8, (navigator.hardwareConcurrency || 4) - 1));

// ---------------------------------------------------------------- game and world

const DLC_NAMES = { base: "Base game", rog: "Reign of Giants", sw: "Shipwrecked", hamlet: "Hamlet" };
const LEVELS = {
  base: [["survival", "Survival"], ["cave", "Caves / Ruins"], ["adventure", "Adventure"]],
  rog: [["survival", "Survival"], ["cave", "Caves / Ruins"], ["adventure", "Adventure"]],
  sw: [["shipwrecked", "Shipwrecked"], ["volcano", "Volcano"], ["survival", "Survival (RoG-compatible)"]],
  hamlet: [["porkland", "Hamlet"], ["survival", "Survival"], ["shipwrecked", "Shipwrecked"], ["volcano", "Volcano"]],
};

function fillSelect(sel, options, value) {
  sel.innerHTML = "";
  for (const [v, label] of options) sel.add(new Option(label, v, false, v === value));
}

const game = () => $("game").value;

function worldOptions() {
  if (game() === "dst") {
    const opts = { preset: $("preset").value };
    // Large is the game's default: leave it out so default worlds keep their cache key.
    if ($("world-size").value !== "large") opts.world_size = $("world-size").value;
    // Likewise Windows, the default.
    if ($("platform").value !== "windows") opts.platform = $("platform").value;
    return opts;
  }
  const opts = { dlc: $("dlc").value, level: $("level").value };
  if (!$("current-level-label").hidden) opts.current_level = Number($("current-level").value);
  return opts;
}
const worldKey = () => `${game()}|${JSON.stringify(worldOptions())}`;
const request = (seed, extra = {}) => ({ game: game(), root: embedded[game()].root, seed, options: worldOptions(), ...extra });

function setupGame() {
  const ds = game() === "ds";
  $("dlc-label").hidden = !ds;
  $("level-label").hidden = !ds;
  $("preset-label").hidden = ds;
  $("world-size-label").hidden = ds;
  $("platform-label").hidden = ds;
  if (ds) {
    const dlcs = ["base", ...embedded.ds.dlcs];
    fillSelect($("dlc"), dlcs.map((d) => [d, DLC_NAMES[d]]), dlcs.includes("rog") ? "rog" : dlcs[0]);
    updateLevels();
  } else {
    $("current-level-label").hidden = true;
  }
}
function updateLevels() {
  fillSelect($("level"), LEVELS[$("dlc").value], LEVELS[$("dlc").value][0][0]);
  updateCurrentLevel();
}
function updateCurrentLevel() {
  const level = $("level").value;
  $("current-level-label").hidden = game() !== "ds" || (level !== "cave" && level !== "adventure");
  if (level === "cave") fillSelect($("current-level"), [["1", "Caves"], ["2", "Ruins"]], "1");
  if (level === "adventure") fillSelect($("current-level"), [1, 2, 3, 4, 5].map((n) => [String(n), `Chapter ${n}`]), "1");
}

let lastGame = null;
async function worldChanged() {
  if (game() !== lastGame) {
    // Prefabs differ between the games: start over.
    if (lastGame !== null) {
      filters = [];
      editing = null;
      renderPills();
    }
    lastGame = game();
    await loadAssets(game());
  }
  applyBrand();
  resetResults();
  loadCatalogue();
  warmUp();
}

$("game").addEventListener("change", () => { setupGame(); worldChanged(); });
$("dlc").addEventListener("change", () => { updateLevels(); worldChanged(); });
$("level").addEventListener("change", () => { updateCurrentLevel(); worldChanged(); });
$("current-level").addEventListener("change", worldChanged);
$("preset").addEventListener("change", worldChanged);
$("world-size").addEventListener("change", worldChanged);
$("platform").addEventListener("change", worldChanged);

// ---------------------------------------------------------------- assets: icons and names

const assetsByGame = {};
let assets = { icons: {}, names: {}, notable: new Set() };
const images = new Map();

async function loadAssets(kind) {
  if (!assetsByGame[kind]) {
    const reply = await pool.run({ type: "assets", game: kind });
    const a = reply.assets && !reply.assets.error ? reply.assets : { icons: {}, names: {}, notable: [] };
    assetsByGame[kind] = { icons: a.icons, names: a.names, notable: new Set(a.notable), brand: a.brand || {} };
  }
  assets = assetsByGame[kind];
}

// The games' own logo, buttons and art, from the user's install.
function applyBrand() {
  const b = assets.brand || {};
  const root = document.documentElement;
  const url = (u) => (u ? `url("${u}")` : "none");
  root.style.setProperty("--btn-img", url(b.button));
  root.style.setProperty("--btn-over-img", url(b.button_over || b.button));
  root.style.setProperty("--bg-img", url(b.background));
  root.classList.toggle("textured", !!b.button);
  const logo = game() === "dst" ? b["title-dst"] : $("dlc").value === "rog" ? b["title-rog"] || b["title-ds"] : b["title-ds"];
  $("logo").hidden = !logo;
  if (logo) $("logo").src = logo;
  $("title-text").textContent = logo ? "World Search" : "Don't Starve world search";
}

const displayName = (prefab) => assets.names[prefab] || prettify(prefab);

function iconImage(prefab) {
  const src = assets.icons[prefab];
  if (!src) return null;
  let img = images.get(src);
  if (!img) {
    img = new Image();
    img.onload = () => scheduleDraw();
    img.src = src;
    images.set(src, img);
  }
  return img;
}
const iconEl = (prefab, cls = "") =>
  assets.icons[prefab] ? el("img", { src: assets.icons[prefab], alt: "", class: cls }) : el("span", { class: `noicon ${cls}` }, prefab[0].toUpperCase());

// ---------------------------------------------------------------- catalogue: what can be filtered on

// Everything seen in generated worlds of the current world type, kept per
// browser so the picker is complete without rescanning.
let catalogue = null;
function emptyCatalogue() { return { worlds: 0, prefabs: {}, tasks: {}, tiles: {} }; }
function loadCatalogue() {
  try { catalogue = JSON.parse(localStorage.getItem("cat:" + worldKey())) || emptyCatalogue(); }
  catch { catalogue = emptyCatalogue(); }
}
let saveTimer = 0;
function addToCatalogue(s) {
  catalogue.worlds++;
  for (const [p, n] of Object.entries(s.prefabs || {})) if (n > 0) catalogue.prefabs[p] = (catalogue.prefabs[p] || 0) + 1;
  for (const t of s.task_names || []) catalogue.tasks[t] = (catalogue.tasks[t] || 0) + 1;
  for (const t of Object.keys(s.tiles || {})) catalogue.tiles[t] = (catalogue.tiles[t] || 0) + 1;
  clearTimeout(saveTimer);
  const key = "cat:" + worldKey(), data = catalogue;
  saveTimer = setTimeout(() => { try { localStorage.setItem(key, JSON.stringify(data)); } catch {} }, 500);
  if ($("picker").open) renderPicker();
}

// A few worlds generated in the background list what the world type
// contains (and warm up every worker).
let warmKey = null;
async function warmUp() {
  const key = worldKey();
  if (warmKey === key || catalogue.worlds >= pool.size) return;
  warmKey = key;
  await Promise.all(Array.from({ length: pool.size }, async (_, i) => {
    const { result } = await pool.run({ type: "generate", request: request(1000000 + i) });
    if (worldKey() === key && result && !result.error) addToCatalogue(result.summary);
  }));
}

// ---------------------------------------------------------------- filters

let filters = [];
let editing = null;
let nextId = 1;

const STATS = { land_tiles: "Land tiles", nodes: "Rooms", roads: "Roads" };

function newFilter(kind, key) {
  const f = { id: nextId++, kind, key, want: true, mode: "min", n: 1, within: null };
  if (kind === "tile") f.n = 500;
  if (kind === "stat") f.n = { land_tiles: 30000, nodes: 150, roads: 10, width: 350 }[key] || 1;
  return f;
}

function filterLabel(f) {
  if (f.kind === "prefab") return displayName(f.key);
  if (f.kind === "task") return f.key;
  if (f.kind === "tile") return prettify(f.key);
  return STATS[f.key] || f.key;
}

const modeText = (f, unit = "") => ({ min: `at least ${f.n}`, max: `at most ${f.n}`, exact: `exactly ${f.n}` })[f.mode] + unit;

function filterSub(f) {
  if (f.kind === "prefab") {
    const where = f.within ? ` within ${f.within} tiles of spawn` : "";
    return f.want ? modeText(f) + where : "none" + where;
  }
  if (f.kind === "task") return f.want ? "in the world" : "not in the world";
  if (f.kind === "tile") return modeText(f, " tiles");
  return modeText(f);
}

function filterColumn(f) {
  return f.kind === "prefab" && f.within ? `${filterLabel(f)} ≤${f.within}t` : filterLabel(f);
}

function renderPills() {
  const box = $("pills");
  box.querySelectorAll(".pill").forEach((p) => p.remove());
  for (const f of filters) {
    const icon = f.kind === "prefab" ? iconEl(f.key) : el("span", { class: "noicon" }, f.kind === "task" ? "T" : f.kind === "tile" ? "G" : "#");
    const pill = el("span", { class: `pill${f.want ? "" : " avoid"}${editing === f.id ? " active" : ""}` },
      el("button", { class: "pill-x", title: "Edit", style: "padding:0;display:inline-flex;gap:6px;align-items:center;color:inherit",
        onclick: () => { editing = editing === f.id ? null : f.id; renderPills(); } },
        icon, el("span", { class: "pill-text" }, el("span", {}, filterLabel(f)), el("small", {}, filterSub(f)))),
      el("button", { class: "pill-x", title: "Remove", onclick: () => removeFilter(f.id) }, "✕"));
    box.insertBefore(pill, $("add-filter"));
  }
  renderEditor();
}

function removeFilter(id) {
  filters = filters.filter((f) => f.id !== id);
  if (editing === id) editing = null;
  renderPills();
  rescore();
}

function changed() {
  renderPills();
  rescore();
}

function seg(options, value, onpick) {
  return el("span", { class: "seg" }, options.map(([v, label]) => el("button", { class: v === value ? "on" : "", onclick: () => onpick(v) }, label)));
}

function renderEditor() {
  const box = $("pill-editor");
  const f = filters.find((x) => x.id === editing);
  box.hidden = !f;
  box.innerHTML = "";
  if (!f) return;
  const parts = [el("strong", {}, filterLabel(f))];
  if (f.kind === "prefab" || f.kind === "task") {
    parts.push(seg([[true, "Want"], [false, "Avoid"]], f.want, (v) => { f.want = v; changed(); }));
  }
  const showCount = f.kind === "tile" || f.kind === "stat" || (f.kind === "prefab" && f.want);
  if (showCount) {
    const mode = el("select", { onchange: (e) => { f.mode = e.target.value; changed(); } });
    fillSelect(mode, [["min", "at least"], ["max", "at most"], ["exact", "exactly"]], f.mode);
    const n = el("input", { type: "number", min: "0", value: String(f.n), onchange: (e) => { f.n = Math.max(0, Number(e.target.value) || 0); changed(); } });
    parts.push(el("label", { class: "inline" }, mode, n));
  }
  if (f.kind === "prefab" && assets.notable.has(f.key)) {
    const within = el("input", { type: "number", min: "1", placeholder: "anywhere", value: f.within ? String(f.within) : "",
      onchange: (e) => { const v = Number(e.target.value); f.within = v > 0 ? v : null; changed(); } });
    parts.push(el("label", { class: "inline" }, "within", within, "tiles of spawn"));
  }
  parts.push(el("button", { onclick: () => removeFilter(f.id) }, "Remove"));
  parts.push(el("button", { class: "primary", onclick: () => { editing = null; renderPills(); } }, "Done"));
  box.append(...parts);
}

// How far beyond the asked-for amount a value goes: 0 at the limit, +1 for
// each doubling ("at least 500": 1000 scores 1, 2000 scores 2; "at most":
// the same towards zero). Logarithmic, so no one filter swamps the others.
function margin(f, v) {
  if (f.mode === "min") return Math.log2(Math.max(v, 0.5) / Math.max(f.n, 0.5));
  if (f.mode === "max") return Math.log2(Math.max(f.n, 0.5) / Math.max(v, 0.5));
  return 0;
}

// Whether a world meets one filter, the value to show, and a bonus for how
// well it meets it, used to rank worlds that meet the same number.
function evaluate(s, f) {
  const cmp = (v) => (f.mode === "min" ? v >= f.n : f.mode === "max" ? v <= f.n : v === f.n);
  if (f.kind === "prefab") {
    if (f.within) {
      const near = ((s.notable || {})[f.key] || []).filter((p) => p.distance <= f.within * 4);
      const nearest = near.length ? Math.min(...near.map((p) => p.distance)) / 4 : null;
      const ok = f.want ? cmp(near.length) : near.length === 0;
      // Closer to spawn is better (0..1), plus extra instances in range.
      const bonus = f.want && nearest != null ? 1 - nearest / f.within + Math.max(0, margin(f, near.length)) : 0;
      return { ok, value: near.length ? `${near.length} · ${Math.round(nearest)}t` : "0", bonus };
    }
    const count = (s.prefabs || {})[f.key] || 0;
    const ok = f.want ? cmp(count) : count === 0;
    return { ok, value: count, bonus: f.want && ok ? Math.max(0, margin(f, count)) : 0 };
  }
  if (f.kind === "task") {
    const present = (s.task_names || []).includes(f.key);
    return { ok: f.want ? present : !present, value: present ? "yes" : "no", bonus: 0 };
  }
  const v = f.kind === "tile" ? (s.tiles || {})[f.key] || 0 : (s.map || {})[f.key] || 0;
  const ok = cmp(v);
  return { ok, value: v, bonus: ok ? Math.max(0, margin(f, v)) : 0 };
}

function scoreWorld(s) {
  const results = filters.map((f) => evaluate(s, f));
  const satisfied = results.filter((r) => r.ok).length;
  const score = results.reduce((a, r) => a + (r.ok ? 1 + r.bonus : 0), 0);
  return { satisfied, total: filters.length, score, results };
}

// ---------------------------------------------------------------- picker modal

const TABS = [["poi", "Points of interest"], ["objects", "Everything"], ["tasks", "Biomes"], ["ground", "Ground"], ["map", "Map"]];
let pickerTab = "poi";

function pickerItems(tab) {
  const seen = catalogue.worlds || 0;
  const pct = (n) => (seen ? `${Math.round((100 * n) / seen)}% of worlds` : "");
  if (tab === "poi" || tab === "objects") {
    const names = new Set(Object.keys(catalogue.prefabs));
    if (tab === "poi") for (const n of assets.notable) names.add(n);
    return [...names]
      .filter((p) => (tab === "poi" ? assets.notable.has(p) : true))
      .map((p) => ({ kind: "prefab", key: p, label: displayName(p), id: p, tag: catalogue.prefabs[p] ? pct(catalogue.prefabs[p]) : "not seen yet", icon: p }))
      .sort((a, b) => a.label.localeCompare(b.label));
  }
  if (tab === "tasks") return Object.entries(catalogue.tasks).map(([t, n]) => ({ kind: "task", key: t, label: t, tag: pct(n) })).sort((a, b) => a.label.localeCompare(b.label));
  if (tab === "ground") return Object.entries(catalogue.tiles).map(([t, n]) => ({ kind: "tile", key: t, label: prettify(t), id: t, tag: pct(n) })).sort((a, b) => a.label.localeCompare(b.label));
  return Object.entries(STATS).map(([k, label]) => ({ kind: "stat", key: k, label, tag: "" }));
}

function renderPicker() {
  const q = $("picker-search").value.trim().toLowerCase();
  const tabs = $("picker-tabs");
  tabs.innerHTML = "";
  for (const [t, label] of TABS) {
    tabs.append(el("button", { class: `tab${t === pickerTab && !q ? " active" : ""}`, onclick: () => { pickerTab = t; $("picker-search").value = ""; renderPicker(); } }, label));
  }
  const list = $("picker-list");
  list.innerHTML = "";
  const groups = q ? TABS.filter(([t]) => t !== "poi") : TABS.filter(([t]) => t === pickerTab);
  let shown = 0;
  for (const [t, label] of groups) {
    const items = pickerItems(t).filter((it) => !q || it.label.toLowerCase().includes(q) || (it.id || "").includes(q));
    if (!items.length) continue;
    if (q) list.append(el("div", { class: "picker-group" }, label));
    for (const it of items.slice(0, q ? 60 : 1000)) {
      const added = filters.some((f) => f.kind === it.kind && f.key === it.key);
      list.append(el("button", { class: `picker-item${added ? " added" : ""}`, onclick: () => pick(it) },
        it.icon ? iconEl(it.icon) : el("span", { class: "noicon" }, it.kind === "task" ? "T" : it.kind === "tile" ? "G" : "#"),
        el("span", {}, it.label, it.id && it.id !== it.label ? el("span", { class: "id" }, it.id) : null),
        el("span", { class: "tag" }, added ? "✓ added" : it.tag)));
      shown++;
    }
  }
  if (!shown) list.append(el("p", { class: "muted" }, catalogue.worlds ? "Nothing matches." : "Scanning a few worlds to list what this world type contains…"));
  $("picker-note").textContent = catalogue.worlds
    ? `Listed from ${catalogue.worlds} generated worlds of this type; the list grows as you search.`
    : "";
}

function pick(it) {
  const f = newFilter(it.kind, it.key);
  filters.push(f);
  editing = f.id;
  $("picker").close();
  changed();
}

$("add-filter").addEventListener("click", () => {
  $("picker-search").value = "";
  renderPicker();
  $("picker").showModal();
  $("picker-search").focus();
});
$("picker-close").addEventListener("click", () => $("picker").close());
$("picker-search").addEventListener("input", renderPicker);
$("picker").addEventListener("click", (e) => { if (e.target === $("picker")) $("picker").close(); });

// ---------------------------------------------------------------- search and results

let rows = [];          // { seed, summary? , s: score }
let matches = [];       // full summaries of complete matches, for download
let searching = null;
let currentSeed = null;
const MAX_KEEP = 20000;

function resetResults() {
  rows = [];
  matches = [];
  $("results").hidden = true;
  $("world").hidden = true;
  $("search-status").textContent = "";
  $("progress-bar").style.width = "0";
  $("download").disabled = true;
}

// Rows keep only what the tables need: per-filter values are recomputed
// from a compact copy of the summary when filters change.
function compact(s) {
  return { seed: s.seed, prefabs: s.prefabs, task_names: s.task_names, tiles: s.tiles, map: s.map,
    notable: Object.fromEntries(Object.entries(s.notable || {}).map(([k, v]) => [k, v.map((p) => ({ distance: p.distance }))])) };
}

function rescore() {
  for (const r of rows) r.s = scoreWorld(r.c);
  scheduleTables();
}

let tableTimer = 0;
function scheduleTables() {
  if (tableTimer) return;
  tableTimer = setTimeout(() => { tableTimer = 0; renderTables(); }, 250);
}

function header(table, cols) {
  table.tHead.innerHTML = "";
  table.tHead.append(el("tr", {}, cols.map(([label, num]) => el("th", { class: num ? "num" : "" }, label))));
}

function valueCells(r) {
  return r.s.results.map((res) => el("td", { class: `num ${filters.length ? (res.ok ? "ok" : "bad") : ""}` }, String(res.value)));
}

function rowEl(r, lead, score = false) {
  const full = r.s.total > 0 && r.s.satisfied === r.s.total;
  const tr = el("tr", { class: r.seed === currentSeed ? "current" : "", title: "Show this world", onclick: () => showWorld(r.seed) },
    lead, el("td", { class: "num" }, r.seed),
    el("td", {}, el("span", { class: `badge${full ? " full" : ""}` }, r.s.total ? `${r.s.satisfied}/${r.s.total}` : "–")),
    score ? el("td", { class: "num", title: "One point per filter met, plus how far beyond each it goes" }, r.s.score.toFixed(2)) : null,
    valueCells(r));
  return tr;
}

function renderTables() {
  $("results").hidden = rows.length === 0;
  const cols = filters.map((f) => [filterColumn(f), true]);
  // Leaderboard: most filters met, then how well.
  const lb = $("leaderboard");
  header(lb, [["#", true], ["Seed", true], ["Match", false], ["Score", true], ...cols]);
  const best = [...rows].sort(byRank).slice(0, 50);
  lb.tBodies[0].replaceChildren(...best.map((r, i) => rowEl(r, el("td", { class: "num rank" }, i + 1), true)));
  // All results, by seed.
  const all = $("all");
  header(all, [["Seed", true], ["Match", false], ...cols]);
  const only = $("only-matches").checked;
  const list = rows.filter((r) => !only || (r.s.total > 0 && r.s.satisfied === r.s.total)).sort((a, b) => a.seed - b.seed);
  const shown = list.slice(0, 500);
  all.tBodies[0].replaceChildren(...shown.map((r) => rowEl(r, null)));
  $("all-note").textContent = (list.length > shown.length ? `Showing the first ${shown.length} of ${list.length} (by seed).` : `${list.length} worlds.`) +
    (pruned ? ` ${pruned} lower-ranked worlds were dropped to save memory.` : "");
}
$("only-matches").addEventListener("change", renderTables);

// Seeds in order, or distinct random seeds from the whole range the games
// use (0 to 2^31 - 1), optionally until stopped.
function updateSeedMode() {
  const random = $("seed-mode").value === "random";
  $("range-inputs").hidden = random;
  $("random-inputs").hidden = !random;
  $("count").disabled = $("endless").checked;
}
$("seed-mode").addEventListener("change", updateSeedMode);
$("endless").addEventListener("change", updateSeedMode);

function seedSource() {
  if ($("seed-mode").value === "range") {
    const from = Number($("from").value), to = Number($("to").value);
    if (!Number.isInteger(from) || !Number.isInteger(to) || from < 0 || to < from || to > 4294967295) {
      throw new Error("Enter a seed range like 1 to 1000.");
    }
    let next = from;
    return { total: to - from + 1, next: () => (next <= to ? next++ : null) };
  }
  const endless = $("endless").checked;
  const count = Number($("count").value);
  if (!endless && (!Number.isInteger(count) || count < 1)) throw new Error("Enter how many random seeds to try.");
  const seen = new Set();
  const buf = new Uint32Array(64);
  let at = buf.length;
  const random = () => {
    if (at === buf.length) {
      crypto.getRandomValues(buf);
      at = 0;
    }
    return buf[at++] & 0x7fffffff;
  };
  let issued = 0;
  return {
    total: endless ? Infinity : count,
    next: () => {
      if (!endless && issued >= count) return null;
      let seed = random();
      while (seen.has(seed)) seed = random();
      seen.add(seed);
      issued++;
      return seed;
    },
  };
}

// Keeps memory bounded on long searches: past MAX_KEEP rows, only the
// best-ranked ones are kept (the leaderboard stays exact).
const byRank = (a, b) => b.s.satisfied - a.s.satisfied || b.s.score - a.s.score || a.seed - b.seed;
let pruned = 0;
function keepRow(r) {
  rows.push(r);
  if (rows.length > MAX_KEEP) {
    rows.sort(byRank);
    pruned += rows.length - Math.floor(MAX_KEEP * 0.75);
    rows.length = Math.floor(MAX_KEEP * 0.75);
  }
}

$("search").addEventListener("click", async () => {
  let source;
  try {
    source = seedSource();
  } catch (e) {
    $("search-status").textContent = e.message;
    return;
  }
  resetResults();
  pruned = 0;
  const key = worldKey();
  const run = { stop: false };
  searching = run;
  $("search").disabled = true;
  $("stop").disabled = false;
  const total = source.total;
  let done = 0, failed = 0;
  const started = performance.now();
  const status = () => {
    const rate = done / ((performance.now() - started) / 1000);
    const full = rows.filter((r) => r.s.total > 0 && r.s.satisfied === r.s.total).length;
    const of = Number.isFinite(total) ? ` / ${total}` : "";
    $("search-status").textContent = `${done}${of} seeds · ${filters.length ? `${full} full matches · ` : ""}${failed ? `${failed} failed · ` : ""}${rate.toFixed(1)} worlds/s`;
    $("progress-bar").style.width = Number.isFinite(total) ? `${(100 * done) / total}%` : "100%";
  };
  const worker = async () => {
    for (let seed = source.next(); !run.stop && seed !== null; seed = run.stop ? null : source.next()) {
      const { result } = await pool.run({ type: "generate", request: request(seed) });
      if (run.stop || worldKey() !== key) return;
      done++;
      if (!result || result.error) failed++;
      else {
        addToCatalogue(result.summary);
        const r = { seed, c: compact(result.summary) };
        r.s = scoreWorld(r.c);
        keepRow(r);
        if (r.s.total > 0 && r.s.satisfied === r.s.total && matches.length < MAX_KEEP) matches.push(result.summary);
        scheduleTables();
      }
      status();
    }
  };
  await Promise.all(Array.from({ length: pool.size }, worker));
  status();
  $("search-status").textContent += run.stop ? " · stopped" : " · done";
  $("search").disabled = false;
  $("stop").disabled = true;
  $("download").disabled = matches.length === 0 && rows.length === 0;
  searching = null;
  renderTables();
});
$("stop").addEventListener("click", () => { if (searching) searching.stop = true; });

function download(name, text, type) {
  const url = URL.createObjectURL(new Blob([text], { type }));
  const a = el("a", { href: url, download: name });
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
$("download").addEventListener("click", () => {
  const list = matches.length ? matches : rows.map((r) => r.c);
  download("worlds.jsonl", list.map((s) => JSON.stringify(s)).join("\n") + "\n", "application/x-ndjson");
});

// ---------------------------------------------------------------- world map

const TILE_COLOURS = {
  IMPASSABLE: "#0d0b0a", INVALID: "#000000", ROAD: "#b8935f", ROCKY: "#8e8778", DIRT: "#9c7a52", SAVANNA: "#c8a447",
  GRASS: "#6e9a3c", FOREST: "#3c6630", MARSH: "#5d4d6e", WEB: "#b9b4ad", WOODFLOOR: "#8b5a33", CARPET: "#8e3a4c",
  CHECKER: "#d8d4cc", CAVE: "#6a5c4c", FUNGUS: "#4d7290", FUNGUSRED: "#9a4b45", FUNGUSGREEN: "#5d8a4f", SINKHOLE: "#62804c",
  UNDERROCK: "#5b5650", MUD: "#6c5233", BRICK: "#8e7462", BRICK_GLOW: "#a88a70", TILES: "#7b7b8c", TILES_GLOW: "#9a9ab0",
  TRIM: "#6e6a7c", TRIM_GLOW: "#8f8aa2", DECIDUOUS: "#b4622c", DESERT_DIRT: "#d59b56", PEBBLEBEACH: "#b9ae96",
  METEOR: "#6f707b", SHELLBEACH: "#e6d9bc", ARCHIVE: "#7d8a8a", FUNGUSMOON: "#6b8aa6", LUNAR_MARSH: "#6b6b8e",
  MONKEY_GROUND: "#a08c55", MONKEY_DOCK: "#7a5a3a", BEACH: "#e5d6a0", JUNGLE: "#2f7a37", SWAMP: "#4f5d3f",
  TIDALMARSH: "#58704f", MAGMAFIELD: "#6b3a2a", ASH: "#7d7a74", VOLCANO: "#4f3a33", VOLCANO_ROCK: "#5e4a43",
  VOLCANO_LAVA: "#d65a1e", MEADOW: "#8fb04a", MANGROVE: "#3e6c63", RAINFOREST: "#3d7a3c", DEEPRAINFOREST: "#2a5a2d",
  GASJUNGLE: "#5e6a3a", PLAINS: "#b6b05a", PAINTED: "#b07a5a", FIELDS: "#a8a04e", SUBURB: "#9a8f7c",
  FOUNDATION: "#8a8a8a", COBBLEROAD: "#9e9282", LAWN: "#7aa34a", BATTLEGROUND: "#6e5d4a", PIGRUINS: "#7d7462",
};
function hashColour(name, sat = 30, light = 45) {
  let h = 0;
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return `hsl(${h % 360} ${sat}% ${light}%)`;
}
function tileColour(name) {
  if (TILE_COLOURS[name]) return TILE_COLOURS[name];
  if (/OCEAN|WATER|SHALLOW|DEEP|CORAL|LILYPOND/.test(name)) {
    return { OCEAN_COASTAL: "#4b8eb2", OCEAN_COASTAL_SHORE: "#6aa7c6", OCEAN_SWELL: "#2f6f9a", OCEAN_ROUGH: "#24587e",
      OCEAN_BRINEPOOL: "#3d808b", OCEAN_HAZARDOUS: "#1f4663", OCEAN_WATERLOG: "#3f705a", OCEAN_SHALLOW: "#5aa0c0",
      OCEAN_MEDIUM: "#3a7aa6", OCEAN_DEEP: "#24527c", OCEAN_CORAL: "#4aa0a0" }[name] || "#36729c";
  }
  return hashColour(name);
}

let world = null;      // { summary, savedata, tile_names, positions, tiles: canvas, view }
let shownPrefabs = new Set();
const viewState = { scale: 1, ox: 0, oy: 0, heading: 45 };
let markers = [];

function decodeTiles(savedata, w, h, dst) {
  const m = /\btiles="([A-Za-z0-9+/=]+)"/.exec(savedata);
  if (!m) return null;
  const bin = atob(m[1]);
  if (bin.length < 9 + w * h * 2 || bin.slice(0, 4) !== "VRSN") return null;
  const ids = new Uint16Array(w * h);
  for (let i = 0; i < w * h; i++) {
    const a = bin.charCodeAt(9 + 2 * i), b = bin.charCodeAt(10 + 2 * i);
    ids[i] = dst ? a | (b << 8) : a;
  }
  return ids;
}

// The tile layer, one pixel per tile, and the land's bounding box.
function tileLayer(r) {
  const { width: w, height: h } = r.summary.map;
  const ids = decodeTiles(r.savedata, w, h, r.summary.dlc === "dst");
  const canvas = document.createElement("canvas");
  canvas.width = w;
  canvas.height = h;
  let bounds = { x: 0, y: 0, w, h };
  if (ids) {
    const ctx = canvas.getContext("2d");
    const img = ctx.createImageData(w, h);
    const probe = document.createElement("canvas").getContext("2d");
    const cache = new Map();
    const rgb = (id) => {
      if (!cache.has(id)) {
        probe.fillStyle = tileColour(r.tile_names[id] || `tile_${id}`);
        probe.fillRect(0, 0, 1, 1);
        cache.set(id, probe.getImageData(0, 0, 1, 1).data);
      }
      return cache.get(id);
    };
    let x0 = w, y0 = h, x1 = -1, y1 = -1;
    for (let i = 0; i < w * h; i++) {
      const c = rgb(ids[i]);
      img.data.set([c[0], c[1], c[2], 255], i * 4);
      const name = r.tile_names[ids[i]];
      if (name && name !== "IMPASSABLE" && name !== "INVALID") {
        const x = i % w, y = (i / w) | 0;
        if (x < x0) x0 = x; if (x > x1) x1 = x; if (y < y0) y0 = y; if (y > y1) y1 = y;
      }
    }
    ctx.putImageData(img, 0, 0);
    if (x1 >= 0) bounds = { x: Math.max(0, x0 - 4), y: Math.max(0, y0 - 4), w: Math.min(w, x1 + 5) - Math.max(0, x0 - 4), h: Math.min(h, y1 + 5) - Math.max(0, y0 - 4) };
  }
  return { canvas, bounds };
}

// Map view in the game's own orientation. The game's camera at heading h
// looks along world (-cos h, -sin h) (cameras/followcamera.lua), so on
// screen, right = world (-sin h, cos h) and down = world (cos h, sin h).
// Both games start at heading 45, where the map shows north up (DST's
// compass reads heading - 45), the square world drawn as a diamond.
// u, v are world x, z in tiles (one tile is 4 units); this mapping is its
// own inverse.
const toTile = (v, size) => v / 4 + size / 2;

function axes() {
  const h = (viewState.heading * Math.PI) / 180;
  return { sn: Math.sin(h), cs: Math.cos(h) };
}

// Tiles (u, v) from the world centre -> screen pixels.
function project(u, v) {
  const { sn, cs } = axes(), s = viewState.scale;
  return [viewState.ox + s * (-sn * u + cs * v), viewState.oy + s * (cs * u + sn * v)];
}

// Screen pixels -> tiles (u, v) from the world centre.
function unproject(X, Y) {
  const { sn, cs } = axes(), s = viewState.scale;
  const dx = (X - viewState.ox) / s, dy = (Y - viewState.oy) / s;
  return [-sn * dx + cs * dy, cs * dx + sn * dy];
}

function fitView() {
  const wrap = $("map-wrap");
  const b = world.layer.bounds;
  const { width: w, height: h } = world.summary.map;
  const { sn, cs } = axes();
  // Extent of the land's bounding box once rotated.
  let x0 = Infinity, x1 = -Infinity, y0 = Infinity, y1 = -Infinity;
  for (const [tx, ty] of [[b.x, b.y], [b.x + b.w, b.y], [b.x, b.y + b.h], [b.x + b.w, b.y + b.h]]) {
    const u = tx - w / 2, v = ty - h / 2;
    const X = -sn * u + cs * v, Y = cs * u + sn * v;
    x0 = Math.min(x0, X); x1 = Math.max(x1, X); y0 = Math.min(y0, Y); y1 = Math.max(y1, Y);
  }
  viewState.scale = Math.min(wrap.clientWidth / (x1 - x0), wrap.clientHeight / (y1 - y0));
  viewState.ox = wrap.clientWidth / 2 - ((x0 + x1) / 2) * viewState.scale;
  viewState.oy = wrap.clientHeight / 2 - ((y0 + y1) / 2) * viewState.scale;
  scheduleDraw();
}

let drawPending = false;
function scheduleDraw() {
  if (drawPending || !world) return;
  drawPending = true;
  requestAnimationFrame(() => { drawPending = false; drawMap(); });
}

// Which world direction each game's compass calls north: DST's compass reads
// camera heading - 45, DS's reads the heading itself.
function northHeading() {
  return world && world.summary.dlc === "dst" ? 45 : 0;
}

function drawMap() {
  const wrap = $("map-wrap"), canvas = $("map");
  const dpr = window.devicePixelRatio || 1;
  const W = wrap.clientWidth, H = wrap.clientHeight;
  canvas.width = W * dpr;
  canvas.height = H * dpr;
  canvas.style.width = W + "px";
  canvas.style.height = H + "px";
  const ctx = canvas.getContext("2d");
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, W, H);
  const { width: w, height: h } = world.summary.map;
  const { sn, cs } = axes(), s = viewState.scale;
  // The tile layer, one pixel per tile, through the camera transform.
  ctx.save();
  ctx.setTransform(dpr * -sn * s, dpr * cs * s, dpr * cs * s, dpr * sn * s, dpr * viewState.ox, dpr * viewState.oy);
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(world.layer.canvas, -w / 2, -h / 2);
  ctx.restore();
  const at = (x, z) => project(x / 4, z / 4);
  const size = Math.max(14, Math.min(34, s * 4));
  markers = [];
  for (const prefab of shownPrefabs) {
    const points = world.positions[prefab] || [];
    const img = iconImage(prefab);
    for (const [x, z] of points) {
      const [cx, cy] = at(x, z);
      if (cx < -size || cy < -size || cx > W + size || cy > H + size) continue;
      if (img && img.complete && img.naturalWidth) ctx.drawImage(img, cx - size / 2, cy - size / 2, size, size);
      else {
        ctx.beginPath();
        ctx.arc(cx, cy, Math.max(3, size / 5), 0, Math.PI * 2);
        ctx.fillStyle = hashColour(prefab, 80, 60);
        ctx.fill();
        ctx.strokeStyle = "#000";
        ctx.stroke();
      }
      markers.push({ x: cx, y: cy, prefab, wx: x, wz: z });
    }
  }
  // Spawn star.
  const [sx, sy] = at(world.summary.spawn.x, world.summary.spawn.z), r = Math.max(8, size * 0.45);
  ctx.beginPath();
  for (let k = 0; k < 10; k++) {
    const a = -Math.PI / 2 + (k * Math.PI) / 5, rr = k % 2 ? r * 0.45 : r;
    ctx.lineTo(sx + rr * Math.cos(a), sy + rr * Math.sin(a));
  }
  ctx.closePath();
  ctx.fillStyle = "#fff";
  ctx.strokeStyle = "#000";
  ctx.lineWidth = 2;
  ctx.fill();
  ctx.stroke();
  drawCompass(ctx, W);
}

// The game's HUD compass (from the install), top right, turned so its N
// points at the game's north; a drawn arrow when the art is not built in.
function brandImage(name) {
  const src = (assets.brand || {})[name];
  if (!src) return null;
  let img = images.get(src);
  if (!img) {
    img = new Image();
    img.onload = () => scheduleDraw();
    img.src = src;
    images.set(src, img);
  }
  return img.complete && img.naturalWidth ? img : null;
}

function drawCompass(ctx, W) {
  const n = (northHeading() * Math.PI) / 180;
  // Facing heading n is facing north: world direction (-cos n, -sin n).
  const nu = -Math.cos(n), nv = -Math.sin(n);
  const { sn, cs } = axes();
  const dx = -sn * nu + cs * nv, dy = cs * nu + sn * nv;
  // Rotation that turns "up" into the screen direction of north.
  const turn = Math.atan2(dy, dx) + Math.PI / 2;
  const face = brandImage("compass_bg"), needle = brandImage("compass_needle");
  const size = 76, cx = W - size / 2 - 10, cy = size / 2 + 10;
  ctx.save();
  ctx.translate(cx, cy);
  if (face) {
    ctx.save();
    ctx.rotate(turn);
    ctx.drawImage(face, -size / 2, -size / 2, size, size);
    ctx.restore();
    if (needle) {
      const nh = size * 0.62, nw = (nh * needle.naturalWidth) / needle.naturalHeight;
      ctx.rotate(turn);
      ctx.drawImage(needle, -nw / 2, -nh / 2, nw, nh);
    }
    ctx.restore();
    return;
  }
  ctx.beginPath();
  ctx.arc(0, 0, 26, 0, Math.PI * 2);
  ctx.fillStyle = "rgba(234, 217, 181, 0.9)";
  ctx.fill();
  ctx.lineWidth = 2;
  ctx.strokeStyle = "#241a12";
  ctx.stroke();
  ctx.rotate(turn);
  ctx.beginPath();
  ctx.moveTo(0, -20);
  ctx.lineTo(7, 12);
  ctx.lineTo(0, 7);
  ctx.lineTo(-7, 12);
  ctx.closePath();
  ctx.fillStyle = "#8e2a1c";
  ctx.fill();
  ctx.fillStyle = "#241a12";
  ctx.font = "bold 11px Georgia, serif";
  ctx.textAlign = "center";
  ctx.fillText("N", 0, -22 + 30);
  ctx.restore();
}

// Zoom and pan.
function zoomAt(factor, cx, cy) {
  const s = Math.max(0.3, Math.min(40, viewState.scale * factor));
  const f = s / viewState.scale;
  viewState.ox = cx - (cx - viewState.ox) * f;
  viewState.oy = cy - (cy - viewState.oy) * f;
  viewState.scale = s;
  scheduleDraw();
}

// Turn the view like the game's camera (45 degrees per step), around the
// middle of the map view.
function rotateBy(degrees) {
  const wrap = $("map-wrap");
  const cx = wrap.clientWidth / 2, cy = wrap.clientHeight / 2;
  const [u, v] = unproject(cx, cy);
  viewState.heading = (viewState.heading + degrees + 360) % 360;
  const [px, py] = project(u, v);
  viewState.ox += cx - px;
  viewState.oy += cy - py;
  scheduleDraw();
}
$("map-wrap").addEventListener("wheel", (e) => {
  e.preventDefault();
  const r = $("map-wrap").getBoundingClientRect();
  zoomAt(e.deltaY < 0 ? 1.2 : 1 / 1.2, e.clientX - r.left, e.clientY - r.top);
}, { passive: false });
let drag = null;
$("map-wrap").addEventListener("pointerdown", (e) => {
  drag = { x: e.clientX, y: e.clientY, ox: viewState.ox, oy: viewState.oy };
  $("map-wrap").setPointerCapture(e.pointerId);
  $("map-wrap").classList.add("dragging");
});
$("map-wrap").addEventListener("pointerup", () => { drag = null; $("map-wrap").classList.remove("dragging"); });
$("map-wrap").addEventListener("pointermove", (e) => {
  const r = $("map-wrap").getBoundingClientRect();
  const mx = e.clientX - r.left, my = e.clientY - r.top;
  if (drag) {
    viewState.ox = drag.ox + e.clientX - drag.x;
    viewState.oy = drag.oy + e.clientY - drag.y;
    scheduleDraw();
    $("tooltip").hidden = true;
    return;
  }
  let best = null, bd = 14 * 14;
  for (const m of markers) {
    const d = (m.x - mx) ** 2 + (m.y - my) ** 2;
    if (d < bd) { bd = d; best = m; }
  }
  const tip = $("tooltip");
  if (!best) { tip.hidden = true; return; }
  const s = world.summary.spawn;
  const dist = Math.hypot(best.wx - s.x, best.wz - s.z) / 4;
  tip.textContent = `${displayName(best.prefab)} · ${Math.round(dist)} tiles from spawn`;
  tip.style.left = `${best.x + 12}px`;
  tip.style.top = `${best.y + 12}px`;
  tip.hidden = false;
});
$("map-wrap").addEventListener("pointerleave", () => { $("tooltip").hidden = true; });
$("zoom-in").addEventListener("click", () => zoomAt(1.4, $("map-wrap").clientWidth / 2, $("map-wrap").clientHeight / 2));
$("zoom-out").addEventListener("click", () => zoomAt(1 / 1.4, $("map-wrap").clientWidth / 2, $("map-wrap").clientHeight / 2));
$("zoom-reset").addEventListener("click", () => { viewState.heading = 45; fitView(); });
$("rotate-left").addEventListener("click", () => rotateBy(-45));
$("rotate-right").addEventListener("click", () => rotateBy(45));
document.addEventListener("keydown", (e) => {
  if (!world || $("world").hidden || e.ctrlKey || e.metaKey || e.altKey) return;
  const tag = (e.target && e.target.tagName) || "";
  if (tag === "INPUT" || tag === "SELECT" || tag === "TEXTAREA" || $("picker").open) return;
  const key = e.key.toLowerCase();
  if (key === "q") { rotateBy(-45); e.preventDefault(); }
  if (key === "e") { rotateBy(45); e.preventDefault(); }
});
window.addEventListener("resize", () => world && scheduleDraw());

function renderLegend() {
  const q = $("legend-filter").value.trim().toLowerCase();
  const box = $("legend");
  box.innerHTML = "";
  const present = Object.entries(world.positions).map(([p, pts]) => [p, pts.length]);
  const match = ([p]) => !q || p.includes(q) || displayName(p).toLowerCase().includes(q);
  const groups = [
    ["Points of interest", present.filter(([p]) => assets.notable.has(p) || assets.icons[p] && filters.some((f) => f.key === p))],
    ["Everything else", present.filter(([p]) => !assets.notable.has(p) && !filters.some((f) => f.key === p))],
  ];
  for (const [label, items] of groups) {
    const list = items.filter(match).sort((a, b) => displayName(a[0]).localeCompare(displayName(b[0])));
    if (!list.length) continue;
    box.append(el("div", { class: "legend-group" }, label));
    for (const [p, n] of list) {
      const box2 = el("input", { type: "checkbox", checked: shownPrefabs.has(p), onchange: (e) => {
        if (e.target.checked) shownPrefabs.add(p); else shownPrefabs.delete(p);
        scheduleDraw();
      } });
      box.append(el("label", {}, box2,
        assets.icons[p] ? el("img", { src: assets.icons[p], alt: "" }) : el("span", { class: "dot", style: `background:${hashColour(p, 80, 60)}` }),
        el("span", {}, displayName(p)), el("span", { class: "count" }, n)));
    }
  }
}
$("legend-filter").addEventListener("input", () => world && renderLegend());

function worldLabel(s) {
  if (s.dlc === "dst") return s.map.prefab === "cave" ? "Don't Starve Together · Caves" : "Don't Starve Together · Forest";
  const level = (LEVELS[s.dlc] || []).find(([v]) => v === s.level_type);
  return `${DLC_NAMES[s.dlc]} · ${level ? level[1] : s.level_type}`;
}

async function showWorld(seed) {
  currentSeed = seed;
  renderTables();
  $("world").hidden = false;
  $("world-title").textContent = `Seed ${seed}`;
  $("world-sub").textContent = "Generating…";
  const { result } = await pool.run({ type: "generate", request: request(seed, { savedata: true }) });
  if (!result || result.error) {
    $("world-sub").textContent = `Could not generate this seed: ${result ? result.error : "no result"}`;
    return;
  }
  if (currentSeed !== seed) return;
  const s = result.summary;
  world = { ...result, layer: tileLayer(result) };
  const present = Object.keys(world.positions);
  // Shown at first: points of interest that have a game icon, and whatever the filters ask for.
  shownPrefabs = new Set(present.filter((p) => (assets.notable.has(p) && assets.icons[p]) || filters.some((f) => f.kind === "prefab" && f.key === p)));
  $("world-sub").textContent = `${worldLabel(s)} · ${s.map.width}×${s.map.height} tiles · ${s.map.nodes} rooms · ${s.map.roads} roads`;
  $("tasks").replaceChildren(...(s.tasks || []).map((t) => el("li", {}, `${t.name} `, el("span", { class: "muted" }, `(${t.rooms})`))));
  $("legend-filter").value = "";
  renderLegend();
  $("world").scrollIntoView({ behavior: "smooth", block: "start" });
  viewState.heading = 45;
  requestAnimationFrame(fitView);
}
$("download-savedata").addEventListener("click", () => world && download(`seed-${world.summary.seed}-savedata.lua`, world.savedata, "text/plain"));

// ---------------------------------------------------------------- start

(async () => {
  $("workers").value = defaultWorkers();
  try {
    await compileModule();
  } catch (e) {
    // Let each worker load the module itself.
    console.warn(e);
  }
  pool = new Pool(defaultWorkers());
  const info = await pool.ready;
  $("loading").hidden = true;
  if (!info || info.error || (!info.ds && !info.dst)) {
    $("nodata").hidden = false;
    if (info && info.error) $("nodata-error").textContent = info.error;
    return;
  }
  embedded = info;
  const kinds = ["ds", "dst"].filter((k) => info[k]);
  fillSelect($("game"), kinds.map((k) => [k, k === "ds" ? "Don't Starve" : "Don't Starve Together"]), kinds[0]);
  setupGame();
  $("step-game").hidden = false;
  $("step-filters").hidden = false;
  await worldChanged();
  renderPills();
})();

$("workers").addEventListener("change", async () => {
  if (searching) return;
  const n = Math.max(1, Math.min(32, Number($("workers").value) || defaultWorkers()));
  pool.terminate();
  pool = new Pool(n);
  await pool.ready;
  warmKey = null;
  warmUp();
});
