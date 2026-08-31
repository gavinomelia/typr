//! A typing test for the terminal, in the spirit of monkeytype.
//!
//! Ported from the Elixir implementation module by module; this is the
//! foundation it is built on.

mod rng;
mod words;
mod words_data;

fn main() {
    println!("typr {}", env!("CARGO_PKG_VERSION"));
}
