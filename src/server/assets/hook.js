// Loaded in the map page of bitchat-geohash before its own script (a
// module, run after parsing): keeps the Leaflet map it makes, so that the
// console's overlay can show on it the cells the console shows.
if (window.L && L.Map) {
  L.Map.addInitHook(function () {
    if (!window.__bitchatMap) window.__bitchatMap = this;
  });
}
