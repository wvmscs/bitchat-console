// The console page (lot 7): the 9 cells, the feed, the composer, private
// messages. Live from the server's SSE stream. Strings from relays (texts,
// nicknames, relay messages) only ever go through `textContent`.

import {
  author, clock, conversations, feed, gridOrder, isImage, linkify, people, personNote, relayCount,
  repeatLabel, statusMark, unixFromLocal, visible,
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
  local: { mutes: [], presets: [], schedules: [] },
  isolate: null,        // key shown alone in the feed (isolation window)
  groups: { groups: [], messages: [], invitations: [] },
  group: null,          // open group id
  writeCell: null,      // cell chosen in the composer, until another is shown
  people: { cell: null, list: [] },  // participants of the current cell
};

// The cell shown alone, else the centre.
const currentCell = () => state.selected || (state.status && state.status.center);

const saved = key => { try { return localStorage.getItem(key) || ""; } catch { return ""; } };
const save = (key, v) => { try { localStorage.setItem(key, v); } catch { /* private window */ } };

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

function keep(id, draw) {
  const box = document.getElementById(id);
  if (pressed || selecting(box)) { later.set(id, draw); return; }
  later.delete(id);
  const values = new Map();
  // Inputs keep their value, messages beside them their text.
  const valueOf = f => ("value" in f ? f.value : f.textContent);
  for (const f of box.querySelectorAll("[data-keep]")) values.set(f.dataset.keep, valueOf(f));
  const a = document.activeElement;
  const focus = a && box.contains(a) && a.dataset.keep
    ? { key: a.dataset.keep, start: a.selectionStart, end: a.selectionEnd } : null;
  const top = box.scrollTop;
  draw(box);
  for (const f of box.querySelectorAll("[data-keep]")) {
    if (!values.has(f.dataset.keep)) continue;
    const v = values.get(f.dataset.keep);
    if (f.tagName === "SELECT" && ![...f.options].some(o => o.value === v)) continue;
    if ("value" in f) f.value = v; else f.textContent = v;
  }
  if (focus) {
    const f = box.querySelector(`[data-keep="${CSS.escape(focus.key)}"]`);
    if (f) {
      f.focus({ preventScroll: true });
      try { if (focus.start !== null) f.setSelectionRange(focus.start, focus.end); } catch { /* not text */ }
    }
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
  ui.error = el("span", "warn");
  top.append(ui.title, ui.geohash, tl, ui.follow, ui.link, ui.info, ui.error);
}

async function follow() {
  const geohash = ui.geohash.value.trim().toLowerCase();
  if (!geohash) return;
  save("teleport", ui.teleport.checked ? "1" : "0");
  ui.error.textContent = "";
  const { ok, data } = await command("api/area", { geohash, teleport: ui.teleport.checked });
  if (!ok) { ui.error.textContent = (data && data.message) || "refused"; return; }
  ui.edited = false;
  state.selected = null;
  state.writeCell = null;
  refreshPeople(0);
}

function updateTop() {
  ui.link.className = "link " + state.link;
  ui.link.textContent = { live: "● live", connecting: "● connecting…", reconnecting: "● reconnecting…" }[state.link];
  const s = state.status;
  if (!s) return;
  if (s.center && !ui.edited && document.activeElement !== ui.geohash) ui.geohash.value = s.center;
  ui.info.textContent = s.center
    ? ` #${s.center}${s.teleport ? " · teleported" : " · on site"} · ${s.connector}`
    : ` ${s.connector}`;
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
  const all = el("button", "cell all" + (state.selected === null ? " on" : ""), "all 9 cells");
  all.addEventListener("click", () => {
    state.selected = null; state.writeCell = null; render(); refreshPeople(0);
  });
  const grid = el("div", "grid");
  for (const c of gridOrder(s.cells)) {
    const b = el("button", "cell" + (state.selected === c.geohash ? " on" : "")
      + (c.geohash === s.center ? " center" : ""));
    const n = (state.messages[c.geohash] || []).length;
    const me = s.keys && s.keys[c.geohash];
    b.title = "people: keys seen in the last 15 minutes (messages and presences)\n"
      + "msgs: channel messages received since the console started (relays keep none)";
    b.append(el("b", null, "#" + c.geohash), el("span", "muted", ` ${c.participants} people`),
      el("span", "muted small", ` ${n} msgs · relays ${relayCount(c)}`));
    if (me) {
      const you = el("span", "you small", "you #" + me.slice(-4));
      you.title = "your key in this cell: " + me;
      b.append(you);
    }
    b.addEventListener("click", () => {
      state.selected = c.geohash; state.writeCell = null; render(); refreshPeople(0);
    });
    grid.append(b);
  }
  box.append(all, grid, drawPeople(s));
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
    const log = el("details", "texts");
    log.append(el("summary", "muted", `relay messages (${state.texts.length})`));
    for (const t of state.texts.slice(-20)) {
      log.append(el("div", "small", `${t.relay.replace(/^wss?:\/\//, "")} #${t.cell}: ${t.text}`));
    }
    box.append(log);
  }
}

// Everyone seen in the current cell: a name opens a private conversation.
function drawPeople(s) {
  const cell = currentCell();
  const me = s.keys && s.keys[cell];
  const known = state.people.cell === cell ? state.people.list : [];
  const list = people(known, me, state.local.mutes);
  const box = el("div", "people");
  box.append(el("h3", null, `In #${cell} · ${list.length} other${list.length === 1 ? "" : "s"}`));
  if (me) {
    const you = el("div", "you small", "you: " + author(saved("nickname"), me));
    you.title = me;
    box.append(you);
  }
  const now = Date.now() / 1000;
  for (const p of list) {
    const row = el("div", "row person");
    const who = el("button", "who", author(p.nickname, p.pubkey));
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
    box.append(row);
  }
  if (!list.length) box.append(el("p", "muted small", "Nobody else seen here in the last 15 minutes."));
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
  const keepBottom = box.scrollHeight - box.scrollTop - box.clientHeight < 40;
  box.replaceChildren();
  const s = state.status;
  if (!s || !s.cells.length) return;
  const cells = state.selected ? [state.selected] : s.cells.map(c => c.geohash);
  const list = el("div", "messages");
  if (state.isolate) {
    const bar = el("div", "isolate");
    const back = el("button", null, "back to all");
    back.addEventListener("click", () => { state.isolate = null; renderFeed(); });
    bar.append(el("span", null, `Only #${state.isolate.slice(-4)} `), back);
    list.append(bar);
  }
  for (const m of visible(feed(state.messages, cells), state.local.mutes, state.isolate)) {
    const row = el("div", "msg" + (m.own ? " own" : ""));
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
  box.append(list, composer());
  if (keepBottom) box.scrollTop = box.scrollHeight;
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
  cell.addEventListener("change", () => { state.writeCell = cell.value; });
  const nick = field("input", "nickname");
  nick.placeholder = "nickname";
  nick.value = saved("nickname");
  nick.addEventListener("change", () => save("nickname", nick.value.trim()));
  const text = field("input", "feed-text", "text");
  text.placeholder = "message";
  const send = el("button", null, "Send");
  const result = field("span", "feed-result", "warn small");
  const go = async () => {
    const value = live(text).value;
    if (!value.trim()) return;
    send.disabled = true;
    const { ok, data } = await command("api/send", {
      text: value, cell: cell.value, nickname: live(nick).value.trim() || null,
    });
    send.disabled = false;
    if (ok) live(text).value = ""; else live(result).textContent = (data && data.message) || "refused";
  };
  send.addEventListener("click", go);
  text.addEventListener("keydown", e => { if (e.key === "Enter") go(); });
  form.append(cell, nick, text, send, result);
  return form;
}

// --- Private messages ---------------------------------------------------------------

function openConvo(peer, cell) {
  state.convo = peer + "|" + cell;
  command("api/read", { peer });
  render();
}

function renderDms() { keep("dms", drawDms); }

function drawDms(box) {
  box.replaceChildren(el("h2", null, "Private messages"));
  const convos = conversations(state.dms);
  if (!convos.length && !state.convo) {
    box.append(el("p", "muted small", "Click a name in the feed to write to that person."));
    return;
  }
  const list = el("div", "convos");
  for (const c of convos.slice().reverse()) {
    const b = el("button", "convo" + (state.convo === c.key ? " on" : ""), `#${c.peer.slice(-4)} in #${c.cell}`);
    b.addEventListener("click", () => openConvo(c.peer, c.cell));
    list.append(b);
  }
  box.append(list);
  if (!state.convo) return;
  const [peer, cell] = state.convo.split("|");
  const convo = convos.find(c => c.key === state.convo);
  const thread = el("div", "thread");
  thread.append(el("div", "muted small", `with #${peer.slice(-4)} · your key of #${cell}`));
  for (const d of convo ? convo.messages : []) {
    const row = el("div", "msg" + (d.own ? " own" : ""));
    const body = el("div", "body");
    textNodes(body, d.content);
    row.append(el("span", "muted small", clock(d.created_at) + (d.own ? " " + statusMark(d.status) : "")), body);
    thread.append(row);
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
  const text = field("input", "dm-text|" + state.convo, "text");
  text.placeholder = "private message (255 bytes at most)";
  const send = el("button", null, "Send");
  const result = field("span", "dm-result|" + state.convo, "warn small");
  const go = async () => {
    const value = live(text).value;
    if (!value.trim()) return;
    const { ok, data } = await command("api/dm", { peer, cell, text: value });
    if (ok) live(text).value = ""; else live(result).textContent = (data && data.message) || "refused";
  };
  send.addEventListener("click", go);
  text.addEventListener("keydown", e => { if (e.key === "Enter") go(); });
  const form = el("div", "composer");
  form.append(text, send, result);
  box.append(thread, form);
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
    yes.addEventListener("click", () => groupOp({ op: "accept", group: inv.group, nickname: saved("nickname") }));
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
    const thread = el("div", "thread");
    for (const m of v.messages.filter(x => x.group === g.id)) {
      const row = el("div", "msg" + (m.own ? " own" : ""));
      const body = el("div", "body");
      textNodes(body, m.content);
      row.append(el("b", null, m.nickname || "?"), el("span", "muted small", " " + clock(m.created_at)), body);
      thread.append(row);
    }
    const text = field("input", "group-text|" + g.id, "text");
    text.placeholder = "message to the group";
    const send = el("button", null, "Send");
    const go = async () => {
      const value = live(text).value;
      if (!value.trim()) return;
      const { ok } = await groupOp({ op: "send", group: g.id, text: value, nickname: saved("nickname") });
      if (ok) live(text).value = "";
    };
    send.addEventListener("click", go);
    text.addEventListener("keydown", e => { if (e.key === "Enter") go(); });
    const form = el("div", "composer");
    form.append(text, send);
    const end = el("button", null, g.creator_is_me ? "destroy the group" : "leave the group");
    end.addEventListener("click", () => {
      if (confirm(g.creator_is_me ? `Destroy “${g.name}” for everyone?` : `Leave “${g.name}”?`)) {
        groupOp({ op: g.creator_is_me ? "destroy" : "leave", group: g.id });
        state.group = null;
      }
    });
    box.append(thread, form, end);
  }
  const name = field("input", "group-name");
  name.placeholder = "new group name";
  const create = el("button", null, "Create");
  create.addEventListener("click", async () => {
    const value = live(name).value;
    if (!value.trim()) return;
    const { ok } = await groupOp({ op: "create", name: value, nickname: saved("nickname") });
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
  const text = field("input", "preset-text", "text");
  text.placeholder = "text to keep";
  const add = el("button", null, "Save");
  add.addEventListener("click", async () => {
    const [n, t] = [live(name), live(text)];
    if (!t.value.trim()) return;
    const { ok } = await command("api/preset", { name: n.value, text: t.value, nickname: saved("nickname") || null });
    if (ok) { live(name).value = ""; live(text).value = ""; }
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
  const stext = field("input", "schedule-text", "text");
  stext.placeholder = "message to send later";
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
      cell: state.selected, nickname: saved("nickname") || null,
    });
    if (ok) { live(stext).value = ""; live(result).textContent = ""; }
    else live(result).textContent = (data && data.message) || "refused";
  });
  const sform = el("div", "composer");
  sform.append(stext, at, repeat, plan, result);
  box.append(sform, el("p", "muted small", "Scheduled messages go out while the console runs."));
}

function render() {
  updateTop();
  renderCells();
  renderFeed();
  renderDms();
  renderGroups();
  renderSaved();
}

function onState(d) {
  state.status = d.status;
  state.messages = d.messages;
  state.dms = d.dms;
  state.local = d.local;
  state.groups = d.groups;
  render();
  refreshPeople();
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
    renderCells(); renderFeed();
    if (m.cell === currentCell()) refreshPeople();
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
    if (!dm.own && state.convo === dm.peer + "|" + dm.cell) command("api/read", { peer: dm.peer });
    renderDms();
  });
  on("dm_status", s => {
    for (const d of state.dms) if (d.own && d.id === s.id) d.status = s.status;
    renderDms();
  });
  on("relay_text", t => { state.texts.push(t); renderCells(); });
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
