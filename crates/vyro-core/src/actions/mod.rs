//! Things a phone allowed to control the PC can ask of it (1.5), done by the app as the
//! signed-in user: end a process ([`process`]), shut down, restart, sleep, lock or sign out
//! ([`power`]), and wake another PC on the same network ([`wake`]). The rules (what is never
//! ended, what a power action needs) are pure and tested here; the Windows calls are small.

pub mod power;
pub mod process;
pub mod wake;

#[cfg(test)]
mod tests;
