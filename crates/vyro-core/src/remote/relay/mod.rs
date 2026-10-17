//! Reach from anywhere (1.4): the PC keeps one outbound connection to a Vyro relay (a Cloudflare
//! Worker, `relay/` in this repository), and paired phones reach it there. What the PC and a
//! phone say to each other is sealed end to end; the relay sees only who is connected, when, and
//! how much.
//!
//! - [`seal`]: a phone's keys from its secret, and sealed frames.

pub mod seal;
