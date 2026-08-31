//! Every completed test, appended to a file.
//!
//! Storing whole results rather than just personal bests means averages, trends
//! and per-letter weaknesses can all be recomputed later, including statistics
//! that had not been thought of when the result was recorded.
//!
//! The format is tab-separated so it can be read by eye, sorted, or fed to `awk`
//! without this program's help. Per-letter tallies are encoded as codepoints
//! (`101,240,12` = the letter `e`, attempted 240 times, missed 12) because the
//! letters themselves can be punctuation that would collide with the separators.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::datetime;
use crate::engine::{Mode, SlipTally};
use crate::stats::{KeyTallies, KeyTally, Stats};
use crate::words::Decoration;

const HEADER: &str = "# typr history v1\tat\tmode\tlimit\tlist\tflags\twpm\traw\taccuracy\tconsistency\tcorrect\tincorrect\textra\tmissed\tduration_ms\tkeys\tslips";

/// How many tab-separated columns a well-formed row has.
const COLUMNS: usize = 16;

/// The settings a test was run under, and the key it is filed against.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub mode: Mode,
    pub limit: i64,
    pub list: String,
    pub decoration: Decoration,
}

impl Config {
    /// The configuration key a result is filed under, such as
    /// `time-30-english-punctuation`.
    pub fn key(&self) -> String {
        let mut parts = vec![self.mode.as_str().to_string(), self.limit.to_string()];

        // Quote mode draws from a fixed set of sentences, so it has no word
        // list to name.
        if self.mode != Mode::Quote {
            parts.push(self.list.clone());
        }

        if self.decoration.punctuation {
            parts.push("punctuation".to_string());
        }

        if self.decoration.numbers {
            parts.push("numbers".to_string());
        }

        parts.join("-")
    }
}

/// One recorded test.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub at: String,
    pub mode: Mode,
    pub limit: i64,
    pub list: String,
    pub decoration: Decoration,
    pub config: String,
    pub wpm: f64,
    pub raw: f64,
    pub accuracy: f64,
    pub consistency: Option<f64>,
    pub correct: u32,
    pub incorrect: u32,
    pub extra: u32,
    pub missed: u32,
    pub duration_ms: i64,
    pub keys: KeyTallies,
    pub slips: SlipTally,
}

/// A history file. Nothing is read or written until asked for.
pub struct History {
    path: PathBuf,
}

impl History {
    /// A history stored at a particular path.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        History { path: path.into() }
    }

    /// The history in the usual place for this machine.
    pub fn open() -> Self {
        History::new(default_path())
    }

    /// Where results are stored, honouring `XDG_CONFIG_HOME`.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends a completed test.
    ///
    /// The caller is expected to ignore failures: an unwritable disk should not
    /// interrupt someone in the middle of practising.
    pub fn append(&self, stats: &Stats, config: &Config) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let fresh = !self.path.exists();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;

        if fresh {
            writeln!(file, "{HEADER}")?;
        }

        writeln!(file, "{}", encode(stats, config))
    }

    /// Every recorded result, oldest first. Unreadable lines are skipped.
    pub fn load(&self) -> Vec<Record> {
        let Ok(contents) = fs::read_to_string(&self.path) else {
            return Vec::new();
        };

        contents
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .filter_map(decode)
            .collect()
    }
}

/// Where results are stored, honouring `XDG_CONFIG_HOME`.
pub fn default_path() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".config")
        });

    base.join("typr").join("history.tsv")
}

fn encode(stats: &Stats, config: &Config) -> String {
    let fields = [
        datetime::timestamp(),
        config.mode.as_str().to_string(),
        config.limit.to_string(),
        config.list.clone(),
        encode_flags(config.decoration),
        round2(stats.wpm),
        round2(stats.raw),
        round2(stats.accuracy),
        stats.consistency.map_or_else(|| "-".to_string(), round2),
        stats.correct.to_string(),
        stats.incorrect.to_string(),
        stats.extra.to_string(),
        stats.missed.to_string(),
        stats.elapsed_ms.to_string(),
        encode_keys(&stats.keys),
        encode_slips(&stats.slips),
    ];

    fields.join("\t")
}

fn decode(line: &str) -> Option<Record> {
    let fields: Vec<&str> = line.split('\t').collect();

    if fields.len() != COLUMNS {
        return None;
    }

    let mode = decode_mode(fields[1]);
    let limit = fields[2].parse().unwrap_or(0);
    let list = fields[3].to_string();
    let decoration = decode_flags(fields[4]);

    let config = Config {
        mode,
        limit,
        list: list.clone(),
        decoration,
    };

    Some(Record {
        at: fields[0].to_string(),
        mode,
        limit,
        list,
        decoration,
        config: config.key(),
        wpm: to_float(fields[5]),
        raw: to_float(fields[6]),
        accuracy: to_float(fields[7]),
        consistency: decode_optional(fields[8]),
        correct: to_integer(fields[9]),
        incorrect: to_integer(fields[10]),
        extra: to_integer(fields[11]),
        missed: to_integer(fields[12]),
        duration_ms: fields[13].parse().unwrap_or(0),
        keys: decode_keys(fields[14]),
        slips: decode_slips(fields[15]),
    })
}

fn decode_mode(text: &str) -> Mode {
    match text {
        "words" => Mode::Words,
        "quote" => Mode::Quote,
        _ => Mode::Time,
    }
}

fn encode_flags(decoration: Decoration) -> String {
    let mut flags = Vec::new();

    if decoration.punctuation {
        flags.push("punctuation");
    }

    if decoration.numbers {
        flags.push("numbers");
    }

    if flags.is_empty() {
        "-".to_string()
    } else {
        flags.join(",")
    }
}

fn decode_flags(text: &str) -> Decoration {
    Decoration {
        punctuation: text.contains("punctuation"),
        numbers: text.contains("numbers"),
    }
}

fn encode_keys(keys: &KeyTallies) -> String {
    if keys.is_empty() {
        return "-".to_string();
    }

    keys.iter()
        .map(|(letter, tally)| format!("{},{},{}", *letter as u32, tally.attempts, tally.errors))
        .collect::<Vec<_>>()
        .join(";")
}

fn decode_keys(text: &str) -> KeyTallies {
    if text == "-" {
        return KeyTallies::new();
    }

    text.split(';')
        .filter(|entry| !entry.is_empty())
        .filter_map(|entry| {
            let mut parts = entry.split(',');
            let letter = grapheme(parts.next()?)?;
            let attempts = to_integer(parts.next()?);
            let errors = to_integer(parts.next()?);

            // A fourth field means this is not a tally we understand.
            if parts.next().is_some() {
                return None;
            }

            Some((letter, KeyTally { attempts, errors }))
        })
        .collect()
}

fn encode_slips(slips: &SlipTally) -> String {
    if slips.is_empty() {
        return "-".to_string();
    }

    slips
        .iter()
        .map(|((expected, actual), count)| {
            format!("{},{},{}", *expected as u32, *actual as u32, count)
        })
        .collect::<Vec<_>>()
        .join(";")
}

fn decode_slips(text: &str) -> SlipTally {
    if text == "-" {
        return SlipTally::new();
    }

    text.split(';')
        .filter(|entry| !entry.is_empty())
        .filter_map(|entry| {
            let mut parts = entry.split(',');
            let expected = grapheme(parts.next()?)?;
            let actual = grapheme(parts.next()?)?;
            let count = to_integer(parts.next()?);

            if parts.next().is_some() {
                return None;
            }

            Some(((expected, actual), count))
        })
        .collect()
}

/// Turns an encoded codepoint back into the character it stands for.
fn grapheme(code: &str) -> Option<char> {
    char::from_u32(code.parse().ok()?)
}

/// Two decimal places, with trailing zeros trimmed but never the whole
/// fraction, so the file reads as numbers rather than as accounting.
fn round2(value: f64) -> String {
    let mut text = format!("{value:.2}");

    while text.ends_with('0') && !text.ends_with(".0") {
        text.pop();
    }

    text
}

fn decode_optional(text: &str) -> Option<f64> {
    if text == "-" {
        None
    } else {
        Some(to_float(text))
    }
}

fn to_integer(text: &str) -> u32 {
    text.parse().unwrap_or(0)
}

fn to_float(text: &str) -> f64 {
    text.parse().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A directory that deletes itself, so the tests leave no litter and never
    /// have to touch `XDG_CONFIG_HOME`.
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            static COUNTER: AtomicU32 = AtomicU32::new(0);

            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("typr-test-{}-{unique}", std::process::id()));

            fs::create_dir_all(&path).expect("could not make a temporary directory");
            TempDir { path }
        }

        fn history(&self) -> History {
            History::new(self.path.join("history.tsv"))
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn stats() -> Stats {
        Stats {
            wpm: 72.5,
            raw: 78.1,
            accuracy: 96.4,
            consistency: Some(81.2),
            correct: 210,
            incorrect: 6,
            extra: 1,
            missed: 2,
            elapsed_ms: 30_000,
            keys: [
                (
                    'e',
                    KeyTally {
                        attempts: 40,
                        errors: 3,
                    },
                ),
                (
                    't',
                    KeyTally {
                        attempts: 30,
                        errors: 0,
                    },
                ),
            ]
            .into_iter()
            .collect(),
            slips: [(('e', 'r'), 3)].into_iter().collect(),
            ..Stats::default()
        }
    }

    fn config() -> Config {
        Config {
            mode: Mode::Time,
            limit: 30,
            list: "english".to_string(),
            decoration: Decoration::default(),
        }
    }

    #[test]
    fn load_is_empty_when_nothing_has_been_recorded() {
        let directory = TempDir::new();

        assert!(directory.history().load().is_empty());
    }

    #[test]
    fn a_result_survives_the_round_trip() {
        let directory = TempDir::new();
        let history = directory.history();
        history.append(&stats(), &config()).unwrap();

        let loaded = history.load();
        assert_eq!(loaded.len(), 1);

        let record = &loaded[0];
        assert_eq!(record.wpm, 72.5);
        assert_eq!(record.accuracy, 96.4);
        assert_eq!(record.consistency, Some(81.2));
        assert_eq!(record.correct, 210);
        assert_eq!(record.duration_ms, 30_000);
        assert_eq!(record.mode, Mode::Time);
        assert_eq!(record.limit, 30);
        assert_eq!(record.list, "english");
        assert_eq!(record.config, "time-30-english");
        assert_eq!(record.keys, stats().keys);
        assert_eq!(record.slips, stats().slips);
    }

    #[test]
    fn results_accumulate_in_the_order_they_were_run() {
        let directory = TempDir::new();
        let history = directory.history();

        for wpm in [60.0, 70.0, 65.0] {
            history
                .append(&Stats { wpm, ..stats() }, &config())
                .unwrap();
        }

        let speeds: Vec<f64> = history.load().iter().map(|record| record.wpm).collect();
        assert_eq!(speeds, [60.0, 70.0, 65.0]);
    }

    #[test]
    fn flags_become_part_of_the_configuration_key() {
        let directory = TempDir::new();
        let history = directory.history();
        let config = Config {
            decoration: Decoration {
                punctuation: true,
                numbers: true,
            },
            ..config()
        };

        history.append(&stats(), &config).unwrap();

        let loaded = history.load();
        assert!(loaded[0].decoration.punctuation);
        assert!(loaded[0].decoration.numbers);
        assert_eq!(loaded[0].config, "time-30-english-punctuation-numbers");
    }

    #[test]
    fn a_missing_consistency_stays_missing_rather_than_becoming_zero() {
        let directory = TempDir::new();
        let history = directory.history();
        history
            .append(
                &Stats {
                    consistency: None,
                    ..stats()
                },
                &config(),
            )
            .unwrap();

        assert_eq!(history.load()[0].consistency, None);
    }

    #[test]
    fn punctuation_characters_in_key_tallies_do_not_corrupt_the_row() {
        // Semicolons and commas are the separators used inside the keys column,
        // so they are exactly the characters that could break the encoding.
        let keys: KeyTallies = [
            (
                ';',
                KeyTally {
                    attempts: 12,
                    errors: 4,
                },
            ),
            (
                ',',
                KeyTally {
                    attempts: 9,
                    errors: 1,
                },
            ),
        ]
        .into_iter()
        .collect();

        let slips: SlipTally = [((';', ','), 4), ((' ', 'n'), 2)].into_iter().collect();

        let directory = TempDir::new();
        let history = directory.history();
        history
            .append(
                &Stats {
                    keys: keys.clone(),
                    slips: slips.clone(),
                    ..stats()
                },
                &config(),
            )
            .unwrap();

        let loaded = history.load();
        assert_eq!(loaded[0].keys, keys);
        assert_eq!(loaded[0].slips, slips);
    }

    #[test]
    fn a_tab_in_the_word_list_name_cannot_forge_a_column() {
        let directory = TempDir::new();
        let history = directory.history();

        // Not reachable through the CLI, but the encoder should not be the
        // reason a hostile value becomes a parsing problem.
        history
            .append(
                &stats(),
                &Config {
                    list: "eng\tlish".to_string(),
                    ..config()
                },
            )
            .unwrap();

        // The row now has too many columns and is skipped rather than
        // silently loading as some other test's result.
        assert!(history.load().is_empty());
    }

    #[test]
    fn the_file_is_human_readable_with_a_header_and_one_row_per_test() {
        let directory = TempDir::new();
        let history = directory.history();
        history.append(&stats(), &config()).unwrap();
        history.append(&stats(), &config()).unwrap();

        let contents = fs::read_to_string(history.path()).unwrap();
        let lines: Vec<&str> = contents.lines().filter(|line| !line.is_empty()).collect();

        assert!(lines[0].starts_with("# typr history v1"));
        assert_eq!(lines.len(), 3);
        assert!(lines[1..]
            .iter()
            .all(|row| row.split('\t').count() == COLUMNS));
    }

    #[test]
    fn corrupt_lines_are_skipped_instead_of_taking_the_file_down_with_them() {
        let directory = TempDir::new();
        let history = directory.history();

        history
            .append(
                &Stats {
                    wpm: 60.0,
                    ..stats()
                },
                &config(),
            )
            .unwrap();

        let mut file = OpenOptions::new()
            .append(true)
            .open(history.path())
            .unwrap();
        writeln!(file, "this is not a result").unwrap();
        drop(file);

        history
            .append(
                &Stats {
                    wpm: 70.0,
                    ..stats()
                },
                &config(),
            )
            .unwrap();

        let speeds: Vec<f64> = history.load().iter().map(|record| record.wpm).collect();
        assert_eq!(speeds, [60.0, 70.0]);
    }

    #[test]
    fn config_keys_name_each_combination_distinctly() {
        assert_eq!(config().key(), "time-30-english");

        assert_eq!(
            Config {
                mode: Mode::Words,
                limit: 25,
                ..config()
            }
            .key(),
            "words-25-english"
        );

        assert_eq!(
            Config {
                decoration: Decoration {
                    punctuation: true,
                    numbers: false
                },
                ..config()
            }
            .key(),
            "time-30-english-punctuation"
        );

        assert_eq!(
            Config {
                list: "english_extended".to_string(),
                ..config()
            }
            .key(),
            "time-30-english_extended"
        );
    }

    #[test]
    fn quote_mode_has_no_word_list_to_name() {
        assert_eq!(
            Config {
                mode: Mode::Quote,
                limit: 0,
                ..config()
            }
            .key(),
            "quote-0"
        );
    }

    #[test]
    fn numbers_keep_a_decimal_point_but_lose_trailing_zeros() {
        assert_eq!(round2(72.5), "72.5");
        assert_eq!(round2(30.0), "30.0");
        assert_eq!(round2(96.437), "96.44");
        assert_eq!(round2(0.0), "0.0");
    }
}
