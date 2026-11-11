//! What a hot-plug watch query has seen (1.4.1), so only a real change reads the list again.
//! Windows reports an object again whenever any of its properties change, and a connected
//! Bluetooth device's change all the time: each report read the whole list again (1.4.1 field
//! check: with a Bluetooth device connected, the Devices tab stayed on "Reading devices…").

use std::collections::BTreeMap;

/// One result of a watch query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seen {
    /// The first listing is complete: what comes after is a change.
    Listed,
    /// An object, with its connection.
    Added(Option<bool>),
    Updated(Option<bool>),
    Removed,
}

/// Each object's connection, as one watch query last reported it.
#[derive(Debug, Default)]
pub struct Connections {
    listed: bool,
    seen: BTreeMap<String, Option<bool>>,
}

impl Connections {
    pub const fn new() -> Self {
        Self {
            listed: false,
            seen: BTreeMap::new(),
        }
    }

    /// Notes one result for the object `id`: whether the list should be read again. Objects
    /// arriving and leaving count; an update counts only when the connection changed.
    pub fn note(&mut self, id: &str, what: Seen) -> bool {
        match what {
            Seen::Listed => {
                self.listed = true;
                false
            }
            Seen::Added(connected) => {
                self.seen.insert(id.to_owned(), connected);
                self.listed
            }
            Seen::Updated(connected) => {
                let was = self.seen.insert(id.to_owned(), connected);
                self.listed && was != Some(connected)
            }
            Seen::Removed => {
                self.seen.remove(id);
                self.listed
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_listing_reads_nothing() {
        let mut c = Connections::new();
        assert!(!c.note("a", Seen::Added(Some(true))));
        assert!(!c.note("b", Seen::Added(None)));
        assert!(!c.note("a", Seen::Updated(Some(false))));
        assert!(!c.note("", Seen::Listed));
        // What the listing ended with is the baseline.
        assert!(!c.note("a", Seen::Updated(Some(false))));
    }

    #[test]
    fn only_a_changed_connection_reads_again() {
        let mut c = Connections::new();
        c.note("headphones", Seen::Added(Some(true)));
        c.note("mouse", Seen::Added(Some(true)));
        c.note("", Seen::Listed);
        // A connected device reported again and again with nothing new: no reads.
        for _ in 0..50 {
            assert!(!c.note("headphones", Seen::Updated(Some(true))));
            assert!(!c.note("mouse", Seen::Updated(Some(true))));
        }
        // It disconnects, then connects again: one read each.
        assert!(c.note("headphones", Seen::Updated(Some(false))));
        assert!(!c.note("headphones", Seen::Updated(Some(false))));
        assert!(c.note("headphones", Seen::Updated(Some(true))));
        // The connection is no longer reported: that is a change too, once.
        assert!(c.note("mouse", Seen::Updated(None)));
        assert!(!c.note("mouse", Seen::Updated(None)));
    }

    #[test]
    fn devices_arriving_and_leaving_read_again() {
        let mut c = Connections::new();
        c.note("", Seen::Listed);
        assert!(c.note("speaker", Seen::Added(Some(false))));
        assert!(!c.note("speaker", Seen::Updated(Some(false))));
        assert!(c.note("speaker", Seen::Removed));
        // One never seen before, reported as an update.
        assert!(c.note("earbuds", Seen::Updated(Some(true))));
    }
}
