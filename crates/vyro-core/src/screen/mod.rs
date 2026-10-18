//! The PC's screen on the phone (1.7): a session per phone allowed to see it, set up through the
//! relay and then direct (`rtc`), video from the monitor (`capture`, Windows), input back from
//! the phone (`input`).

pub mod input;
pub mod rtc;
pub mod stun;
#[cfg(test)]
mod tests;
