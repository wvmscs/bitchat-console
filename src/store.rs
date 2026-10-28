//! Where received and sent messages live (D16): in memory, per cell, bounded
//! in number and age, lost when the console stops. Behind [`MessageStore`]
//! so that another store (an optional local history, Redis if several
//! processes ever share the feed) can replace it.

use std::collections::{HashMap, VecDeque};

use serde::Serialize;

/// A channel message (kind 20000), as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Message {
    pub id: String,
    /// The cell (`g` tag).
    pub cell: String,
    pub pubkey: String,
    /// `n` tag, if any.
    pub nickname: Option<String>,
    pub content: String,
    pub created_at: i64,
    /// `t` = `teleport`: the sender says it is not in the cell.
    pub teleport: bool,
    /// Sent by this console.
    pub own: bool,
    /// Relays that delivered (or accepted) it.
    pub relays: Vec<String>,
    /// Arrival rank in this console, across cells: the feed's order, which
    /// a late or misdated event cannot reshuffle.
    pub received: u64,
}

/// Storage of the messages of the followed cells.
pub trait MessageStore: Send {
    /// Adds a message; false if its id is already there.
    fn add(&mut self, message: Message) -> bool;
    /// Notes that another relay delivered (or accepted) a known message.
    fn add_relay(&mut self, id: &str, relay: &str);
    /// Messages of a cell, in arrival order.
    fn cell(&self, cell: &str) -> Vec<Message>;
    /// Drops what is older than `before` (Unix time).
    fn purge(&mut self, before: i64);
    /// Drops a message (deleted in the history, lot 16); false if unknown.
    fn remove(&mut self, id: &str) -> bool;
    /// Changes the content of a message (edited in the history); false if
    /// unknown or unchanged.
    fn set_content(&mut self, id: &str, content: &str) -> bool;
}

/// In memory: at most `per_cell` messages per cell.
pub struct MemoryStore {
    per_cell: usize,
    cells: HashMap<String, VecDeque<Message>>,
}

impl MemoryStore {
    pub fn new(per_cell: usize) -> Self {
        MemoryStore {
            per_cell: per_cell.max(1),
            cells: HashMap::new(),
        }
    }
}

impl MessageStore for MemoryStore {
    fn add(&mut self, message: Message) -> bool {
        let q = self.cells.entry(message.cell.clone()).or_default();
        if q.iter().any(|m| m.id == message.id) {
            return false;
        }
        // Arrival order: what is shown does not move when a late one comes.
        q.push_back(message);
        while q.len() > self.per_cell {
            q.pop_front();
        }
        true
    }

    fn add_relay(&mut self, id: &str, relay: &str) {
        for q in self.cells.values_mut() {
            if let Some(m) = q.iter_mut().find(|m| m.id == id) {
                if !m.relays.iter().any(|r| r == relay) {
                    m.relays.push(relay.to_owned());
                }
                return;
            }
        }
    }

    fn cell(&self, cell: &str) -> Vec<Message> {
        self.cells
            .get(cell)
            .map(|q| q.iter().cloned().collect())
            .unwrap_or_default()
    }

    fn purge(&mut self, before: i64) {
        for q in self.cells.values_mut() {
            q.retain(|m| m.created_at >= before);
        }
        self.cells.retain(|_, q| !q.is_empty());
    }

    fn remove(&mut self, id: &str) -> bool {
        for q in self.cells.values_mut() {
            if let Some(i) = q.iter().position(|m| m.id == id) {
                q.remove(i);
                return true;
            }
        }
        false
    }

    fn set_content(&mut self, id: &str, content: &str) -> bool {
        for q in self.cells.values_mut() {
            if let Some(m) = q.iter_mut().find(|m| m.id == id) {
                if m.content == content {
                    return false;
                }
                m.content = content.to_owned();
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(id: &str, cell: &str, at: i64) -> Message {
        Message {
            id: id.into(),
            cell: cell.into(),
            pubkey: "p".into(),
            nickname: None,
            content: "hi".into(),
            created_at: at,
            teleport: false,
            own: false,
            relays: vec!["wss://a".into()],
            received: at as u64,
        }
    }

    #[test]
    fn bounded_ordered_deduplicated() {
        let mut s = MemoryStore::new(3);
        assert!(s.add(msg("1", "u09tv", 10)));
        assert!(!s.add(msg("1", "u09tv", 10)));
        assert!(s.add(msg("3", "u09tv", 30)));
        assert!(s.add(msg("2", "u09tv", 20)));
        assert!(s.add(msg("4", "u09tv", 40)));
        // In arrival order, the oldest arrivals dropped first.
        let ids: Vec<String> = s.cell("u09tv").into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["3", "2", "4"]);
        s.add_relay("3", "wss://b");
        s.add_relay("3", "wss://b");
        assert_eq!(s.cell("u09tv")[0].relays, ["wss://a", "wss://b"]);
        s.purge(35);
        assert_eq!(s.cell("u09tv").len(), 1);
        assert!(s.cell("u09tw").is_empty());
    }
}
