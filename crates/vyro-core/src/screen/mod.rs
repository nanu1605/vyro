//! The PC's screen on the phone (1.7): a session per phone allowed to see it, set up through the
//! relay and then direct (`rtc`), video from the monitor (`capture`, `frames`, `encode`), input
//! back from the phone (`input`, `inject`); the rules the app keeps around it (`share`).

#[cfg(windows)]
pub mod capture;
#[cfg(windows)]
pub mod encode;
pub mod frames;
#[cfg(windows)]
pub mod inject;
pub mod input;
#[cfg(windows)]
pub mod jpeg;
pub mod producer;
pub mod rtc;
pub mod share;
pub mod stun;
#[cfg(test)]
mod tests;
