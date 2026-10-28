-- SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
-- SPDX-License-Identifier: GPL-3.0-or-later

-- Private groups (lot 10, D27): the creator-signed state as the app encodes
-- it (key included: the file is readable by the user only), and the pinned
-- Nostr address of the creator.
CREATE TABLE grp (
    group_id     TEXT PRIMARY KEY,
    state        BLOB NOT NULL,
    creator_addr TEXT NOT NULL
);
