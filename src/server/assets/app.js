// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

// The console page (lot 7): the 9 cells, the feed, the composer, private
// messages. Live from the server's SSE stream. Strings from relays (texts,
// nicknames, relay messages) only ever go through `textContent`.

import {
  author, clock, conversations, convoOver, hasLeft, feed, feedCells, gridOrder, groupTexts, isImage, linkify, nicknames, people, personNote,
  relayCount, repeatLabel, statusMark, typedCell, unixFromLocal, unreadTotal, visible,
} from "./view.js";

function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

// --- State -----------------------------------------------------------------

const state = {
  status: null,         // area status from the server
  messages: {},         // cell -> messages
  dms: [],
  selected: null,       // cell shown alone, or null for all 9
  convo: null,          // open conversation key "peer|cell"
  link: "connecting",
  texts: [],            // last relay texts (NOTICE, CLOSED, refusals)
  local: { mutes: [], presets: [], schedules: [], nickname: null, anon_cells: [] },
  isolate: null,        // key shown alone in the feed (isolation window)
  groups: { groups: [], messages: [], invitations: [] },
  group: null,          // open group id
  writeCell: null,      // cell chosen in the composer, until another is shown
  people: { cell: null, list: [] },  // participants of the current cell
  muteNeighbours: false,  // the feed shows the centre only (kept in the browser)
  present: null,        // last seen of each key in each cell (lot 15)
};
state.muteNeighbours = (() => { try { return localStorage.getItem("muteNeighbours") === "1"; } catch { return false; } })();

// The cell shown alone, else the centre.
const currentCell = () => state.selected || (state.status && state.status.center);

const saved = key => { try { return localStorage.getItem(key) || ""; } catch { return ""; } };
const save = (key, v) => { try { localStorage.setItem(key, v); } catch { /* private window */ } };

// The nickname is kept by the console, for every cell (D32), and not sent in
// the cells the user chose.
const myNickname = () => state.local.nickname || "";
const isAnon = cell => state.local.anon_cells.includes(cell);

// Pages before lot 13 kept the nickname in the browser, per origin (lost when
// the port changes): handed over to the console once.
function handOverNickname() {
  const old = saved("nickname");
  if (!old) return;
  const drop = () => { try { localStorage.removeItem("nickname"); } catch { /* private window */ } };
  if (state.local.nickname) { drop(); return; }
  command("api/nickname", { nickname: old }).then(({ ok }) => { if (ok) drop(); });
}

// A message field: grows with its text (a few lines at most, then scrolls),
// Enter sends, Shift+Enter goes to the next line.
const grow = f => { f.style.height = "auto"; f.style.height = f.scrollHeight + 2 + "px"; };
function messageField(key, placeholder, go) {
  const f = field("textarea", key, "text");
  f.rows = 1;
  f.placeholder = placeholder;
  f.addEventListener("input", () => grow(f));
  f.addEventListener("keydown", e => {
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) { e.preventDefault(); go(); }
  });
  return f;
}
const clear = f => { f.value = ""; if (f.tagName === "TEXTAREA") grow(f); };

// Paths are relative: the page is served at `/`, or at `/console/` over the
// map of bitchat-geohash (lot 9).
async function command(path, body) {
  try {
    const r = await fetch(path, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
    let data = null;
    try { data = await r.json(); } catch { /* not JSON */ }
    return { ok: r.ok, data };
  } catch {
    return { ok: false, data: { message: "server unreachable" } };
  }
}

// Who is still in their cell (lot 15): asked every 20 s; a message seen in
// between counts at once.
const now = () => Date.now() / 1000;
async function refreshPresent() {
  try {
    const r = await fetch("api/present");
    if (r.ok) { state.present = await r.json(); renderFeed(); renderDms(); }
  } catch { /* the stream says when the console is gone */ }
}
setInterval(refreshPresent, 20_000);
function seenNow(cell, pubkey, at) {
  if (!state.present) return;
  const c = state.present.cells[cell] || (state.present.cells[cell] = {});
  c[pubkey] = Math.max(c[pubkey] || 0, at);
}

// --- Re-rendering without getting in the way --------------------------------
//
// Sections are rebuilt on every live event. Fields marked `data-keep` keep
// their value, the focus and the caret; a section is not rebuilt while a
// button is held (the click would be lost) or text is selected in it.

const later = new Map();  // section id -> draw, postponed
let pressed = false;

function selecting(box) {
  const s = document.getSelection();
  return !!s && !s.isCollapsed && s.rangeCount > 0
    && box.contains(s.getRangeAt(0).commonAncestorContainer);
}

// The pointer is over a list that must not move under it (a name about to
// be clicked).
const hovering = box => !!box.querySelector("[data-hold]:hover");

function keep(id, draw) {
  const box = document.getElementById(id);
  if (pressed || selecting(box) || hovering(box)) { later.set(id, draw); return; }
  later.delete(id);
  const values = new Map();
  // Inputs keep their value, messages beside them their text.
  const valueOf = f => ("value" in f ? f.value : f.textContent);
  for (const f of box.querySelectorAll("[data-keep]")) values.set(f.dataset.keep, valueOf(f));
  // Where each field had scrolled to: a long text keeps its end in view.
  const fieldScrolls = new Map([...box.querySelectorAll("input[data-keep], textarea[data-keep]")]
    .map(f => [f.dataset.keep, { left: f.scrollLeft, top: f.scrollTop }]));
  const a = document.activeElement;
  const focus = a && box.contains(a) && a.dataset.keep
    ? { key: a.dataset.keep, start: a.selectionStart, end: a.selectionEnd } : null;
  const top = box.scrollTop;
  const scrolls = new Map([...box.querySelectorAll("[data-scroll]")]
    .map(e => [e.dataset.scroll, { top: e.scrollTop, bottom: e.scrollHeight - e.scrollTop - e.clientHeight < 40 }]));
  draw(box);
  // Inner lists keep their place; those marked `stick` follow the newest
  // when they were at the bottom.
  for (const e of box.querySelectorAll("[data-scroll]")) {
    const was = scrolls.get(e.dataset.scroll);
    if (e.dataset.stick !== undefined && (!was || was.bottom)) e.scrollTop = e.scrollHeight;
    else if (was) e.scrollTop = was.top;
  }
  for (const f of box.querySelectorAll("[data-keep]")) {
    if (!values.has(f.dataset.keep)) continue;
    // A field showing the console's state (the nickname) keeps what the
    // user is typing only.
    if (f.dataset.state !== undefined && !(focus && focus.key === f.dataset.keep)) continue;
    const v = values.get(f.dataset.keep);
    if (f.tagName === "SELECT" && ![...f.options].some(o => o.value === v)) continue;
    if ("value" in f) f.value = v; else f.textContent = v;
    if (f.tagName === "TEXTAREA") grow(f);
  }
  if (focus) {
    const f = box.querySelector(`[data-keep="${CSS.escape(focus.key)}"]`);
    if (f) {
      f.focus({ preventScroll: true });
      try { if (focus.start !== null) f.setSelectionRange(focus.start, focus.end); } catch { /* not text */ }
    }
  }
  for (const f of box.querySelectorAll("input[data-keep], textarea[data-keep]")) {
    const was = fieldScrolls.get(f.dataset.keep);
    if (was) { f.scrollLeft = was.left; f.scrollTop = was.top; }
  }
  if (id !== "feed") box.scrollTop = top;
}

function flush() {
  for (const [id, draw] of [...later]) keep(id, draw);
}

document.addEventListener("pointerdown", () => { pressed = true; }, true);
for (const name of ["pointerup", "pointercancel"]) {
  // After the click that follows.
  document.addEventListener(name, () => { pressed = false; setTimeout(flush, 0); }, true);
}
document.addEventListener("selectionchange", () => { if (later.size) flush(); });
// Leaving a held list: catch up.
document.addEventListener("pointerout", () => { if (later.size) setTimeout(flush, 0); }, true);

/// A list that folds (as the user left it) and scrolls within a bounded
/// height, so that no list takes the whole panel.
function fold(key, title, items, { open = true, stick = false, hold = false } = {}) {
  const d = el("details", "fold");
  const was = saved("fold:" + key);
  d.open = was ? was === "1" : open;
  d.addEventListener("toggle", () => save("fold:" + key, d.open ? "1" : "0"));
  const body = el("div", "scroll");
  body.dataset.scroll = key;
  if (stick) body.dataset.stick = "";
  if (hold) body.dataset.hold = "";
  body.append(...items);
  d.append(el("summary", null, title), body);
  return d;
}

// --- Sound on a new private message ------------------------------------------
//
// The browser lets a page play sound only after the user has touched it:
// the audio context is made at the first click or key.

let audio = null;
const unlock = () => {
  try {
    audio = audio || new AudioContext();
    if (audio.state === "suspended") audio.resume();
  } catch { /* no audio */ }
};
document.addEventListener("pointerdown", unlock, true);
document.addEventListener("keydown", unlock, true);

function ding() {
  if (!ui.sound || !ui.sound.checked || !audio) return;
  try {
    const t = audio.currentTime;
    for (const [f, at] of [[880, 0], [1320, 0.12]]) {
      const o = audio.createOscillator();
      const g = audio.createGain();
      o.frequency.value = f;
      g.gain.setValueAtTime(0.0001, t + at);
      g.gain.exponentialRampToValueAtTime(0.15, t + at + 0.01);
      g.gain.exponentialRampToValueAtTime(0.0001, t + at + 0.2);
      o.connect(g).connect(audio.destination);
      o.start(t + at);
      o.stop(t + at + 0.22);
    }
  } catch { /* no audio */ }
}

/// A field marked `key`, kept across re-rendering.
function field(tag, key, cls) {
  const f = el(tag, cls);
  f.dataset.keep = key;
  return f;
}

/// The field as it is now on the page: the one a handler closed over may
/// have been replaced since.
const live = f => document.querySelector(`[data-keep="${CSS.escape(f.dataset.keep)}"]`) || f;

// --- Participants of the current cell ------------------------------------------

let peopleTimer = null;

// Asked again shortly after a change, and every 15 s (last seen times).
function refreshPeople(delay = 500) {
  clearTimeout(peopleTimer);
  peopleTimer = setTimeout(async () => {
    const cell = currentCell();
    if (!cell) return;
    try {
      const r = await fetch("api/participants?cell=" + encodeURIComponent(cell));
      if (r.ok && cell === currentCell()) {
        state.people = { cell, list: await r.json() };
        renderCells();
      }
    } catch { /* next time */ }
  }, delay);
}
setInterval(() => refreshPeople(0), 15000);

// --- Top bar ------------------------------------------------------------------

const ui = {};

function buildTop() {
  const top = document.getElementById("top");
  ui.title = el("h1", null, "BitChat console");
  ui.geohash = el("input");
  ui.geohash.placeholder = "geohash (2, 4, 5, 6 or 7 characters)";
  ui.geohash.title = "a geohash (u09tv or #u09tv)";
  ui.geohash.setAttribute("aria-label", "geohash");
  ui.teleport = el("input");
  ui.teleport.type = "checkbox";
  ui.teleport.checked = saved("teleport") !== "0";
  const tl = el("label", "check");
  tl.append(ui.teleport, document.createTextNode(" teleported"));
  tl.title = "Say you are not there: messages tagged teleport, no presence";
  ui.follow = el("button", null, "Follow");
  ui.follow.addEventListener("click", follow);
  ui.geohash.addEventListener("keydown", e => { if (e.key === "Enter") follow(); });
  // Typed by the user: kept until followed. The field loses the focus when
  // the button is pressed, before the click; a live update must not put the
  // current centre back in between.
  ui.geohash.addEventListener("input", () => { ui.edited = true; });
  ui.link = el("span", "link");
  ui.info = el("span", "muted");
  // On the title's line (lot 21): the version, the approximate name of the
  // cell followed (lot 19).
  ui.version = el("span", "muted small");
  ui.version.title = "version of this bitchat-console";
  ui.place = el("span", "place muted small");
  ui.error = el("span", "warn");
  ui.sound = el("input");
  ui.sound.type = "checkbox";
  ui.sound.checked = saved("sound") !== "0";
  ui.sound.addEventListener("change", () => { save("sound", ui.sound.checked ? "1" : "0"); if (ui.sound.checked) ding(); });
  const sl = el("label", "check");
  sl.append(ui.sound, document.createTextNode(" 🔔"));
  sl.title = "sound on a new private message";
  ui.unread = el("button", "unread");
  ui.unread.title = "unread private messages: open the first";
  ui.unread.addEventListener("click", () => {
    const d = state.dms.find(x => !x.own && !x.read);
    if (d) openConvo(d.peer, d.cell);
  });
  // Everything kept (D31), in a tab of its own (lot 16).
  ui.history = el("a", "small", "history ↗");
  ui.history.target = "_blank";
  ui.history.rel = "noopener";
  ui.history.title = "every message kept, for the cell shown: search, sort, delete";
  top.append(ui.title, ui.version, ui.place, ui.geohash, tl, ui.follow, ui.link, ui.info, sl, ui.unread, ui.history, ui.error);
}

async function follow() {
  // A place or an IP address too, when the console looks them up (lot 19).
  const geohash = typedCell(ui.geohash.value);
  if (!geohash) return;
  save("teleport", ui.teleport.checked ? "1" : "0");
  ui.error.textContent = "";
  const { ok, data } = await command("api/area", { geohash, teleport: ui.teleport.checked });
  if (!ok) { ui.error.textContent = (data && data.message) || "refused"; return; }
  // A place typed: the field shows the cell it gave (lot 19).
  if (data && data.cells && data.cells.length) ui.geohash.value = data.cells[0];
  ui.edited = false;
  state.selected = null;
  state.writeCell = null;
  refreshPeople(0);
}

function updateTop() {
  const cell = currentCell();
  ui.history.href = "history" + (cell ? "?cell=" + encodeURIComponent(cell) : "");
  const n = unreadTotal(state.dms);
  ui.unread.textContent = n ? `✉ ${n}` : "";
  ui.unread.hidden = !n;
  document.title = (n ? `(${n}) ` : "") + "BitChat console";
  ui.link.className = "link " + state.link;
  ui.link.textContent = { live: "● live", connecting: "● connecting…", reconnecting: "● reconnecting…" }[state.link];
  const s = state.status;
  if (!s) return;
  if (s.center && !ui.edited && document.activeElement !== ui.geohash) ui.geohash.value = s.center;
  ui.info.textContent = s.center
    ? ` #${s.center}${s.teleport ? " · teleported" : " · on site"} · ${s.connector}`
    : ` ${s.connector}`;
  if (s.center) nameCell(s.center);
}

// --- Name of the cell (lot 19) ----------------------------------------------------

// Asked once per cell. A console without places answers 404: the field then
// keeps its geohash-only hints.
async function nameCell(cell) {
  if (ui.place.dataset.for === cell || ui.place.dataset.off) return;
  ui.place.dataset.for = cell;
  ui.place.textContent = "";
  ui.place.title = "";
  try {
    const resp = await fetch("api/place?geohash=" + encodeURIComponent(cell));
    if (resp.status === 404) {
      const data = await resp.json().catch(() => null);
      if (data && data.error === "no_places") { ui.place.dataset.off = "1"; return; }
    }
    ui.geohash.placeholder = "geohash, a place or an IP address";
    ui.geohash.title = "a geohash (u09tv or #u09tv), a place (Rue de Rivoli, Paris) or an IP address; "
      + "places are looked up in direct, not through Tor";
    const data = resp.ok ? await resp.json() : null;
    if (ui.place.dataset.for !== cell || !data || !data.place) return;
    ui.place.textContent = "≈ " + data.place.label;
    ui.place.title = `${data.place.label}\napproximate name of #${cell} (${data.place.source}: ${data.place.attribution})`;
  } catch { /* no name: the cell is shown anyway */ }
}

// --- Cells ----------------------------------------------------------------------

function renderCells() { keep("cells", drawCells); }

function drawCells(box) {
  box.replaceChildren();
  const s = state.status;
  if (!s || !s.cells.length) {
    box.append(el("p", "muted", "Choose a geohash to follow it and its 8 neighbours."));
    return;
  }
  const all = el("button", "cell all" + (state.selected === null ? " on" : ""),
    state.muteNeighbours ? `#${s.center} only (neighbours muted)` : "all 9 cells");
  all.addEventListener("click", () => {
    state.selected = null; state.writeCell = null; render(); refreshPeople(0);
  });
  const grid = el("div", "grid");
  for (const c of gridOrder(s.cells)) {
    const b = el("button", "cell" + (state.selected === c.geohash ? " on" : "")
      + (c.geohash === s.center ? " center" : "")
      + (state.muteNeighbours && c.geohash !== s.center ? " quiet" : ""));
    const n = (state.messages[c.geohash] || []).length;
    const me = s.keys && s.keys[c.geohash];
    b.title = "people: keys seen in the last 15 minutes (messages and presences)\n"
      + "msgs: channel messages received since the console started (relays keep none)"
      + (overMap ? "\ndouble-click or ctrl-click: the map watches the area of this cell" : "");
    b.append(el("b", null, "#" + c.geohash), el("span", "muted", ` ${c.participants} people`),
      el("span", "muted small", ` ${n} msgs · relays ${relayCount(c)}`));
    if (me) {
      const you = el("span", "you small", "you #" + me.slice(-4));
      you.title = "your key in this cell: " + me;
      b.append(you);
    }
    b.addEventListener("click", e => {
      if (e.ctrlKey || e.metaKey) { mapAreaOf(c.geohash); return; }
      state.selected = c.geohash; state.writeCell = null; render(); refreshPeople(0);
    });
    // Over the map: another area for it. A ctrl-click on a Mac is a
    // context menu.
    b.addEventListener("dblclick", () => mapAreaOf(c.geohash));
    b.addEventListener("contextmenu", e => {
      if (!overMap || !e.ctrlKey) return;
      e.preventDefault();
      mapAreaOf(c.geohash);
    });
    grid.append(b);
  }
  // Muting the 8 neighbours: their messages leave the feed (still
  // followed: counts, private messages, a click shows one).
  const mute = el("input");
  mute.type = "checkbox";
  mute.checked = state.muteNeighbours;
  mute.addEventListener("change", () => {
    state.muteNeighbours = mute.checked;
    save("muteNeighbours", mute.checked ? "1" : "0");
    postView();
    renderCells();
    renderFeed();
  });
  const muteLabel = el("label", "check small");
  muteLabel.append(mute, document.createTextNode(" mute the 8 neighbours"));
  muteLabel.title = "show only the centre's messages; a click on a neighbour still shows its own";
  box.append(all, grid, muteLabel, drawPeople(s));
  if (state.local.mutes.length) {
    const m = el("div", "muted-list");
    m.append(el("h3", null, "Muted"));
    for (const key of state.local.mutes) {
      const row = el("div", "row");
      const show = el("button", null, "isolate");
      show.title = "show only this person's messages";
      show.addEventListener("click", () => { state.isolate = key; renderFeed(); });
      const un = el("button", null, "unmute");
      un.addEventListener("click", () => command("api/mute", { pubkey: key, muted: false }));
      row.append(el("span", null, "#" + key.slice(-4) + " "), show, un);
      m.append(row);
    }
    box.append(m);
  }
  if (state.texts.length) {
    const groups = groupTexts(state.texts);
    const rows = groups.map(g => {
      const row = el("div", "small text-row");
      row.append(el("b", null, g.relay.replace(/^wss?:\/\//, "")), document.createTextNode(" " + g.text + " "),
        el("span", "muted", `×${g.count} · ${clock(g.last)} · #${g.cells.slice(0, 4).join(" #")}`
          + (g.cells.length > 4 ? ` +${g.cells.length - 4}` : "")));
      return row;
    });
    box.append(fold("relay-texts", `Relay messages (${groups.length})`, rows, { open: false }));
  }
}

// Everyone seen in the current cell: a name opens a private conversation.
function drawPeople(s) {
  const cell = currentCell();
  const me = s.keys && s.keys[cell];
  const known = state.people.cell === cell ? state.people.list : [];
  const list = people(known, me, state.local.mutes);
  const names = nicknames(state.messages);
  const box = el("div", "people");
  if (me) {
    const you = el("div", "you small", "you: " + author(isAnon(cell) ? "anon" : myNickname(), me)
      + (isAnon(cell) ? " (nickname not sent here)" : ""));
    you.title = me;
    box.append(you);
  }
  const rows = [];
  const now = Date.now() / 1000;
  for (const p of list) {
    const row = el("div", "row person");
    const who = el("button", "who", author(p.nickname || names.get(p.pubkey), p.pubkey));
    who.title = "private message · " + p.pubkey;
    who.addEventListener("click", () => openConvo(p.pubkey, cell));
    row.append(who, el("span", "muted small", personNote(p, now)));
    if (state.local.mutes.includes(p.pubkey)) {
      row.append(el("span", "muted small", " · muted"));
    } else {
      const mute = el("button", "mute", "🔇");
      mute.title = "mute: hide everything from this key";
      mute.setAttribute("aria-label", "mute");
      mute.addEventListener("click", () => command("api/mute", { pubkey: p.pubkey, muted: true }));
      row.append(mute);
    }
    rows.push(row);
  }
  if (!list.length) rows.push(el("p", "muted small", "Nobody else seen here in the last 15 minutes."));
  box.append(fold("people", `In #${cell} · ${list.length} other${list.length === 1 ? "" : "s"}`, rows, { hold: true }));
  return box;
}

// --- Feed -------------------------------------------------------------------------

function textNodes(parent, text) {
  for (const part of linkify(text)) {
    if (part.link) {
      const a = el("a", isImage(part.link) ? "image" : null, part.link);
      a.href = part.link;
      a.target = "_blank";
      a.rel = "noopener noreferrer nofollow";
      parent.append(a);
    } else {
      parent.append(document.createTextNode(part.text));
    }
  }
}

function renderFeed() { keep("feed", drawFeed); }

function drawFeed(box) {
  box.replaceChildren();
  const s = state.status;
  if (!s || !s.cells.length) return;
  const cells = feedCells(s.cells.map(c => c.geohash), s.center, state.selected, state.muteNeighbours);
  const list = el("div", "messages");
  if (state.isolate) {
    const bar = el("div", "isolate");
    const back = el("button", null, "back to all");
    back.addEventListener("click", () => { state.isolate = null; renderFeed(); });
    bar.append(el("span", null, `Only #${state.isolate.slice(-4)} `), back);
    list.append(bar);
  }
  for (const m of visible(feed(state.messages, cells), state.local.mutes, state.isolate)) {
    const left = !m.own && hasLeft(state.present, m.cell, m.pubkey, now());
    const row = el("div", "msg" + (m.own ? " own" : "") + (left ? " left" : ""));
    if (left) row.title = "left the cell (not seen for 10 min)";
    const who = el("button", "who", author(m.nickname, m.pubkey));
    who.title = m.own ? "you" : "private message";
    if (!m.own) who.addEventListener("click", () => openConvo(m.pubkey, m.cell));
    const meta = el("span", "muted small",
      ` ${clock(m.created_at)} #${m.cell}${m.teleport ? " · teleported" : ""}`
      + (m.own ? ` · ${m.relays.length ? "✓ " + m.relays.length : "…"}` : ""));
    const body = el("div", "body");
    textNodes(body, m.content);
    row.append(who, meta);
    if (!m.own) {
      const mute = el("button", "mute", "🔇");
      mute.title = "mute: hide everything from this key";
      mute.setAttribute("aria-label", "mute");
      mute.addEventListener("click", () => command("api/mute", { pubkey: m.pubkey, muted: true }));
      row.append(mute);
    }
    row.append(body);
    list.append(row);
  }
  const n = list.querySelectorAll(".msg").length;
  const where = state.selected ? " in #" + state.selected
    : state.muteNeighbours ? ` in #${s.center} (neighbours muted)` : " in the 9 cells";
  box.append(fold("feed", `Messages · ${n}${where}`,
    [list], { stick: true, hold: true }), composer());
}

function composer() {
  const s = state.status;
  const form = el("div", "composer");
  const cell = el("select");
  cell.setAttribute("aria-label", "cell to write in");
  for (const c of s.cells) {
    const o = el("option", null, "#" + c.geohash + (c.geohash === s.center ? " (centre)" : ""));
    o.value = c.geohash;
    cell.append(o);
  }
  const cells = s.cells.map(c => c.geohash);
  cell.value = cells.includes(state.writeCell) ? state.writeCell : state.selected || s.center;
  const nick = field("input", "nickname");
  nick.placeholder = "nickname";
  nick.title = "your nickname, the same in every cell";
  nick.value = myNickname();
  nick.dataset.state = "";
  nick.addEventListener("change", () => command("api/nickname", { nickname: nick.value.trim() || null }));
  // Per cell: send the nickname or not (D32).
  const sendNick = el("input");
  sendNick.type = "checkbox";
  const sendNickLabel = el("label", "small");
  sendNickLabel.title = "Off: no nickname, the app shows anon#…. The app, and relays that keep "
    + "events, still know a nickname already sent with this cell's key (it does not change).";
  const showNick = () => {
    sendNick.checked = !isAnon(cell.value);
    sendNickLabel.lastChild.textContent = ` nickname in #${cell.value}`;
  };
  sendNickLabel.append(sendNick, document.createTextNode(""));
  sendNick.addEventListener("change", () => command("api/anon", { cell: cell.value, anon: !sendNick.checked }));
  cell.addEventListener("change", () => { state.writeCell = cell.value; showNick(); });
  showNick();
  const text = messageField("feed-text", "message", () => go());
  const send = el("button", null, "Send");
  const result = field("span", "feed-result", "warn small");
  const go = async () => {
    const value = live(text).value;
    if (!value.trim()) return;
    send.disabled = true;
    const { ok, data } = await command("api/send", { text: value, cell: cell.value });
    send.disabled = false;
    if (ok) clear(live(text)); else live(result).textContent = (data && data.message) || "refused";
  };
  send.addEventListener("click", go);
  form.append(cell, nick, sendNickLabel, text, send, result);
  return form;
}

// --- Private messages ---------------------------------------------------------------

function openConvo(peer, cell) {
  state.convo = peer + "|" + cell;
  command("api/read", { peer });
  for (const d of state.dms) if (!d.own && d.peer === peer) d.read = true;
  render();
}

function renderDms() { keep("dms", drawDms); }

function drawDms(box) {
  const total = unreadTotal(state.dms);
  box.replaceChildren(el("h2", null, "Private messages" + (total ? ` · ${total} unread` : "")));
  const convos = conversations(state.dms);
  const names = nicknames(state.messages);
  if (!convos.length && !state.convo) {
    box.append(el("p", "muted small", "Click a name in the feed to write to that person."));
    return;
  }
  const button = c => {
    const b = el("button", "convo" + (state.convo === c.key ? " on" : "") + (c.unread ? " new" : ""),
      `${author(names.get(c.peer), c.peer)} in #${c.cell}` + (c.unread ? ` ● ${c.unread}` : ""));
    b.title = c.peer;
    b.addEventListener("click", () => openConvo(c.peer, c.cell));
    return b;
  };
  // Over when the other side left its cell (lot 15); the open one stays.
  const t = now();
  const over = c => c.key !== state.convo && convoOver(c, state.present, t);
  const newest = convos.slice().reverse();
  const active = newest.filter(c => !over(c)), ended = newest.filter(over);
  box.append(fold("convos", `Conversations (${active.length})`, active.map(button), { hold: true }));
  if (ended.length) {
    const all = el("a", "small", "all private messages ↗");
    all.href = "history?kind=dm";
    all.target = "_blank";
    all.rel = "noopener";
    box.append(fold("ended", `Ended (${ended.length}): the other side left`, [...ended.map(button), all],
      { open: false, hold: true }));
  }
  if (!state.convo) return;
  const [peer, cell] = state.convo.split("|");
  const convo = convos.find(c => c.key === state.convo);
  const rows = [];
  for (const d of convo ? convo.messages : []) {
    const row = el("div", "msg" + (d.own ? " own" : ""));
    const body = el("div", "body");
    textNodes(body, d.content);
    const meta = el("span", "muted small", clock(d.created_at) + " ");
    if (d.own) meta.append(el("span", "status " + (d.status || "sent"), statusMark(d.status)));
    const del = el("button", "mute", "×");
    del.title = "delete this message (here only: the other side keeps it)";
    del.setAttribute("aria-label", "delete");
    del.addEventListener("click", () => command("api/dm/delete", { peer, cell, ids: [d.id] }));
    row.append(meta, del, body);
    rows.push(row);
  }
  box.append(fold("thread", `With ${author(names.get(peer), peer)} · your key of #${cell}`, rows, { stick: true }));
  if (rows.length) {
    const wipe = el("button", "small", "delete the conversation");
    wipe.title = "here only: the other side keeps it";
    wipe.addEventListener("click", () => {
      if (confirm("Delete this conversation here? The other side keeps it.")) {
        command("api/dm/delete", { peer, cell });
      }
    });
    box.append(wipe);
  }
  // Inviting this person to one of my groups (I am the creator).
  const mine = state.groups.groups.filter(g => g.creator_is_me);
  if (mine.length) {
    const pick = field("select", "invite-group");
    pick.setAttribute("aria-label", "group to invite to");
    for (const g of mine) {
      const o = el("option", null, g.name);
      o.value = g.id;
      pick.append(o);
    }
    const inv = el("button", null, "Invite to group");
    inv.addEventListener("click", async () => {
      const { ok, data } = await command("api/group", { op: "invite", group: pick.value, peer, cell });
      inv.textContent = ok ? "invitation sent" : (data && data.message) || "refused";
    });
    const row = el("div", "row");
    row.append(pick, inv);
    box.append(row);
  }
  const text = messageField("dm-text|" + state.convo, "private message (255 bytes at most)", () => go());
  const send = el("button", null, "Send");
  const result = field("span", "dm-result|" + state.convo, "warn small");
  const go = async () => {
    const value = live(text).value;
    if (!value.trim()) return;
    const { ok, data } = await command("api/dm", { peer, cell, text: value });
    if (ok) clear(live(text)); else live(result).textContent = (data && data.message) || "refused";
  };
  send.addEventListener("click", go);
  const form = el("div", "composer");
  form.append(text, send, result);
  box.append(form);
}

// --- Rendering and live stream ------------------------------------------------------

// --- Groups (lot 10) ------------------------------------------------------------

function groupOp(body) {
  return command("api/group", body).then(r => {
    if (!r.ok) alert((r.data && r.data.message) || "refused");
    return r;
  });
}

function renderGroups() { keep("groups", drawGroups); }

function drawGroups(box) {
  box.replaceChildren(el("h2", null, "Groups"));
  const v = state.groups;
  for (const inv of v.invitations) {
    const row = el("div", "row invitation");
    const yes = el("button", null, "accept");
    yes.addEventListener("click", () => groupOp({ op: "accept", group: inv.group }));
    const no = el("button", null, "decline");
    no.addEventListener("click", () => groupOp({ op: "decline", group: inv.group }));
    row.append(el("span", null, `${inv.from || "#" + inv.peer.slice(-4)} invites you to “${inv.name}” `), yes, no);
    box.append(row);
  }
  for (const g of v.groups) {
    const b = el("button", "convo" + (state.group === g.id ? " on" : ""),
      `${g.name} · ${g.members.length} members${g.creator_is_me ? " · yours" : ""}`);
    b.addEventListener("click", () => { state.group = state.group === g.id ? null : g.id; renderGroups(); });
    box.append(b);
  }
  const g = v.groups.find(x => x.id === state.group);
  if (g) {
    const members = el("div", "small");
    for (const m of g.members) {
      const row = el("div", "row");
      row.append(el("span", null, (m.nickname || "?") + (m.me ? " (you)" : "") + (m.creator ? " · creator" : "")));
      if (g.creator_is_me && !m.me) {
        const rm = el("button", null, "remove");
        rm.addEventListener("click", () => groupOp({ op: "remove", group: g.id, fingerprint: m.fingerprint }));
        row.append(rm);
      }
      members.append(row);
    }
    box.append(members);
    const thread = [];
    for (const m of v.messages.filter(x => x.group === g.id)) {
      const row = el("div", "msg" + (m.own ? " own" : ""));
      const body = el("div", "body");
      textNodes(body, m.content);
      row.append(el("b", null, m.nickname || "?"), el("span", "muted small", " " + clock(m.created_at)), body);
      thread.push(row);
    }
    const text = messageField("group-text|" + g.id, "message to the group", () => go());
    const send = el("button", null, "Send");
    const go = async () => {
      const value = live(text).value;
      if (!value.trim()) return;
      const { ok } = await groupOp({ op: "send", group: g.id, text: value });
      if (ok) clear(live(text));
    };
    send.addEventListener("click", go);
    const form = el("div", "composer");
    form.append(text, send);
    const end = el("button", null, g.creator_is_me ? "destroy the group" : "leave the group");
    end.addEventListener("click", () => {
      if (confirm(g.creator_is_me ? `Destroy “${g.name}” for everyone?` : `Leave “${g.name}”?`)) {
        groupOp({ op: g.creator_is_me ? "destroy" : "leave", group: g.id });
        state.group = null;
      }
    });
    box.append(fold("group-thread", `Messages of “${g.name}” (${thread.length})`, thread, { stick: true }), form, end);
  }
  const name = field("input", "group-name");
  name.placeholder = "new group name";
  const create = el("button", null, "Create");
  create.addEventListener("click", async () => {
    const value = live(name).value;
    if (!value.trim()) return;
    const { ok } = await groupOp({ op: "create", name: value });
    if (ok) live(name).value = "";
  });
  const form = el("div", "composer");
  form.append(name, create);
  box.append(form, el("p", "muted small",
    "Groups work between consoles, over Nostr; invite someone from a private conversation."));
}

// --- Saved and scheduled messages ------------------------------------------------

function renderSaved() { keep("saved", drawSaved); }

function drawSaved(box) {
  box.replaceChildren(el("h2", null, "Saved messages"));
  for (const p of state.local.presets) {
    const row = el("div", "row");
    const send = el("button", null, "send");
    send.addEventListener("click", async () => {
      const { ok, data } = await command("api/preset/send", { id: p.id, cell: state.selected });
      if (!ok) alert((data && data.message) || "refused");
    });
    const del = el("button", null, "×");
    del.title = "delete";
    del.addEventListener("click", () => command("api/preset/delete", { id: p.id }));
    row.append(el("b", null, p.name || "(no name)"), el("span", "muted small", " " + p.text + " "), send, del);
    box.append(row);
  }
  const name = field("input", "preset-name");
  name.placeholder = "name";
  const text = messageField("preset-text", "text to keep", () => add.click());
  const add = el("button", null, "Save");
  add.addEventListener("click", async () => {
    const [n, t] = [live(name), live(text)];
    if (!t.value.trim()) return;
    const { ok } = await command("api/preset", { name: n.value, text: t.value });
    if (ok) { live(name).value = ""; clear(live(text)); }
  });
  const form = el("div", "composer");
  form.append(name, text, add);
  box.append(form);

  box.append(el("h2", null, "Scheduled"));
  for (const sc of state.local.schedules) {
    const row = el("div", "row");
    const del = el("button", null, "×");
    del.title = "delete";
    del.addEventListener("click", () => command("api/schedule/delete", { id: sc.id }));
    const when = sc.next_at ? clock(sc.next_at) : "done";
    row.append(el("span", null, sc.text + " "),
      el("span", "muted small", `${when} · ${repeatLabel(sc.repeat_secs)}${sc.cell ? " · #" + sc.cell : ""}`
        + (sc.last_result ? ` · last: ${sc.last_result}` : "") + " "), del);
    box.append(row);
  }
  const stext = messageField("schedule-text", "message to send later", () => plan.click());
  const at = field("input", "schedule-at");
  at.type = "datetime-local";
  const repeat = field("select", "schedule-repeat");
  for (const [v, l] of [["", "once"], ["3600", "every hour"], ["86400", "every day"]]) {
    const o = el("option", null, l);
    o.value = v;
    repeat.append(o);
  }
  const plan = el("button", null, "Schedule");
  const result = field("span", "schedule-result", "warn small");
  plan.addEventListener("click", async () => {
    const [t, r] = [live(stext), live(result)];
    const when = unixFromLocal(live(at).value);
    if (!t.value.trim() || !when) { r.textContent = "text and time needed"; return; }
    const rep = live(repeat).value;
    const { ok, data } = await command("api/schedule", {
      text: t.value, at: when, repeat_secs: rep ? Number(rep) : null,
      cell: state.selected,
    });
    if (ok) { clear(live(stext)); live(result).textContent = ""; }
    else live(result).textContent = (data && data.message) || "refused";
  });
  const sform = el("div", "composer");
  sform.append(stext, at, repeat, plan, result);
  box.append(sform, el("p", "muted small", "Scheduled messages go out while the console runs."));
}

const overMap = window.parent !== window;

// Asks the map to watch the area of `cell` (the overlay does it).
function mapAreaOf(cell) {
  if (overMap) window.parent.postMessage({ type: "bitchat-console:area", cell }, location.origin);
}

// Over the map (the page is in the overlay's frame): tells the map which
// cells are shown, so that it shows them too. The first time it does not
// move the map, which keeps its own view.
let lastView = null;
function postView() {
  const s = state.status;
  if (window.parent === window || !s || !s.center) return;
  const view = {
    center: s.center,
    cells: s.cells.map(c => c.geohash),
    selected: state.selected ?? (state.muteNeighbours ? s.center : null),
  };
  const key = JSON.stringify(view);
  if (key === lastView) return;
  const fit = lastView !== null;
  lastView = key;
  window.parent.postMessage({ type: "bitchat-console:view", ...view, fit }, location.origin);
}

function render() {
  postView();
  updateTop();
  renderCells();
  renderFeed();
  renderDms();
  renderGroups();
  renderSaved();
}

function onState(d) {
  ui.version.textContent = d.version;
  state.status = d.status;
  state.messages = d.messages;
  state.dms = d.dms;
  state.local = d.local;
  state.groups = d.groups;
  handOverNickname();
  render();
  refreshPeople();
  refreshPresent();
}

let source = null;
let retry = null;

function connect() {
  if (source) source.close();
  clearTimeout(retry);
  source = new EventSource("api/events");
  const on = (name, f) => source.addEventListener(name, e => { f(JSON.parse(e.data)); });
  on("state", d => { state.link = "live"; onState(d); });
  on("status", s => { state.status = s; updateTop(); renderCells(); });
  on("message", m => {
    const list = state.messages[m.cell] || (state.messages[m.cell] = []);
    if (!list.some(x => x.id === m.id)) list.push(m);
    seenNow(m.cell, m.pubkey, m.created_at);
    renderCells(); renderFeed();
    if (m.cell === currentCell()) refreshPeople();
    // A correspondent's nickname, now known.
    if (m.nickname && state.dms.some(d => d.peer === m.pubkey)) renderDms();
  });
  on("delivered", d => {
    for (const list of Object.values(state.messages)) {
      const m = list.find(x => x.id === d.id);
      if (m && !m.relays.includes(d.relay)) m.relays.push(d.relay);
    }
    renderFeed();
  });
  on("participants", p => {
    const c = state.status && state.status.cells.find(x => x.geohash === p.cell);
    if (c) { c.participants = p.count; renderCells(); }
    if (p.cell === currentCell()) refreshPeople();
  });
  on("dm", dm => {
    state.dms.push(dm);
    if (!dm.own) {
      if (state.convo === dm.peer + "|" + dm.cell && document.visibilityState === "visible") {
        command("api/read", { peer: dm.peer });
        dm.read = true;
      } else {
        ding();
      }
    }
    renderDms();
    updateTop();
  });
  on("dms", list => { state.dms = list; renderDms(); updateTop(); });
  on("dm_status", s => {
    for (const d of state.dms) if (d.own && d.id === s.id) d.status = s.status;
    renderDms();
  });
  on("relay_text", t => {
    state.texts.push({ ...t, at: Date.now() / 1000 });
    if (state.texts.length > 1000) state.texts.splice(0, state.texts.length - 1000);
    renderCells();
  });
  on("local", l => { state.local = l; renderCells(); renderFeed(); renderSaved(); });
  on("groups", g => { state.groups = g; renderGroups(); renderDms(); });
  on("group_message", m => {
    if (!state.groups.messages.some(x => x.group === m.group && x.id === m.id)) state.groups.messages.push(m);
    renderGroups();
  });
  source.onerror = () => {
    state.link = "reconnecting";
    updateTop();
    if (source.readyState === EventSource.CLOSED) retry = setTimeout(connect, 3000);
  };
}

buildTop();
render();
connect();
