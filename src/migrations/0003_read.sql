-- SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
-- SPDX-License-Identifier: GPL-3.0-or-later

-- Private messages already read (D29): their BitChat message ID (a random
-- UUID) and when, nothing else. Relays keep the envelopes; after a restart
-- they come back, and stay read instead of unread.
CREATE TABLE dm_read (
    id TEXT PRIMARY KEY,
    at INTEGER NOT NULL
);
