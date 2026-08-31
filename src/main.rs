//! A typing test for the terminal, in the spirit of monkeytype.
//!
//! The interesting parts are [`engine`], which models a test as a state machine
//! driven by an injected clock, and [`stats`], which scores one. Neither knows a
//! terminal exists.

mod engine;
mod rng;
mod stats;
mod words;
mod words_data;

fn main() {
    println!("typr {}", env!("CARGO_PKG_VERSION"));
}
