//! A typing test for the terminal, in the spirit of monkeytype.
//!
//! The interesting parts are [`engine`], which models a test as a state machine
//! driven by an injected clock, and [`stats`], which scores one. Neither knows a
//! terminal exists.

mod app;
mod cli;
mod datetime;
mod engine;
mod history;
mod render;
mod report;
mod rng;
mod stats;
mod summary;
mod terminal;
mod theme;
mod words;
mod words_data;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    std::process::exit(cli::main(&args));
}
