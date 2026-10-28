// The console page (lot 7): the 9 cells, the feed, the composer, private
// messages. Live from the server's SSE stream. Strings from relays (texts,
// nicknames, relay messages) only ever go through `textContent`.

import { author, clock, conversations, feed, gridOrder, isImage, linkify, relayCount, statusMark }
  from "./view.js";

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
};

const saved = key => { try { return localStorage.getItem(key) || ""; } catch { return ""; } };
const save = (key, v) => { try { localStorage.setItem(key, v); } catch { /* private window */ } };

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
  const { ok, data } = await command("/api/area", { geohash, teleport: ui.teleport.checked });
  if (!ok) ui.error.textContent = (data && data.message) || "refused";
  state.selected = null;
}

function updateTop() {
  ui.link.className = "link " + state.link;
  ui.link.textContent = { live: "● live", connecting: "● connecting…", reconnecting: "● reconnecting…" }[state.link];
  const s = state.status;
  if (!s) return;
  if (s.center && document.activeElement !== ui.geohash) ui.geohash.value = s.center;
  ui.info.textContent = s.center
    ? ` #${s.center}${s.teleport ? " · teleported" : " · on site"} · ${s.connector}`
    : ` ${s.connector}`;
}

// --- Cells ----------------------------------------------------------------------

function renderCells() {
  const box = document.getElementById("cells");
  box.replaceChildren();
  const s = state.status;
  if (!s || !s.cells.length) {
    box.append(el("p", "muted", "Choose a geohash to follow it and its 8 neighbours."));
    return;
  }
  const all = el("button", "cell all" + (state.selected === null ? " on" : ""), "all 9 cells");
  all.addEventListener("click", () => { state.selected = null; render(); });
  const grid = el("div", "grid");
  for (const c of gridOrder(s.cells)) {
    const b = el("button", "cell" + (state.selected === c.geohash ? " on" : "")
      + (c.geohash === s.center ? " center" : ""));
    const n = (state.messages[c.geohash] || []).length;
    b.append(el("b", null, "#" + c.geohash), el("span", "muted", ` ${c.participants} people`),
      el("span", "muted small", ` ${n} msgs · relays ${relayCount(c)}`));
    b.addEventListener("click", () => { state.selected = c.geohash; render(); });
    grid.append(b);
  }
  box.append(all, grid);
  if (state.texts.length) {
    const log = el("details", "texts");
    log.append(el("summary", "muted", `relay messages (${state.texts.length})`));
    for (const t of state.texts.slice(-20)) {
      log.append(el("div", "small", `${t.relay.replace(/^wss?:\/\//, "")} #${t.cell}: ${t.text}`));
    }
    box.append(log);
  }
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

function renderFeed() {
  const box = document.getElementById("feed");
  const keepBottom = box.scrollHeight - box.scrollTop - box.clientHeight < 40;
  box.replaceChildren();
  const s = state.status;
  if (!s || !s.cells.length) return;
  const cells = state.selected ? [state.selected] : s.cells.map(c => c.geohash);
  const list = el("div", "messages");
  for (const m of feed(state.messages, cells)) {
    const row = el("div", "msg" + (m.own ? " own" : ""));
    const who = el("button", "who", author(m.nickname, m.pubkey));
    who.title = m.own ? "you" : "private message";
    if (!m.own) who.addEventListener("click", () => openConvo(m.pubkey, m.cell));
    const meta = el("span", "muted small",
      ` ${clock(m.created_at)} #${m.cell}${m.teleport ? " · teleported" : ""}`
      + (m.own ? ` · ${m.relays.length ? "✓ " + m.relays.length : "…"}` : ""));
    const body = el("div", "body");
    textNodes(body, m.content);
    row.append(who, meta, body);
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
  cell.value = state.selected || s.center;
  const nick = el("input");
  nick.placeholder = "nickname";
  nick.value = saved("nickname");
  nick.addEventListener("change", () => save("nickname", nick.value.trim()));
  const text = el("input", "text");
  text.placeholder = "message";
  const send = el("button", null, "Send");
  const result = el("span", "warn small");
  const go = async () => {
    if (!text.value.trim()) return;
    send.disabled = true;
    const { ok, data } = await command("/api/send", { text: text.value, cell: cell.value, nickname: nick.value.trim() || null });
    send.disabled = false;
    if (ok) text.value = ""; else result.textContent = (data && data.message) || "refused";
  };
  send.addEventListener("click", go);
  text.addEventListener("keydown", e => { if (e.key === "Enter") go(); });
  form.append(cell, nick, text, send, result);
  return form;
}

// --- Private messages ---------------------------------------------------------------

function openConvo(peer, cell) {
  state.convo = peer + "|" + cell;
  command("/api/read", { peer });
  render();
}

function renderDms() {
  const box = document.getElementById("dms");
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
  const text = el("input", "text");
  text.placeholder = "private message (255 bytes at most)";
  const send = el("button", null, "Send");
  const result = el("span", "warn small");
  const go = async () => {
    if (!text.value.trim()) return;
    const { ok, data } = await command("/api/dm", { peer, cell, text: text.value });
    if (ok) text.value = ""; else result.textContent = (data && data.message) || "refused";
  };
  send.addEventListener("click", go);
  text.addEventListener("keydown", e => { if (e.key === "Enter") go(); });
  const form = el("div", "composer");
  form.append(text, send, result);
  box.append(thread, form);
}

// --- Rendering and live stream ------------------------------------------------------

function render() {
  updateTop();
  renderCells();
  renderFeed();
  renderDms();
}

function onState(d) {
  state.status = d.status;
  state.messages = d.messages;
  state.dms = d.dms;
  render();
}

let source = null;
let retry = null;

function connect() {
  if (source) source.close();
  clearTimeout(retry);
  source = new EventSource("/api/events");
  const on = (name, f) => source.addEventListener(name, e => { f(JSON.parse(e.data)); });
  on("state", d => { state.link = "live"; onState(d); });
  on("status", s => { state.status = s; updateTop(); renderCells(); });
  on("message", m => {
    const list = state.messages[m.cell] || (state.messages[m.cell] = []);
    if (!list.some(x => x.id === m.id)) list.push(m);
    renderCells(); renderFeed();
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
  });
  on("dm", dm => {
    state.dms.push(dm);
    if (!dm.own && state.convo === dm.peer + "|" + dm.cell) command("/api/read", { peer: dm.peer });
    renderDms();
  });
  on("dm_status", s => {
    for (const d of state.dms) if (d.own && d.id === s.id) d.status = s.status;
    renderDms();
  });
  on("relay_text", t => { state.texts.push(t); renderCells(); });
  source.onerror = () => {
    state.link = "reconnecting";
    updateTop();
    if (source.readyState === EventSource.CLOSED) retry = setTimeout(connect, 3000);
  };
}

buildTop();
render();
connect();
