# typr

A typing test for the terminal, in Rust. Inspired by https://monkeytype.com.

<img width="780" height="350" alt="Screenshot 2026-08-31 at 9 29 38 AM" src="https://github.com/user-attachments/assets/4b389c75-396f-454c-b347-580a69213d2e" />


Type the words as they appear. Correct letters brighten, wrong ones turn red,
and the graph afterwards shows where you sped up, slowed down and slipped.

## Running it

```sh
cargo build --release      # produces target/release/typr
./target/release/typr      # a 30 second test
```

Put it on your path with `cp target/release/typr ~/.local/bin/` or similar.

## Usage

```
modes
  -t, --time SECONDS      timed test (default: 30)
  -w, --words COUNT       fixed word-count test
  -q, --quote             type a sentence

text
  -l, --list NAME         english, english_extended
  -p, --punctuation       mix in punctuation and capitals
  -n, --numbers           mix in numbers

display
      --theme NAME        default, matrix, mono, ocean
      --width COLUMNS     text column width (default: 72)
      --seed N            reproducible words — same seed, same test
      --no-live-wpm       hide the live speed counter

behaviour
      --free-backspace    allow going back to correctly typed words

other
      --stats             bests, averages and trouble keys
      --doctor            report terminal capabilities
```

Keys: `tab` restarts, `r` repeats the same words from the results screen,
`ctrl+w` deletes a word, `esc` quits.

Two people can race the same words by agreeing on a seed:

```sh
typr -w 25 --seed 1312
```

## Stats

Every finished test is appended to `~/.config/typr/history.tsv`. Run
`typr --stats` to see bests, averages, trends and the letters you get wrong
most often.

## Tests

```sh
cargo test
```

## History

This started as an Elixir escript and was rewritten in Rust for a
single, dependency-free binary. A `history.tsv` written by the Elixir
version loads here without conversion.
