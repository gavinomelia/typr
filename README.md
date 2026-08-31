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

The binary is self-contained: about 500 KB, one dependency (`libc`), and no
runtime to install on the far end. Anyone with a Mac can run it without
installing Rust.

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

Every finished test is appended to `~/.config/typr/history.tsv`, so averages,
trends and per-letter weaknesses can be recomputed later — including
statistics that didn't exist when the result was recorded. It's a plain
tab-separated file; sort it, grep it, throw it away.

```
$ typr --stats

typr — 142 tests · 1h 12m typing · 14,204 words

  best            87 wpm    time-30-english, yesterday
  average         72 wpm
  last 10         78 wpm    +6 on the 10 before
  accuracy        96.4%
  consistency     81.2%
  practised       12 days   3 day streak
  last test       today

  recent          ▃▄▂▅▆▅▇█▆▇▅▆▇█▇▆█▇██   62–87 wpm

by test
  time-30-english            58 tests   best  87   avg  73
  words-25-english-punct     12 tests   best  79   avg  68

trouble keys
  e     93.1%   210 missed of 3,050
  ;     88.4%    31 missed of 267

most often typed instead
  e → r   38
  n → m   21
```

Mistakes are blamed on the letter you *should* have typed, not the one you
hit, which is what makes the list actionable. Letters you've barely typed are
excluded so a single slip on a rare letter can't top the table.

Each result screen also shows how the run compares to your best and average
for that exact configuration, plus the letters that let you down in that test.

## How it works

The interesting part is the split between a pure core and a thin IO shell.

| Module | Job |
| --- | --- |
| `engine` | The test as a state machine. Every keystroke is a method call and time is passed in, never read, so a whole test can be replayed deterministically in a unit test. |
| `stats` | Scoring: wpm, raw, accuracy, consistency, per-letter tallies. |
| `summary` | Aggregates a history into bests, averages, streaks and trouble keys. |
| `render` | `state -> String`. Word wrap, the scrolling three-line window, the results graph. Pure. |
| `app` | The event loop: a reader thread blocks on stdin while the loop wakes on its own timer. |
| `terminal` | Raw mode, ANSI, terminal size. |
| `history` | Appends results to disk and reads them back. |
| `rng` | A seedable SplitMix64, so `--seed` is reproducible. |

Scoring follows monkeytype's definitions: **wpm** counts correctly typed
characters (plus the spaces after correctly typed words) in units of five per
minute, **raw** ignores correctness, **accuracy** is judged at the moment each
key is pressed — fixing a typo doesn't buy it back — and **consistency** comes
from the coefficient of variation of per-second speed.

### Getting a keystroke out of a terminal

File descriptor 0 is the terminal, so raw mode is one `tcsetattr` and the size
is one `ioctl`. The reader thread blocks on stdin and posts each character to a
channel; the main loop's `recv_timeout` either hands over the next keystroke or
says the frame is due, which is what lets the clock keep running while nothing
is being typed.

Raw mode and the alternate screen are both RAII guards, so they are undone even
if the program panics — otherwise a crash would leave the shell with no echo.

If keys aren't registering, `typr --doctor` reports what the terminal layer can
and cannot do here.

## Tests

```sh
cargo test
```

175 tests, no dependencies beyond `libc`. The engine, scoring, aggregation and
rendering are all pure, so they're tested directly — including things that are
tedious to check by hand, like a timed test reporting exactly its limit when
the final tick lands late.

For the parts that need a real terminal there's a pty harness:

```sh
python3 test/support/drive.py --send 'the ' --wait 1 --send 'quick ' \
  -- ./target/release/typr -w 5 --seed 42
```

It allocates a pty with a known window size, sends keystrokes with delays, and
replays the escape sequences onto a character grid so you can see the final
screen as text. Point `XDG_CONFIG_HOME` at a scratch directory when driving it,
or the runs land in your real history.

The code is kept clean under `cargo clippy` and formatted with `cargo fmt`.

## History

This started as an Elixir escript. That version needed an Erlang runtime on the
target machine, which macOS has never shipped, so it was rewritten in Rust to
get a single binary that runs on a stock Mac. The scoring, word lists, file
format and screen layout are unchanged — a `history.tsv` written by the Elixir
version loads here without conversion.
