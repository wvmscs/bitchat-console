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
