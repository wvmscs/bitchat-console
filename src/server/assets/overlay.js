// The console over the map of bitchat-geohash (lot 9): a floating panel
// with the console page in a frame, and "Follow in console" in the menu of
// a cell of the map. Loaded by the console's relay of the map page. The
// cells the console shows are outlined on the map, which fits them into the
// room the panel leaves.

import { bbox, freeRoom } from "/console/assets/view.js";

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
  const cells = v.selected ? [v.selected] : v.cells;
  const boxes = cells.map(c => [c, bbox(c)]).filter(([, b]) => b);
  if (!boxes.length) return;
  outline = L.layerGroup(boxes.map(([c, b]) => L.rectangle(b, {
    interactive: false, fill: false, dashArray: c === v.center || v.selected ? null : "4 4",
    weight: c === v.center || v.selected ? 3 : 1.5, color: "#0a66c2",
  }))).addTo(map);
  if (!v.fit) return;
  const r = box.classList.contains("folded") ? null : box.getBoundingClientRect();
  const size = map.getSize();
  const p = document.getElementById("panel")?.getBoundingClientRect();
  const room = freeRoom(size.x, size.y, r && { right: r.right, top: r.top },
    p && p.width ? { left: p.left, top: p.top, bottom: p.bottom } : null);
  map.fitBounds(L.latLngBounds(boxes.map(([, b]) => b)), {
    paddingTopLeft: room.topLeft, paddingBottomRight: room.bottomRight, maxZoom: 17,
  });
}

window.addEventListener("message", e => {
  if (e.origin !== location.origin || e.source !== frame.contentWindow) return;
  if (e.data && e.data.type === "bitchat-console:view") showView(e.data);
});
