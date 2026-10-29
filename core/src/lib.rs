//! Chip independent logic of the Luftpumpe, which can be tested on the host

#![cfg_attr(not(test), no_std)]

pub mod modbus;
pub mod registers;
pub mod settings;

/// Interval in ms in which the registers of the heat pump are read (and the state is published)
pub const POLL_INTERVAL: u64 = 30_000;
