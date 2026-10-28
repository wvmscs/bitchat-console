-- SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
-- SPDX-License-Identifier: GPL-3.0-or-later

-- The user's nickname (lot 13, D32): one for every cell, kept by the console
-- rather than by the browser (whose storage depends on the page's origin).
CREATE TABLE setting (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
-- Cells where the nickname is not sent (no `n` tag).
CREATE TABLE anon_cell (
    geohash TEXT PRIMARY KEY,
    at      INTEGER NOT NULL
);
