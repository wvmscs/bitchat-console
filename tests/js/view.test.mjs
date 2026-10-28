// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

// Pure helpers of the console page. Run: node --test tests/js/
import { test } from "node:test";
import assert from "node:assert/strict";
import * as v from "../../src/server/assets/view.js";

test("grid of the 9 cells", () => {
  const cells = ["C", "N", "NE", "E", "SE", "S", "SW", "W", "NW"];
  assert.deepEqual(v.gridOrder(cells), ["NW", "N", "NE", "W", "C", "E", "SW", "S", "SE"]);
  assert.deepEqual(v.gridOrder(["C", "E"]), ["C", "E"]);
});

test("authors as the app shows them", () => {
  assert.equal(v.author("alice", "ab".repeat(31) + "1234"), "alice#1234");
  assert.equal(v.author(null, "ab".repeat(31) + "1234"), "#1234");
});

test("links are found, never fetched", () => {
  assert.deepEqual(v.linkify("voir https://x.org/a.png, merci"),
    [{ text: "voir " }, { link: "https://x.org/a.png" }, { text: ", merci" }]);
  assert.deepEqual(v.linkify("javascript:alert(1)"), [{ text: "javascript:alert(1)" }]);
  assert.ok(v.isImage("https://x.org/a.PNG?x=1"));
  assert.ok(!v.isImage("https://x.org/a.html"));
});

test("conversations and feed", () => {
  const dms = [
    { peer: "p1", cell: "u09tv", created_at: 2 },
    { peer: "p2", cell: "u09tv", created_at: 1 },
    { peer: "p1", cell: "u09tv", created_at: 3 },
  ];
  const c = v.conversations(dms);
  assert.deepEqual(c.map(x => [x.peer, x.messages.length]), [["p2", 1], ["p1", 2]]);
  const f = v.feed({ a: [{ id: "1", created_at: 5 }], b: [{ id: "2", created_at: 3 }, { id: "1", created_at: 5 }] }, ["a", "b"]);
  assert.deepEqual(f.map(m => m.id), ["2", "1"]);
  assert.equal(v.statusMark("delivered"), "✓✓");
  assert.equal(v.relayCount({ relays: [{ state: "connected" }, { state: "connecting" }] }), "1/2");
});

test("muted keys and isolation (lot 8)", () => {
  const ms = [{ pubkey: "a" }, { pubkey: "b" }, { pubkey: "a" }];
  assert.deepEqual(v.visible(ms, ["a"], null).map(m => m.pubkey), ["b"]);
  assert.deepEqual(v.visible(ms, ["a"], "a").map(m => m.pubkey), ["a", "a"]);
  assert.equal(v.repeatLabel(null), "once");
  assert.equal(v.repeatLabel(86400), "every day");
  assert.equal(v.repeatLabel(7200), "every 2 h");
  assert.equal(v.repeatLabel(900), "every 15 min");
  assert.equal(v.unixFromLocal("nonsense"), null);
  assert.ok(v.unixFromLocal("2026-10-28T10:00") > 1_800_000_000 - 1e9);
});

test("people of a cell", () => {
  const list = [
    { pubkey: "aa01", nickname: "bob", last_seen: 100, teleport: true, messages: 2 },
    { pubkey: "me00", nickname: null, last_seen: 90, teleport: false, messages: 1 },
    { pubkey: "cc03", nickname: null, last_seen: 80, teleport: false, messages: 0 },
    { pubkey: "dd04", nickname: "eve", last_seen: 70, teleport: false, messages: 1 },
  ];
  assert.deepEqual(v.people(list, "me00", ["aa01"]).map(p => p.pubkey), ["cc03", "dd04", "aa01"]);
  // By the last 4 hex digits, whatever the server's order.
  const keys = ["ff0002", "000003", "aa0001"].map(pubkey => ({ pubkey }));
  assert.deepEqual(v.people(keys, null, []).map(p => p.pubkey), ["aa0001", "ff0002", "000003"]);
  assert.deepEqual(v.people(keys.slice().reverse(), null, []).map(p => p.pubkey), ["aa0001", "ff0002", "000003"]);
  const now = 100;
  assert.match(v.personNote(list[0], now), / · teleported · 2 msgs$/);
  assert.match(v.personNote(list[3], now), /\d · 1 msg$/);
  assert.match(v.personNote(list[2], now), / · presence$/);
});

test("unread private messages", () => {
  const dms = [
    { peer: "p1", cell: "a", created_at: 1, own: false, read: false },
    { peer: "p1", cell: "a", created_at: 2, own: true, read: false },
    { peer: "p2", cell: "a", created_at: 3, own: false, read: true },
    { peer: "p2", cell: "a", created_at: 4, own: false, read: false },
  ];
  assert.deepEqual(v.conversations(dms).map(c => [c.peer, c.unread]), [["p1", 1], ["p2", 1]]);
  assert.equal(v.unreadTotal(dms), 2);
  assert.equal(v.statusMark("read"), "✓✓ read");
  assert.equal(v.statusMark(null), "…");
});

test("relay messages grouped", () => {
  const t = (relay, cell, text, at) => ({ relay, cell, text, at });
  const g = v.groupTexts([
    t("wss://a", "u09tv", "auth-required: x", 1),
    t("wss://b", "u09tv", "refused: y", 2),
    t("wss://a", "u09ty", "auth-required: x", 3),
    t("wss://a", "u09tv", "auth-required: x", 4),
  ]);
  assert.deepEqual(g.map(x => [x.relay, x.count, x.cells.join(","), x.last]),
    [["wss://a", 3, "u09tv,u09ty", 4], ["wss://b", 1, "u09tv", 2]]);
});

test("feed in arrival order", () => {
  // A late message with an old date stays where it arrived.
  const f = v.feed({ a: [{ id: "1", created_at: 50, received: 1 }, { id: "2", created_at: 10, received: 2 }],
    b: [{ id: "3", created_at: 60, received: 3 }] }, ["b", "a"]);
  assert.deepEqual(f.map(m => m.id), ["1", "2", "3"]);
});

test("nicknames known from the channels", () => {
  const n = v.nicknames({
    a: [{ pubkey: "k1", nickname: "bob", received: 1 }, { pubkey: "k2", nickname: null, received: 2 }],
    b: [{ pubkey: "k1", nickname: "bobby", received: 3 }, { pubkey: "me", nickname: "me", own: true, received: 4 }],
  });
  assert.equal(n.get("k1"), "bobby");
  assert.equal(n.has("k2") || n.has("me"), false);
});

test("geohash rectangles", () => {
  const [[s, w], [n, e]] = v.bbox("u09tv");
  // Paris, a level-5 cell: about 4.9 km by 4.9 km.
  assert.ok(s < 48.86 && n > 48.82 && w < 2.33 && e > 2.28, `${s} ${w} ${n} ${e}`);
  assert.ok(Math.abs((n - s) - 180 / 2 ** 12) < 1e-9 && Math.abs((e - w) - 360 / 2 ** 13) < 1e-9);
  assert.equal(v.bbox("u0a"), null);
  assert.equal(v.bbox(""), null);
});

test("room beside the panel", () => {
  // A panel at the bottom left of a wide window: fit to its right.
  assert.deepEqual(v.freeRoom(1400, 900, { right: 530, top: 260 }),
    { topLeft: [550, 20], bottomRight: [20, 20] });
  // A narrow window: fit above it.
  assert.deepEqual(v.freeRoom(600, 900, { right: 590, top: 400 }),
    { topLeft: [20, 20], bottomRight: [20, 520] });
  assert.deepEqual(v.freeRoom(600, 900, null), { topLeft: [20, 20], bottomRight: [20, 20] });
  // The map's panel on the right too: between the two.
  assert.deepEqual(v.freeRoom(1400, 900, { right: 530, top: 260 }, { left: 1030, top: 10, bottom: 320 }),
    { topLeft: [550, 20], bottomRight: [390, 20] });
  // Folded to a strip: it does not count.
  assert.deepEqual(v.freeRoom(1400, 900, { right: 530, top: 260 }, { left: 1030, top: 10, bottom: 60 }),
    { topLeft: [550, 20], bottomRight: [20, 20] });
});

test("the map's area for the console's cell", () => {
  assert.equal(v.mapArea("u09tv"), "u09t");
  assert.equal(v.mapArea("u09tvw"), "u09tv");
  assert.equal(v.mapArea("u0"), "u0");
});

test("neighbours muted", () => {
  const cells = ["u09tv", "u09wj", "u09ty"];
  assert.deepEqual(v.feedCells(cells, "u09tv", null, false), cells);
  assert.deepEqual(v.feedCells(cells, "u09tv", null, true), ["u09tv"]);
  // A neighbour chosen is shown, muted or not.
  assert.deepEqual(v.feedCells(cells, "u09tv", "u09wj", true), ["u09wj"]);
});

test("someone not seen for 10 min has left; a conversation then is over", () => {
  const { hasLeft, convoOver } = v;
  const present = { now: 10_000, departure_secs: 600, cells: { u09tv: { aa: 9_500 } } };
  assert.equal(hasLeft(null, "u09tv", "aa", 10_000), false, "no data yet: nobody left");
  assert.equal(hasLeft(present, "u09tv", "aa", 10_000), false);
  assert.equal(hasLeft(present, "u09tv", "aa", 10_200), true);
  assert.equal(hasLeft(present, "u09tv", "bb", 10_000), true, "never seen");
  assert.equal(hasLeft(present, "u09ty", "bb", 10_000, 9_900), false, "a private message counts");
  const convo = (unread, at) => ({ peer: "bb", cell: "u09ty", unread,
    messages: [{ own: false, created_at: at }, { own: true, created_at: 9_990 }] });
  assert.equal(convoOver(convo(0, 9_000), present, 10_000), true, "my own messages do not count");
  assert.equal(convoOver(convo(1, 9_000), present, 10_000), false, "unread: kept");
  assert.equal(convoOver(convo(0, 9_800), present, 10_000), false);
});
