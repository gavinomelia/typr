//! Aggregates a history of results into the figures worth looking at.
//!
//! Pure functions over a list of records, so the interesting arithmetic —
//! averages, trends, which letters let you down — can be tested without any
//! files or terminals involved.

use std::cmp::Reverse;
use std::collections::BTreeMap;

use crate::datetime::{self, Date};
use crate::engine::SlipTally;
use crate::history::Record;
use crate::stats::KeyTallies;

const RECENT_WINDOW: usize = 10;
const TREND_WINDOW: usize = 20;

/// How many times a letter must have been typed before it can be called
/// trouble, and how many of them to report.
#[derive(Clone, Copy, Debug)]
pub struct TroubleOptions {
    pub min_attempts: u32,
    pub keys: usize,
    pub slips: usize,
}

impl Default for TroubleOptions {
    fn default() -> Self {
        TroubleOptions {
            min_attempts: 20,
            keys: 8,
            slips: 6,
        }
    }
}

/// A letter that goes wrong often enough to be worth practising.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TroubleKey {
    pub key: char,
    pub attempts: u32,
    pub errors: u32,
    pub accuracy: f64,
}

/// One letter typed in place of another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slip {
    pub expected: char,
    pub actual: char,
    pub count: u32,
}

/// How one configuration has gone over time.
#[derive(Clone, Debug, PartialEq)]
pub struct ConfigSummary {
    pub config: String,
    pub tests: usize,
    pub best: f64,
    pub average: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Summary {
    pub tests: usize,
    pub typing_ms: i64,
    pub words_typed: u64,
    pub best: Option<Record>,
    pub average_wpm: Option<f64>,
    pub average_accuracy: Option<f64>,
    pub average_consistency: Option<f64>,
    pub recent_average: Option<f64>,
    pub improvement: Option<f64>,
    pub trend: Vec<f64>,
    pub by_config: Vec<ConfigSummary>,
    pub days_practiced: usize,
    pub streak: u32,
    pub last_at: Option<String>,
    pub trouble_keys: Vec<TroubleKey>,
    pub slips: Vec<Slip>,
}

impl Summary {
    /// Builds a summary of every result given.
    pub fn build(results: &[Record], options: TroubleOptions) -> Self {
        if results.is_empty() {
            return Summary::default();
        }

        let speeds: Vec<f64> = results.iter().map(|record| record.wpm).collect();
        let days = practice_days(results);

        Summary {
            tests: results.len(),
            typing_ms: results.iter().map(|record| record.duration_ms).sum(),
            // Five characters to a word, by the same convention WPM uses.
            words_typed: (results
                .iter()
                .map(|record| u64::from(record.correct))
                .sum::<u64>() as f64
                / 5.0)
                .round() as u64,
            best: best_of(results).cloned(),
            average_wpm: average(&speeds),
            average_accuracy: average(
                &results
                    .iter()
                    .map(|record| record.accuracy)
                    .collect::<Vec<_>>(),
            ),
            average_consistency: average(
                &results
                    .iter()
                    .filter_map(|record| record.consistency)
                    .collect::<Vec<_>>(),
            ),
            recent_average: average(last(&speeds, RECENT_WINDOW)),
            improvement: improvement(&speeds),
            trend: last(&speeds, TREND_WINDOW).to_vec(),
            by_config: by_config(results),
            days_practiced: days.len(),
            streak: streak(&days),
            last_at: results.last().map(|record| record.at.clone()),
            trouble_keys: trouble_keys(&merge_keys(results), options),
            slips: top_slips(&merge_slips(results), options.slips),
        }
    }
}

/// The best result overall, or `None` for an empty history.
pub fn best_of(results: &[Record]) -> Option<&Record> {
    results
        .iter()
        .reduce(|best, record| if record.wpm > best.wpm { record } else { best })
}

/// The best result for one configuration, or `None` if it has never been run.
pub fn best_for<'a>(results: &'a [Record], config: &str) -> Option<&'a Record> {
    results
        .iter()
        .filter(|record| record.config == config)
        .reduce(|best, record| if record.wpm > best.wpm { record } else { best })
}

/// Average speed for one configuration, or `None`.
pub fn average_for(results: &[Record], config: &str) -> Option<f64> {
    let speeds: Vec<f64> = results
        .iter()
        .filter(|record| record.config == config)
        .map(|record| record.wpm)
        .collect();

    average(&speeds)
}

/// How many tests have been run with one configuration.
pub fn count_for(results: &[Record], config: &str) -> usize {
    results
        .iter()
        .filter(|record| record.config == config)
        .count()
}

/// Adds up per-letter tallies across every result.
pub fn merge_keys(results: &[Record]) -> KeyTallies {
    let mut merged = KeyTallies::new();

    for record in results {
        for (letter, tally) in &record.keys {
            let entry = merged.entry(*letter).or_default();
            entry.attempts += tally.attempts;
            entry.errors += tally.errors;
        }
    }

    merged
}

/// Adds up letter confusions across every result.
pub fn merge_slips(results: &[Record]) -> SlipTally {
    let mut merged = SlipTally::new();

    for record in results {
        for (pair, count) in &record.slips {
            *merged.entry(*pair).or_insert(0) += count;
        }
    }

    merged
}

/// The letters that go wrong most often, worst accuracy first.
///
/// Letters typed only a handful of times are excluded: one slip on a letter you
/// have typed twice says nothing, and would otherwise dominate the list.
pub fn trouble_keys(keys: &KeyTallies, options: TroubleOptions) -> Vec<TroubleKey> {
    let mut trouble: Vec<TroubleKey> = keys
        .iter()
        .filter(|(_letter, tally)| tally.attempts >= options.min_attempts && tally.errors > 0)
        .map(|(letter, tally)| TroubleKey {
            key: *letter,
            attempts: tally.attempts,
            errors: tally.errors,
            accuracy: f64::from(tally.attempts - tally.errors) / f64::from(tally.attempts) * 100.0,
        })
        .collect();

    // Worst accuracy first; where two letters are equally bad, the one you get
    // wrong more often matters more.
    trouble.sort_by(|a, b| {
        a.accuracy
            .partial_cmp(&b.accuracy)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.errors.cmp(&a.errors))
    });

    trouble.truncate(options.keys);
    trouble
}

/// The most frequent letter confusions.
pub fn top_slips(slips: &SlipTally, limit: usize) -> Vec<Slip> {
    let mut ranked: Vec<Slip> = slips
        .iter()
        .map(|((expected, actual), count)| Slip {
            expected: *expected,
            actual: *actual,
            count: *count,
        })
        .collect();

    ranked.sort_by_key(|slip| Reverse(slip.count));
    ranked.truncate(limit);
    ranked
}

/// Formats a duration the way a person would say it.
pub fn humanize_ms(ms: i64) -> String {
    if ms < 60_000 {
        format!("{}s", ms / 1000)
    } else if ms < 3_600_000 {
        format!("{}m", ms / 60_000)
    } else {
        format!("{}h {}m", ms / 3_600_000, (ms % 3_600_000) / 60_000)
    }
}

fn by_config(results: &[Record]) -> Vec<ConfigSummary> {
    let mut grouped: BTreeMap<&str, Vec<f64>> = BTreeMap::new();

    for record in results {
        grouped
            .entry(record.config.as_str())
            .or_default()
            .push(record.wpm);
    }

    let mut summaries: Vec<ConfigSummary> = grouped
        .into_iter()
        .map(|(config, speeds)| ConfigSummary {
            config: config.to_string(),
            tests: speeds.len(),
            best: speeds.iter().copied().fold(f64::MIN, f64::max),
            average: average(&speeds).unwrap_or(0.0),
        })
        .collect();

    summaries.sort_by_key(|summary| Reverse(summary.tests));
    summaries
}

/// Compares the most recent tests against the ones before them, which is a
/// fairer read on progress than first-ever versus latest.
fn improvement(speeds: &[f64]) -> Option<f64> {
    if speeds.len() < 2 * RECENT_WINDOW {
        return None;
    }

    let recent = last(speeds, RECENT_WINDOW);
    let earlier = last(&speeds[..speeds.len() - RECENT_WINDOW], RECENT_WINDOW);

    Some(average(recent)? - average(earlier)?)
}

/// The distinct days that were practised on, most recent first.
fn practice_days(results: &[Record]) -> Vec<Date> {
    let mut days: Vec<Date> = results
        .iter()
        .filter_map(|record| datetime::parse_date(&record.at))
        .collect();

    days.sort_unstable();
    days.dedup();
    days.reverse();
    days
}

/// A streak is only alive if it reaches today or yesterday; otherwise it was
/// broken and the count would be a flattering lie.
fn streak(days: &[Date]) -> u32 {
    let Some(most_recent) = days.first() else {
        return 0;
    };

    if datetime::today().diff(*most_recent) > 1 {
        return 0;
    }

    let mut count = 1;

    for pair in days.windows(2) {
        if pair[0].diff(pair[1]) == 1 {
            count += 1;
        } else {
            break;
        }
    }

    count
}

fn last<T>(values: &[T], count: usize) -> &[T] {
    &values[values.len().saturating_sub(count)..]
}

fn average(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }

    Some(values.iter().sum::<f64>() / values.len() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datetime::days_ago;
    use crate::engine::Mode;
    use crate::stats::KeyTally;
    use crate::words::Decoration;

    fn record(wpm: f64) -> Record {
        Record {
            at: days_ago(0),
            mode: Mode::Time,
            limit: 30,
            list: "english".to_string(),
            decoration: Decoration::default(),
            config: "time-30-english".to_string(),
            wpm,
            raw: 75.0,
            accuracy: 96.0,
            consistency: Some(80.0),
            correct: 175,
            incorrect: 5,
            extra: 0,
            missed: 0,
            duration_ms: 30_000,
            keys: KeyTallies::new(),
            slips: SlipTally::new(),
        }
    }

    fn records(speeds: &[f64]) -> Vec<Record> {
        speeds.iter().map(|wpm| record(*wpm)).collect()
    }

    fn on(day: i64) -> Record {
        Record {
            at: days_ago(day),
            ..record(70.0)
        }
    }

    fn keys(entries: &[(char, u32, u32)]) -> KeyTallies {
        entries
            .iter()
            .map(|(letter, attempts, errors)| {
                (
                    *letter,
                    KeyTally {
                        attempts: *attempts,
                        errors: *errors,
                    },
                )
            })
            .collect()
    }

    fn options() -> TroubleOptions {
        TroubleOptions::default()
    }

    // build

    #[test]
    fn an_empty_history_summarises_to_nothing_rather_than_crashing() {
        let summary = Summary::build(&[], options());

        assert_eq!(summary.tests, 0);
        assert!(summary.best.is_none());
        assert!(summary.average_wpm.is_none());
        assert!(summary.trouble_keys.is_empty());
    }

    #[test]
    fn counts_tests_time_and_words() {
        let summary = Summary::build(&records(&[60.0, 70.0, 80.0]), options());

        assert_eq!(summary.tests, 3);
        assert_eq!(summary.typing_ms, 90_000);
        // 175 correct characters per test, five characters to a word.
        assert_eq!(summary.words_typed, 105);
    }

    #[test]
    fn finds_the_best_result_and_the_averages() {
        let summary = Summary::build(&records(&[60.0, 90.0, 75.0]), options());

        assert_eq!(summary.best.unwrap().wpm, 90.0);
        assert_eq!(summary.average_wpm, Some(75.0));
        assert_eq!(summary.average_accuracy, Some(96.0));
        assert_eq!(summary.average_consistency, Some(80.0));
    }

    #[test]
    fn averages_the_last_ten_tests_separately_from_all_time() {
        let mut speeds = vec![40.0; 10];
        speeds.extend(vec![80.0; 10]);

        let summary = Summary::build(&records(&speeds), options());

        assert_eq!(summary.average_wpm, Some(60.0));
        assert_eq!(summary.recent_average, Some(80.0));
    }

    #[test]
    fn improvement_compares_the_last_ten_against_the_ten_before() {
        let mut speeds = vec![50.0; 10];
        speeds.extend(vec![65.0; 10]);

        assert_eq!(
            Summary::build(&records(&speeds), options()).improvement,
            Some(15.0)
        );
    }

    #[test]
    fn improvement_is_withheld_until_there_is_enough_history_to_mean_anything() {
        let speeds = vec![50.0; 19];

        assert_eq!(
            Summary::build(&records(&speeds), options()).improvement,
            None
        );
    }

    #[test]
    fn the_trend_keeps_the_most_recent_tests_in_order() {
        let speeds: Vec<f64> = (1..=30).map(f64::from).collect();
        let summary = Summary::build(&records(&speeds), options());

        assert_eq!(summary.trend.len(), 20);
        assert_eq!(summary.trend.first(), Some(&11.0));
        assert_eq!(summary.trend.last(), Some(&30.0));
    }

    #[test]
    fn groups_results_by_configuration_busiest_first() {
        let mut history = records(&[60.0, 80.0]);

        for wpm in [50.0, 90.0, 70.0] {
            history.push(Record {
                config: "words-25-english".to_string(),
                ..record(wpm)
            });
        }

        let by_config = Summary::build(&history, options()).by_config;

        assert_eq!(by_config[0].config, "words-25-english");
        assert_eq!(by_config[0].tests, 3);
        assert_eq!(by_config[0].best, 90.0);
        assert_eq!(by_config[0].average, 70.0);
        assert_eq!(by_config[1].tests, 2);
    }

    #[test]
    fn consistency_is_averaged_only_over_the_tests_that_measured_it() {
        let history = vec![
            Record {
                consistency: None,
                ..record(70.0)
            },
            Record {
                consistency: Some(60.0),
                ..record(70.0)
            },
            Record {
                consistency: Some(80.0),
                ..record(70.0)
            },
        ];

        assert_eq!(
            Summary::build(&history, options()).average_consistency,
            Some(70.0)
        );
    }

    // streaks

    #[test]
    fn counts_consecutive_days_up_to_today() {
        let summary = Summary::build(&[on(2), on(1), on(0)], options());

        assert_eq!(summary.days_practiced, 3);
        assert_eq!(summary.streak, 3);
    }

    #[test]
    fn several_tests_in_one_day_count_as_one_day() {
        let summary = Summary::build(&[on(0), on(0), on(1)], options());

        assert_eq!(summary.days_practiced, 2);
        assert_eq!(summary.streak, 2);
    }

    #[test]
    fn a_streak_still_counts_if_todays_session_has_not_happened_yet() {
        assert_eq!(Summary::build(&[on(2), on(1)], options()).streak, 2);
    }

    #[test]
    fn a_gap_of_more_than_a_day_breaks_the_streak() {
        assert_eq!(Summary::build(&[on(9), on(8)], options()).streak, 0);
    }

    #[test]
    fn only_the_run_up_to_now_counts_not_the_longest_run_ever() {
        let history = [on(20), on(19), on(18), on(1), on(0)];

        assert_eq!(Summary::build(&history, options()).streak, 2);
    }

    // per configuration lookups

    #[test]
    fn best_average_and_count_are_scoped_to_one_configuration() {
        let mut history = records(&[60.0, 80.0]);
        history.push(Record {
            config: "words-25-english".to_string(),
            ..record(100.0)
        });

        assert_eq!(best_for(&history, "time-30-english").unwrap().wpm, 80.0);
        assert_eq!(average_for(&history, "time-30-english"), Some(70.0));
        assert_eq!(count_for(&history, "time-30-english"), 2);
        assert_eq!(best_for(&history, "words-25-english").unwrap().wpm, 100.0);
    }

    #[test]
    fn a_configuration_never_run_has_no_best() {
        let history = records(&[60.0]);

        assert!(best_for(&history, "quote-0").is_none());
        assert_eq!(average_for(&history, "quote-0"), None);
        assert_eq!(count_for(&history, "quote-0"), 0);
    }

    // trouble keys

    #[test]
    fn adds_up_letter_tallies_across_tests() {
        let history = vec![
            Record {
                keys: keys(&[('e', 100, 5)]),
                ..record(70.0)
            },
            Record {
                keys: keys(&[('e', 100, 15), ('t', 50, 0)]),
                ..record(70.0)
            },
        ];

        assert_eq!(merge_keys(&history), keys(&[('e', 200, 20), ('t', 50, 0)]));
    }

    #[test]
    fn ranks_the_least_accurate_letter_first() {
        let tallies = keys(&[('e', 100, 20), ('t', 100, 5), ('a', 100, 40)]);
        let trouble = trouble_keys(&tallies, options());

        assert_eq!(trouble[0].key, 'a');
        assert_eq!(trouble[0].accuracy, 60.0);
        assert_eq!(trouble[0].errors, 40);
        assert_eq!(trouble[1].key, 'e');
        assert_eq!(trouble[2].key, 't');
    }

    #[test]
    fn ignores_letters_that_have_barely_been_typed() {
        let tallies = keys(&[('z', 2, 2), ('e', 100, 10)]);
        let trouble = trouble_keys(&tallies, options());

        assert_eq!(trouble.len(), 1);
        assert_eq!(trouble[0].key, 'e');
    }

    #[test]
    fn the_threshold_can_be_lowered_for_a_single_test() {
        let tallies = keys(&[('z', 4, 2)]);
        let trouble = trouble_keys(
            &tallies,
            TroubleOptions {
                min_attempts: 3,
                ..options()
            },
        );

        assert_eq!(trouble[0].key, 'z');
    }

    #[test]
    fn letters_typed_perfectly_are_not_trouble() {
        assert!(trouble_keys(&keys(&[('e', 100, 0)]), options()).is_empty());
    }

    #[test]
    fn the_trouble_list_is_capped() {
        let tallies: KeyTallies = ('a'..='z')
            .map(|letter| {
                (
                    letter,
                    KeyTally {
                        attempts: 100,
                        errors: 10,
                    },
                )
            })
            .collect();

        assert_eq!(trouble_keys(&tallies, options()).len(), 8);
        assert_eq!(
            trouble_keys(
                &tallies,
                TroubleOptions {
                    keys: 3,
                    ..options()
                }
            )
            .len(),
            3
        );
    }

    // slips

    #[test]
    fn adds_up_confusions_across_tests_and_ranks_them() {
        let history = vec![
            Record {
                slips: [(('e', 'r'), 10), (('n', 'm'), 2)].into_iter().collect(),
                ..record(70.0)
            },
            Record {
                slips: [(('e', 'r'), 5), (('a', 's'), 20)].into_iter().collect(),
                ..record(70.0)
            },
        ];

        let ranked = top_slips(&merge_slips(&history), 5);

        assert_eq!(
            ranked[0],
            Slip {
                expected: 'a',
                actual: 's',
                count: 20
            }
        );
        assert_eq!(
            ranked[1],
            Slip {
                expected: 'e',
                actual: 'r',
                count: 15
            }
        );
        assert_eq!(
            ranked[2],
            Slip {
                expected: 'n',
                actual: 'm',
                count: 2
            }
        );
    }

    // humanize_ms

    #[test]
    fn durations_read_the_way_a_person_would_say_them() {
        assert_eq!(humanize_ms(45_000), "45s");
        assert_eq!(humanize_ms(600_000), "10m");
        assert_eq!(humanize_ms(4_320_000), "1h 12m");
    }
}
