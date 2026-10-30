// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

// Pure helpers of the console page (no DOM), checked with node:
//   node --test tests/js/

// The 9 cells come centre first, then N, NE, E, SE, S, SW, W, NW (as
// `with_neighbours`). Grid order, row by row: NW N NE / W C E / SW S SE.
export function gridOrder(cells) {
  if (cells.length !== 9) return cells.slice();
  return [8, 1, 2, 7, 0, 3, 6, 5, 4].map(i => cells[i]);
}

// "name#abcd" as the app shows a participant; "#abcd" without a name.
export function author(nickname, pubkey) {
  const suffix = "#" + (pubkey || "").slice(-4);
  return nickname ? nickname + suffix : suffix;
}

// "14:05" today, "27/10 14:05" before.
export function clock(unix, now = Date.now() / 1000) {
  const d = new Date(unix * 1000);
  const hm = d.toTimeString().slice(0, 5);
  const today = new Date(now * 1000).toDateString() === d.toDateString();
  return today ? hm : `${String(d.getDate()).padStart(2, "0")}/${String(d.getMonth() + 1).padStart(2, "0")} ${hm}`;
}

// Text split into plain parts and http(s) links: [{text}|{link}]. Links are
// shown, never fetched (no image is loaded).
export function linkify(text) {
  const out = [];
  const re = /\bhttps?:\/\/[^\s<>"']+/g;
  let last = 0;
  for (const m of text.matchAll(re)) {
    // Trailing punctuation is not part of the link.
    const url = m[0].replace(/[.,;:!?)\]]+$/, "");
    if (m.index > last) out.push({ text: text.slice(last, m.index) });
    out.push({ link: url });
    last = m.index + url.length;
  }
  if (last < text.length) out.push({ text: text.slice(last) });
  return out;
}

// A link that points to a picture (shown as a link, marked as an image).
export const isImage = url => /\.(png|jpe?g|gif|webp|avif|heic)(\?.*)?$/i.test(url);

// Conversations of private messages: key "peer|cell", newest last.
export function conversations(dms) {
  const by = new Map();
  for (const d of dms) {
    const key = d.peer + "|" + d.cell;
    if (!by.has(key)) by.set(key, { key, peer: d.peer, cell: d.cell, messages: [], unread: 0 });
    const c = by.get(key);
    c.messages.push(d);
    if (!d.own && !d.read) c.unread += 1;
  }
  return [...by.values()].sort((a, b) =>
    (a.messages.at(-1)?.created_at ?? 0) - (b.messages.at(-1)?.created_at ?? 0));
}

// "✓" relayed, "✓✓" delivered, "✓✓ read" read; "…" before a relay answers.
export function statusMark(status) {
  return { relayed: "✓", delivered: "✓✓", read: "✓✓ read" }[status] ?? "…";
}

// The last nickname each key used in the channels (messages carry it,
// presences do not): key -> nickname.
export function nicknames(messagesByCell) {
  const last = new Map();
  for (const list of Object.values(messagesByCell)) {
    for (const m of list) {
      if (!m.nickname || m.own) continue;
      const was = last.get(m.pubkey);
      if (!was || (m.received ?? 0) >= was.received) last.set(m.pubkey, { nickname: m.nickname, received: m.received ?? 0 });
    }
  }
  return new Map([...last].map(([k, v]) => [k, v.nickname]));
}

// Unread private messages, all conversations.
export function unreadTotal(dms) {
  return dms.filter(d => !d.own && !d.read).length;
}

// Relay messages, the same text of the same relay once, with how many
// times and in which cells; the latest first.
export function groupTexts(texts) {
  const by = new Map();
  for (const t of texts) {
    const key = t.relay + "\n" + t.text;
    const g = by.get(key) || { relay: t.relay, text: t.text, count: 0, cells: [], last: 0 };
    g.count += 1;
    if (!g.cells.includes(t.cell)) g.cells.push(t.cell);
    g.last = Math.max(g.last, t.at ?? 0);
    by.set(key, g);
  }
  return [...by.values()].sort((a, b) => b.last - a.last);
}

// Messages of the selected cells, oldest first, without duplicates.
export function feed(messagesByCell, cells) {
  const seen = new Set();
  const out = [];
  for (const c of cells) {
    for (const m of messagesByCell[c] || []) {
      if (!seen.has(m.id)) {
        seen.add(m.id);
        out.push(m);
      }
    }
  }
  // Arrival order (the server's rank), so that nothing shown moves.
  return out.sort((a, b) => (a.received ?? 0) - (b.received ?? 0) || a.created_at - b.created_at);
}

// Relays of a cell that are connected, out of all.
export function relayCount(cell) {
  const up = cell.relays.filter(r => r.state === "connected").length;
  return `${up}/${cell.relays.length}`;
}

// People of a cell as listed: everyone but me, by the 4 last hex digits of
// their key (as the app shows them; a stable order), the muted after.
export function people(list, me, mutes) {
  const key = p => p.pubkey.slice(-4) + p.pubkey;
  const others = list.filter(p => p.pubkey !== me)
    .sort((a, b) => (key(a) < key(b) ? -1 : key(a) > key(b) ? 1 : 0));
  return [...others.filter(p => !mutes.includes(p.pubkey)), ...others.filter(p => mutes.includes(p.pubkey))];
}

// "14:05 · teleported · 3 msgs", or "· presence" for someone who only showed up.
export function personNote(p, now) {
  return clock(p.last_seen, now) + (p.teleport ? " · teleported" : "")
    + (p.messages ? ` · ${p.messages} msg${p.messages > 1 ? "s" : ""}` : " · presence");
}

// Messages to show: without muted keys; only one key when isolated.
export function visible(messages, mutes, isolate) {
  if (isolate) return messages.filter(m => m.pubkey === isolate);
  const muted = new Set(mutes);
  return messages.filter(m => !muted.has(m.pubkey));
}

// "every day", "every hour", "every 15 min", "once".
export function repeatLabel(secs) {
  if (!secs) return "once";
  if (secs % 86400 === 0) return secs === 86400 ? "every day" : `every ${secs / 86400} days`;
  if (secs % 3600 === 0) return secs === 3600 ? "every hour" : `every ${secs / 3600} h`;
  return `every ${Math.round(secs / 60)} min`;
}

// `<input type="datetime-local">` value -> Unix time (local time zone).
export function unixFromLocal(value) {
  const t = new Date(value).getTime();
  return Number.isFinite(t) ? Math.floor(t / 1000) : null;
}

const BASE32 = "0123456789bcdefghjkmnpqrstuvwxyz";

// [[south, west], [north, east]] of a geohash, or null.
export function bbox(geohash) {
  const lat = [-90, 90], lon = [-180, 180];
  let even = true;
  for (const ch of String(geohash).toLowerCase()) {
    const v = BASE32.indexOf(ch);
    if (v < 0) return null;
    for (let b = 4; b >= 0; b--) {
      const r = even ? lon : lat;
      const mid = (r[0] + r[1]) / 2;
      if ((v >> b) & 1) r[0] = mid; else r[1] = mid;
      even = !even;
    }
  }
  return geohash ? [[lat[0], lon[0]], [lat[1], lon[1]]] : null;
}

// Room left on the map beside the console's panel (its rectangle in the
// window, or null when folded) and the map's own panel (on the right, or
// null): Leaflet's paddings for fitting into it.
export function freeRoom(width, height, panel, mapPanel = null, margin = 20) {
  // The map's panel counts when it is a column, not a folded strip.
  const right = mapPanel && mapPanel.bottom - mapPanel.top > 120 ? width - mapPanel.left + margin : margin;
  if (!panel) return { topLeft: [margin, margin], bottomRight: [right, margin] };
  const beside = (width - panel.right - right + margin) * height;
  const above = width * panel.top;
  return beside >= above
    ? { topLeft: [panel.right + margin, margin], bottomRight: [right, margin] }
    : { topLeft: [margin, margin], bottomRight: [margin, height - panel.top + margin] };
}

// The area the map watches for a console that follows `center`: its parent
// (u09tv -> u09t), but never shorter than a channel of level 2.
export function mapArea(center) {
  return center.length > 2 ? center.slice(0, -1) : center;
}

// Cells whose messages the feed shows: the one chosen, else the 9, or only
// the centre when the neighbours are muted.
export function feedCells(cells, center, selected, muteNeighbours) {
  if (selected) return [selected];
  return muteNeighbours ? cells.filter(c => c === center) : cells;
}

// Lot 15: someone not seen in their cell (presence or message) for
// `present.departure_secs` has left. `present` is `GET /api/present`; none
// yet: nobody has left. `also`: another time they were active (a private
// message from them).
export function hasLeft(present, cell, pubkey, now, also = 0) {
  if (!present) return false;
  const seen = present.cells?.[cell]?.[pubkey] ?? 0;
  return now - Math.max(seen, also) > present.departure_secs;
}

// A conversation is over when the other side has left its cell and has
// written nothing for as long; never while a message is unread.
export function convoOver(convo, present, now) {
  if (convo.unread) return false;
  const last = Math.max(0, ...convo.messages.filter(d => !d.own).map(d => d.created_at));
  return hasLeft(present, convo.cell, convo.peer, now, last);
}

// What the cell field holds, as sent to the server (lot 19): `#U09TV` and
// `u09tv` are the cell `u09tv`; a place or an IP address stays as typed.
export function typedCell(text) {
  const t = text.trim();
  const g = t.replace(/^#\s*/, "").toLowerCase();
  return /^[0-9b-hjkmnp-z]{1,12}$/.test(g) ? g : t;
}
