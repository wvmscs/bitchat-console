// The console over the map of bitchat-geohash (lot 9): a floating panel
// with the console page in a frame, and "Follow in console" in the menu of
// a cell of the map. Loaded by the console's relay of the map page.

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
