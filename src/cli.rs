//! Command line entry point.
//!
//! The options are few and the shapes are simple, so the parsing is done by
//! hand rather than pulled in from a crate. Unknown flags are refused rather
//! than ignored: a mistyped option should not silently give you a different
//! test from the one you asked for.

use crate::app::{self, Options};
use crate::engine::{Backtrack, Mode};
use crate::history::{Config, History};
use crate::report;
use crate::stats::Stats;
use crate::summary::{Summary, TroubleOptions};
use crate::terminal;
use crate::theme::Theme;
use crate::words::{self, Decoration};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_SECONDS: i64 = 30;
const DEFAULT_WIDTH: usize = 72;
const MIN_WIDTH: usize = 20;

/// What the parsed arguments asked us to do.
enum Command {
    Run(Box<Options>),
    Print(String),
    Diagnose,
    Stats,
}

/// Runs `typr`, returning the process exit code.
pub fn main(args: &[String]) -> i32 {
    match parse(args) {
        Err(message) => {
            eprintln!("typr: {message}");
            1
        }
        Ok(Command::Print(text)) => {
            println!("{text}");
            0
        }
        Ok(Command::Stats) => {
            let results = History::open().load();
            let summary = Summary::build(&results, TroubleOptions::default());
            print!("{}", report::render(&summary));
            0
        }
        Ok(Command::Diagnose) => {
            print!("{}", diagnosis());
            0
        }
        Ok(Command::Run(options)) => match app::play(&options) {
            Err(message) => {
                eprintln!("typr: {message}");
                1
            }
            Ok(stats) => {
                if let Some(stats) = stats {
                    println!("{}", summarise(&stats));
                }
                0
            }
        },
    }
}

/// One recognised option and its value, if it takes one.
struct Flags {
    time: Option<i64>,
    words: Option<i64>,
    quote: bool,
    list: String,
    punctuation: bool,
    numbers: bool,
    theme: String,
    width: usize,
    seed: Option<u64>,
    live_wpm: bool,
    free_backspace: bool,
}

impl Default for Flags {
    fn default() -> Self {
        Flags {
            time: None,
            words: None,
            quote: false,
            list: "english".to_string(),
            punctuation: false,
            numbers: false,
            theme: "default".to_string(),
            width: DEFAULT_WIDTH,
            seed: None,
            live_wpm: true,
            free_backspace: false,
        }
    }
}

fn parse(args: &[String]) -> Result<Command, String> {
    let mut flags = Flags::default();
    let mut arguments = args.iter().peekable();

    while let Some(argument) = arguments.next() {
        let mut value = || {
            arguments
                .next()
                .ok_or_else(|| format!("{argument} needs a value"))
        };

        match argument.as_str() {
            "-h" | "--help" => return Ok(Command::Print(usage())),
            "--version" => return Ok(Command::Print(format!("typr {VERSION}"))),
            "--themes" => return Ok(Command::Print(Theme::names().join("\n"))),
            "--lists" => return Ok(Command::Print(words::list_names().join("\n"))),
            "--stats" => return Ok(Command::Stats),
            "--doctor" => return Ok(Command::Diagnose),

            "-t" | "--time" => flags.time = Some(parse_number(argument, value()?)?),
            "-w" | "--words" => flags.words = Some(parse_number(argument, value()?)?),
            "-q" | "--quote" => flags.quote = true,
            "-l" | "--list" => flags.list = value()?.clone(),
            "-p" | "--punctuation" => flags.punctuation = true,
            "-n" | "--numbers" => flags.numbers = true,
            "--theme" => flags.theme = value()?.clone(),
            "--width" => flags.width = parse_number::<i64>(argument, value()?)?.max(0) as usize,
            "--seed" => flags.seed = Some(parse_number(argument, value()?)?),
            "--live-wpm" => flags.live_wpm = true,
            "--no-live-wpm" => flags.live_wpm = false,
            "--free-backspace" => flags.free_backspace = true,

            unknown if unknown.starts_with('-') => {
                return Err(format!("unknown option: {unknown}"))
            }
            extra => return Err(format!("unexpected argument: {extra}")),
        }
    }

    Ok(Command::Run(Box::new(build(flags)?)))
}

fn build(flags: Flags) -> Result<Options, String> {
    let (mode, limit) = resolve_mode(&flags)?;

    if !flags.quote && words::vocabulary(&flags.list).is_none() {
        return Err(format!(
            "unknown word list: {} (try: {})",
            flags.list,
            words::list_names().join(", ")
        ));
    }

    if !Theme::exists(&flags.theme) {
        return Err(format!(
            "unknown theme: {} (try: {})",
            flags.theme,
            Theme::names().join(", ")
        ));
    }

    Ok(Options {
        config: Config {
            mode,
            limit,
            list: flags.list,
            decoration: Decoration {
                punctuation: flags.punctuation,
                numbers: flags.numbers,
            },
        },
        theme: flags.theme,
        width: flags.width.max(MIN_WIDTH),
        live_wpm: flags.live_wpm,
        backtrack: if flags.free_backspace {
            Backtrack::Free
        } else {
            Backtrack::Strict
        },
        seed: flags.seed,
    })
}

fn resolve_mode(flags: &Flags) -> Result<(Mode, i64), String> {
    if flags.quote {
        return Ok((Mode::Quote, 0));
    }

    if let Some(count) = flags.words {
        return if count > 0 {
            Ok((Mode::Words, count))
        } else {
            Err("word count must be positive".to_string())
        };
    }

    let seconds = flags.time.unwrap_or(DEFAULT_SECONDS);

    if seconds > 0 {
        Ok((Mode::Time, seconds))
    } else {
        Err("time must be positive".to_string())
    }
}

fn parse_number<T: std::str::FromStr>(flag: &str, text: &str) -> Result<T, String> {
    text.parse()
        .map_err(|_| format!("{flag} needs a number, not {text:?}"))
}

fn summarise(stats: &Stats) -> String {
    let consistency = stats
        .consistency
        .map_or_else(|| "--".to_string(), |value| format!("{}%", value.round()));

    format!(
        "{} wpm · {}% acc · {} raw · {consistency} consistency",
        stats.wpm.round(),
        stats.accuracy.round(),
        stats.raw.round()
    )
}

/// Reports what the terminal layer can and cannot do here.
///
/// Terminal handling is the part of this program most likely to behave
/// differently on someone else's machine, so it can be interrogated directly
/// rather than guessed at from a failure message.
fn diagnosis() -> String {
    let (rows, columns) = terminal::size();

    let lines = [
        ("tty", terminal::is_tty().to_string()),
        (
            "term",
            std::env::var("TERM").unwrap_or_else(|_| "(unset)".to_string()),
        ),
        (
            "colorterm",
            std::env::var("COLORTERM").unwrap_or_else(|_| "(unset)".to_string()),
        ),
        ("size", format!("{rows}x{columns}")),
        ("history", History::open().path().display().to_string()),
    ];

    lines
        .iter()
        .map(|(key, value)| format!("{key:16}{value}\n"))
        .collect()
}

fn usage() -> String {
    format!(
        "\
typr — a typing test for the terminal

usage: typr [options]

modes
  -t, --time SECONDS      timed test (default: {DEFAULT_SECONDS})
  -w, --words COUNT       fixed word-count test
  -q, --quote             type a sentence

text
  -l, --list NAME         word list: {lists}
  -p, --punctuation       mix in punctuation and capitals
  -n, --numbers           mix in numbers

display
      --theme NAME        {themes}
      --width COLUMNS     text column width (default: {DEFAULT_WIDTH})
      --seed N            reproducible words — same seed, same test
      --no-live-wpm       hide the live speed counter

behaviour
      --free-backspace    allow going back to correctly typed words

other
      --stats             show bests, averages and trouble keys
      --doctor            report terminal capabilities
      --themes            list themes
      --lists             list word lists
      --version
  -h, --help

keys
  tab                     restart with new words
  r                       repeat the same words (results screen)
  backspace               delete a character
  ctrl+w                  delete the current word
  esc                     quit",
        lists = words::list_names().join(", "),
        themes = Theme::names().join(", "),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Result<Command, String> {
        parse(&args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>())
    }

    fn options(args: &[&str]) -> Options {
        match parse_args(args) {
            Ok(Command::Run(options)) => *options,
            _ => panic!("{args:?} did not produce a runnable test"),
        }
    }

    fn error(args: &[&str]) -> String {
        parse_args(args)
            .err()
            .unwrap_or_else(|| panic!("{args:?} was accepted"))
    }

    #[test]
    fn the_default_is_a_thirty_second_english_test() {
        let options = options(&[]);

        assert_eq!(options.config.mode, Mode::Time);
        assert_eq!(options.config.limit, 30);
        assert_eq!(options.config.list, "english");
        assert_eq!(options.config.decoration, Decoration::default());
        assert_eq!(options.width, DEFAULT_WIDTH);
        assert!(options.live_wpm);
        assert_eq!(options.backtrack, Backtrack::Strict);
    }

    #[test]
    fn short_and_long_flags_agree() {
        assert_eq!(
            options(&["-t", "15"]).config.limit,
            options(&["--time", "15"]).config.limit
        );
        assert_eq!(options(&["-w", "25"]).config.mode, Mode::Words);
        assert_eq!(options(&["--words", "25"]).config.limit, 25);
    }

    #[test]
    fn quote_mode_overrides_the_other_modes() {
        let options = options(&["-q", "-t", "60"]);

        assert_eq!(options.config.mode, Mode::Quote);
        assert_eq!(options.config.limit, 0);
    }

    #[test]
    fn decorations_are_collected() {
        let options = options(&["-p", "-n"]);

        assert!(options.config.decoration.punctuation);
        assert!(options.config.decoration.numbers);
    }

    #[test]
    fn live_wpm_can_be_turned_off_and_back_on() {
        assert!(!options(&["--no-live-wpm"]).live_wpm);
        assert!(options(&["--no-live-wpm", "--live-wpm"]).live_wpm);
    }

    #[test]
    fn free_backspace_relaxes_backtracking() {
        assert_eq!(options(&["--free-backspace"]).backtrack, Backtrack::Free);
    }

    #[test]
    fn the_width_has_a_floor_so_the_column_stays_usable() {
        assert_eq!(options(&["--width", "2"]).width, MIN_WIDTH);
        assert_eq!(options(&["--width", "100"]).width, 100);
    }

    #[test]
    fn a_seed_is_carried_through() {
        assert_eq!(options(&["--seed", "7"]).seed, Some(7));
        assert_eq!(options(&[]).seed, None);
    }

    #[test]
    fn unknown_options_are_refused_rather_than_ignored() {
        assert!(error(&["--nope"]).contains("unknown option"));
    }

    #[test]
    fn stray_arguments_are_refused() {
        assert!(error(&["oops"]).contains("unexpected argument"));
    }

    #[test]
    fn options_that_need_a_value_say_so() {
        assert!(error(&["--time"]).contains("needs a value"));
        assert!(error(&["--theme"]).contains("needs a value"));
    }

    #[test]
    fn numbers_have_to_be_numbers() {
        assert!(error(&["--time", "soon"]).contains("needs a number"));
    }

    #[test]
    fn zero_and_negative_lengths_are_refused() {
        assert!(error(&["-t", "0"]).contains("time must be positive"));
        assert!(error(&["-w", "0"]).contains("word count must be positive"));
        assert!(error(&["-t", "-5"]).contains("time must be positive"));
    }

    #[test]
    fn unknown_lists_and_themes_are_named_with_the_alternatives() {
        let list_error = error(&["--list", "klingon"]);
        assert!(list_error.contains("unknown word list"));
        assert!(list_error.contains("english_extended"));

        let theme_error = error(&["--theme", "neon"]);
        assert!(theme_error.contains("unknown theme"));
        assert!(theme_error.contains("matrix"));
    }

    #[test]
    fn quote_mode_does_not_need_a_valid_word_list() {
        // Nothing is drawn from the list, so an unused stale value in a shell
        // alias should not stop a quote test running.
        assert_eq!(
            options(&["-q", "--list", "klingon"]).config.mode,
            Mode::Quote
        );
    }

    #[test]
    fn help_and_version_report_without_running_a_test() {
        assert!(matches!(parse_args(&["--help"]), Ok(Command::Print(_))));
        assert!(matches!(parse_args(&["--version"]), Ok(Command::Print(_))));
        assert!(matches!(parse_args(&["--stats"]), Ok(Command::Stats)));
        assert!(matches!(parse_args(&["--doctor"]), Ok(Command::Diagnose)));
    }

    #[test]
    fn the_usage_text_lists_every_theme_and_word_list() {
        let usage = usage();

        for name in Theme::names() {
            assert!(usage.contains(name), "usage does not mention {name}");
        }

        for name in words::list_names() {
            assert!(usage.contains(name), "usage does not mention {name}");
        }
    }

    #[test]
    fn the_result_line_reads_as_a_sentence() {
        let stats = Stats {
            wpm: 72.4,
            accuracy: 96.6,
            raw: 78.1,
            consistency: Some(81.2),
            ..Stats::default()
        };

        assert_eq!(
            summarise(&stats),
            "72 wpm · 97% acc · 78 raw · 81% consistency"
        );
    }

    #[test]
    fn a_missing_consistency_shows_a_dash() {
        let stats = Stats {
            consistency: None,
            ..Stats::default()
        };

        assert!(summarise(&stats).contains("-- consistency"));
    }
}
