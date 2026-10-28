// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

// The console over the map of bitchat-geohash (lot 9): a floating panel
// with the console page in a frame, and "Follow in console" in the menu of
// a cell of the map. Loaded by the console's relay of the map page. The
// cells the console shows are outlined on the map, which fits them into the
// room the panel leaves.

import { bbox, freeRoom, mapArea } from "/console/assets/view.js";

const box = document.createElement("div");
box.id = "bc-console";
const bar = document.createElement("div");
bar.className = "bc-bar";
const title = document.createElement("span");
title.textContent = "BitChat console";
const fold = document.createElement("button");
fold.textContent = "–";
fold.title = "fold";
const frame = document.createElement("iframe");
frame.src = "/console/";
frame.title = "BitChat console";
fold.addEventListener("click", () => {
  const folded = box.classList.toggle("folded");
  fold.textContent = folded ? "+" : "–";
});
bar.append(title, fold);
box.append(bar, frame);
document.body.append(box);

function teleported() {
  try { return localStorage.getItem("teleport") !== "0"; } catch { return true; }
}

async function follow(geohash, button) {
  button.disabled = true;
  const r = await fetch("/console/api/area", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ geohash, teleport: teleported() }),
  }).catch(() => null);
  button.disabled = false;
  if (r && r.ok) {
    button.textContent = `Following #${geohash} in the console`;
    box.classList.remove("folded");
    fold.textContent = "–";
  } else {
    let message = "refused";
    try { message = (await r.json()).message || message; } catch { /* not JSON */ }
    button.textContent = message;
  }
}

// The map draws a menu (`.menu`, header `#geohash · level …`) when a cell is
// clicked: add the button to it.
new MutationObserver(() => {
  for (const menu of document.querySelectorAll(".leaflet-popup .menu")) {
    if (menu.querySelector(".bc-follow")) continue;
    const head = menu.querySelector(".menu-head b");
    if (!head) continue;
    const geohash = head.textContent.replace(/^#/, "").trim();
    const b = document.createElement("button");
    b.className = "bc-follow";
    b.textContent = "Follow in console";
    b.addEventListener("click", () => follow(geohash, b));
    menu.insertBefore(b, menu.querySelector(".result"));
  }
}).observe(document.body, { childList: true, subtree: true });

// The console's cells on the map: the selected one, or the 9.
let outline = null;

function showView(v) {
  const map = window.__bitchatMap;
  if (!map || !window.L) return;
  if (outline) outline.remove();
  // Their own pane, deaf to the pointer: an SVG drawn over the map's own
  // would take its hovers and clicks, even where it draws nothing.
  if (!map.getPane("bc-outline")) {
    const pane = map.createPane("bc-outline");
    pane.style.pointerEvents = "none";
    pane.style.zIndex = 450;
  }
  const cells = v.selected ? [v.selected] : v.cells;
  const boxes = cells.map(c => [c, bbox(c)]).filter(([, b]) => b);
  if (!boxes.length) return;
  outline = L.layerGroup(boxes.map(([c, b]) => L.rectangle(b, {
    pane: "bc-outline", interactive: false, fill: false, dashArray: c === v.center || v.selected ? null : "4 4",
    weight: c === v.center || v.selected ? 3 : 1.5, color: "#0a66c2",
  }))).addTo(map);
  // The map shows the level of the console's cell (its own `#level=`).
  mapLevel((v.selected || v.center).length);
  if (!v.fit) return;
  // After the map has changed its level (it may frame on its own).
  setTimeout(() => fit(map, boxes), 60);
}

function mapLevel(level) {
  const h = new URLSearchParams(location.hash.slice(1));
  if (h.get("show") === "meshes" || h.get("level") === String(level)) return;
  h.set("level", String(level));
  location.hash = h.toString();
}

function fit(map, boxes) {
  const r = box.classList.contains("folded") ? null : box.getBoundingClientRect();
  const size = map.getSize();
  const p = document.getElementById("panel")?.getBoundingClientRect();
  const room = freeRoom(size.x, size.y, r && { right: r.right, top: r.top },
    p && p.width ? { left: p.left, top: p.top, bottom: p.bottom } : null);
  map.fitBounds(L.latLngBounds(boxes.map(([, b]) => b)), {
    paddingTopLeft: room.topLeft, paddingBottomRight: room.bottomRight, maxZoom: 17,
  });
}

// The map watches the area around the console's cell (its parent): when
// the console moves, the map moves with it, with all its own functions
// (bitchat-geohash `POST /api/area`, lot 19). Then the console's cells are
// framed again, after the map's own framing of its new area.
let synced = null;
let lastView = null;
// The map follows the console's centre when it moves, not its selection:
// an area chosen by double-click stays until the centre moves.
let lastCenter = null;

// `force`: asked by the user (double-click on a cell of the console), even
// if the console's centre did not move.
async function followArea(v, force = false) {
  const area = mapArea(v.center);
  if (area === synced && !force) return;
  synced = area;
  title.textContent = `BitChat console · map → #${area}…`;
  const r = await fetch("/api/area", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ geohash: area }),
  }).catch(() => null);
  let note = "";
  if (!r || !r.ok) {
    synced = null;
    try { note = ` · map: ${(await r.json()).message}`; } catch { note = " · map unreachable"; }
  }
  title.textContent = "BitChat console" + note;
  if (r && r.ok) setTimeout(() => showView({ ...v, fit: true }), 1500);
}

window.addEventListener("message", e => {
  if (e.origin !== location.origin || e.source !== frame.contentWindow) return;
  if (e.data && e.data.type === "bitchat-console:view") {
    lastView = e.data;
    showView(e.data);
    if (e.data.center !== lastCenter) {
      lastCenter = e.data.center;
      followArea(e.data);
    }
  } else if (e.data && e.data.type === "bitchat-console:area" && typeof e.data.cell === "string") {
    // The map's area for one of the console's cells; the console keeps its
    // own centre.
    const v = { ...(lastView || { cells: [e.data.cell] }), center: e.data.cell, selected: e.data.cell };
    followArea(v, true);
  }
});
