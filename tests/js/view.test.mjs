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
