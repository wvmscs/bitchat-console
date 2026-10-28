-- SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
-- SPDX-License-Identifier: GPL-3.0-or-later

-- bitchat-console message history (lot 14, D31). Read and changed by other
-- processes too: the daily cron (backup, rotation, purge, D33), a local AI.
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
-- `since` (Unix time, text): the start of this base. The console writes
-- nothing older, so that relays sending a message again cannot bring back
-- what a rotation or a purge took away. A rotation sets it to its own start.
CREATE TABLE message (
    id          INTEGER PRIMARY KEY,
    -- 'channel' (kind 20000), 'dm' (private message), 'group'.
    kind        TEXT NOT NULL CHECK (kind IN ('channel', 'dm', 'group')),
    -- channel: Nostr event ID; dm: BitChat message ID (UUID); group: group
    -- message ID.
    msg_id      TEXT NOT NULL,
    -- 1: sent by this console.
    own         INTEGER NOT NULL,
    -- channel: the cell; dm: the cell whose key the console uses with the
    -- other side; group: NULL.
    geohash     TEXT,
    group_id    TEXT,
    -- Hex keys: channel and dm, Nostr (x-only); group, Ed25519.
    sender      TEXT NOT NULL,
    -- dm: the other key; else NULL.
    recipient   TEXT,
    -- As sent (`n` tag, group member); NULL: none (private messages carry
    -- none).
    nickname    TEXT,
    -- channel: tagged `t=teleport`.
    teleport    INTEGER NOT NULL DEFAULT 0,
    -- NULL once deleted (deleted_at set): the row stays, so that the message
    -- is not written again when a relay sends it again.
    content     TEXT,
    -- Unix time: of the event (never in the future), and of its arrival.
    created_at  INTEGER NOT NULL,
    received_at INTEGER NOT NULL,
    -- dm sent: 'sent', 'relayed', 'delivered', 'read'; dm received: 'read'
    -- once read; else NULL.
    status      TEXT,
    deleted_at  INTEGER,
    UNIQUE (kind, msg_id)
);
CREATE INDEX message_cell ON message (geohash, created_at);
CREATE INDEX message_time ON message (created_at);
CREATE INDEX message_group ON message (group_id, created_at);
