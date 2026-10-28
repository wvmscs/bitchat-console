// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

// The history page (lot 16): search, sort, select and delete what the
// console kept (D31), live. Contents only ever go through `textContent`.
import { author, clock } from "./view.js";

const PAGE = 200;
const $ = id => document.getElementById(id);
function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

const fields = ["cell", "kind", "text", "key", "from", "to", "order", "deleted"];
const state = { records: [], selected: new Set(), limit: PAGE, error: "" };

// `datetime-local` <-> Unix time, in the browser's time zone.
const toUnix = v => (v ? Math.floor(new Date(v).getTime() / 1000) : null);
const toLocal = t => {
  const d = new Date(t * 1000);
  const p = n => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}`;
};

// The filters from the address (`history?cell=u09tv&kind=dm`), and back.
function fromAddress() {
  const q = new URLSearchParams(location.search);
  for (const f of fields) {
    const v = q.get(f);
    if (v === null) continue;
    if (f === "deleted") $(f).checked = v === "true";
    else if (f === "from" || f === "to") $(f).value = toLocal(Number(v));
    else $(f).value = v;
  }
}

function params() {
  const q = new URLSearchParams();
  for (const f of ["cell", "kind", "text", "key", "order"]) {
    const v = $(f).value.trim();
    if (v && !(f === "order" && v === "new")) q.set(f, v);
  }
  for (const f of ["from", "to"]) {
    const t = toUnix($(f).value);
    if (t !== null) q.set(f, String(t));
  }
  if ($("deleted").checked) q.set("deleted", "true");
  return q;
}

async function load() {
  const q = params();
  history.replaceState(null, "", "history" + (q.size ? "?" + q : ""));
  q.set("limit", String(state.limit));
  try {
    const r = await fetch("api/history?" + q);
    const data = await r.json();
    if (!r.ok) { state.error = data.message || "refused"; state.records = []; }
    else { state.error = ""; state.records = data.records; }
  } catch {
    state.error = "console unreachable";
  }
  const ids = new Set(state.records.map(r => r.id));
  for (const id of [...state.selected]) if (!ids.has(id)) state.selected.delete(id);
  draw();
}

let soon = null;
const reload = (ms = 300) => { clearTimeout(soon); soon = setTimeout(load, ms); };

function where(r) {
  if (r.kind === "group") return "group " + (r.group_id || "").slice(0, 8);
  return "#" + (r.geohash || "?");
}

function row(r) {
  const tr = el("div", "row hrow" + (r.own ? " own" : "") + (r.deleted_at ? " left" : ""));
  const pick = el("input");
  pick.type = "checkbox";
  pick.checked = state.selected.has(r.id);
  pick.disabled = !!r.deleted_at;
  pick.setAttribute("aria-label", "select");
  pick.addEventListener("change", () => {
    if (pick.checked) state.selected.add(r.id); else state.selected.delete(r.id);
    drawActions();
  });
  const who = author(r.nickname, r.sender) + (r.own ? " (you)" : "")
    + (r.recipient ? " → " + author(null, r.recipient) : "");
  const meta = el("span", "muted small",
    ` ${clock(r.created_at)} · ${r.kind} · ${where(r)}${r.teleport ? " · teleported" : ""}`
    + (r.status ? " · " + r.status : "") + (r.deleted_at ? " · deleted " + clock(r.deleted_at) : ""));
  const whoEl = el("b", null, who);
  whoEl.title = r.sender + (r.recipient ? " → " + r.recipient : "");
  const body = el("div", "body", r.content ?? "(deleted)");
  tr.append(pick, whoEl, meta, body);
  return tr;
}

let actions = null;
function drawActions() {
  const n = state.selected.size;
  actions.replaceChildren();
  const all = el("button", null, "select all");
  all.addEventListener("click", () => {
    for (const r of state.records) if (!r.deleted_at) state.selected.add(r.id);
    draw();
  });
  const none = el("button", null, "none");
  none.addEventListener("click", () => { state.selected.clear(); draw(); });
  const del = el("button", null, n ? `delete ${n}` : "delete");
  del.disabled = !n;
  del.title = "here only: the content goes, the line stays, and the console stops showing it";
  del.addEventListener("click", async () => {
    if (!confirm(`Delete ${n} message${n > 1 ? "s" : ""} from the history?`)) return;
    const r = await fetch("api/history/delete", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ ids: [...state.selected] }),
    });
    if (r.ok) state.selected.clear();
    load();
  });
  actions.append(all, none, del);
}

function draw() {
  const box = $("list");
  const head = el("div", "row");
  head.append(el("b", null, state.error ? state.error : `${state.records.length} message${state.records.length === 1 ? "" : "s"}`));
  actions = el("span");
  drawActions();
  head.append(document.createTextNode(" "), actions);
  const list = el("div", "hlist");
  for (const r of state.records) list.append(row(r));
  box.replaceChildren(head, list);
  if (state.records.length >= state.limit) {
    const more = el("button", null, "more");
    more.addEventListener("click", () => { state.limit += PAGE; load(); });
    box.append(more);
  }
}

// Live: what the console writes, and changes made elsewhere (the cron, a
// tool), come in within seconds.
function connect() {
  const link = $("link");
  const source = new EventSource("api/events");
  source.addEventListener("open", () => { link.className = "link live"; link.textContent = "● live"; });
  for (const name of ["history", "message", "dm", "dms", "group_message"]) {
    source.addEventListener(name, () => reload(1000));
  }
  source.onerror = () => { link.className = "link reconnecting"; link.textContent = "● reconnecting…"; };
}

fromAddress();
for (const f of fields) {
  $(f).addEventListener(f === "text" || f === "cell" || f === "key" ? "input" : "change", () => {
    state.limit = PAGE;
    reload(f === "text" ? 400 : 100);
  });
}
load();
connect();
