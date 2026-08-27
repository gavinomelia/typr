# typr

A monkeytype-style typing test for the terminal, in Elixir.

```
                                                                        
  23                                                        78 wpm      
                                                                        
  the quick brown fox jumps over the lazy dog while people think about  
  their own work and find the time to make something good with words    
  that come back again after every line you finish typing out loud      
                                                                        
                        tab restart · esc quit                          
```

Type the words as they appear. Correct letters brighten, wrong ones turn red,
and the graph afterwards shows where you sped up, slowed down and slipped.

## Running it

Needs Elixir and Erlang/OTP 26 or later.

```sh
mix escript.build     # produces ./typr
./typr                # a 30 second test
```

Put it on your path with `cp typr ~/.local/bin/` or similar.

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
./typr -w 25 --seed 1312
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
| `Typr.Engine` | The test as a pure state machine. Every keystroke is a function call and time is passed in, never read, so a whole test can be replayed deterministically in a unit test. |
| `Typr.Stats` | Scoring: wpm, raw, accuracy, consistency, per-letter tallies. |
| `Typr.Summary` | Aggregates a history into bests, averages, streaks and trouble keys. |
| `Typr.Render` | `state -> iodata`. Word wrap, the scrolling three-line window, the results graph. Pure. |
| `Typr.App` | The event loop: a reader process blocks on stdin while the loop wakes on its own timer. |
| `Typr.Terminal` | Raw mode, ANSI, terminal size. |
| `Typr.History` | Appends results to disk and reads them back. |

Scoring follows monkeytype's definitions: **wpm** counts correctly typed
characters (plus the spaces after correctly typed words) in units of five per
minute, **raw** ignores correctness, **accuracy** is judged at the moment each
key is pressed — fixing a typo doesn't buy it back — and **consistency** comes
from the coefficient of variation of per-second speed.

### Getting a keystroke out of a terminal

This is the one genuinely awkward part on the BEAM, and it's worth writing
down. The runtime hands child processes pipes rather than the terminal, and it
has no controlling terminal of its own, so `stty` reaches the wrong device
whether you point it at stdin or at `/dev/tty`. What works is asking the
operating system which terminal the runtime is attached to — `ps -o tty=` —
and pointing `stty` at that device by name.

OTP 28 has a proper answer, `shell:start_interactive({noshell, raw})`, and
`Typr.Terminal` uses it when available. On earlier releases that call is worse
than unsupported: the option isn't recognised, so it starts a full interactive
shell that silently eats every keystroke. Hence the version gate.

If keys aren't registering, `typr --doctor` reports what the terminal layer can
and cannot do here, including a two second input probe.

## Tests

```sh
mix test
```

109 tests, no external dependencies. The engine, scoring, aggregation and
rendering are all pure, so they're tested directly — including things that are
tedious to check by hand, like a timed test reporting exactly its limit when
the final tick lands late.

For the parts that need a real terminal there's a pty harness:

```sh
python3 test/support/drive.py --send 'the ' --wait 1 --send 'quick ' -- ./typr -w 5 --seed 42
```

It allocates a pty with a known window size, sends keystrokes with delays, and
replays the escape sequences onto a character grid so you can see the final
screen as text.

## Packaging

`escript` was chosen over a Mix release for distribution: one file, and the
only requirement on the far end is an Erlang runtime. If you want a binary with
no runtime dependency at all, [Burrito](https://github.com/burrito-elixir/burrito)
is the usual next step.
