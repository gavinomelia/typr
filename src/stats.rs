//! Scoring, following monkeytype's definitions.
//!
//!   * **wpm** — correctly typed characters (plus the spaces after correctly
//!     typed words) divided by five, per minute. Mistyped words earn nothing,
//!     which is why accuracy and speed are not independent.
//!   * **raw** — the same figure ignoring correctness, i.e. how fast the fingers
//!     moved.
//!   * **accuracy** — correct keystrokes as a share of all keystrokes, judged at
//!     the moment each key was pressed. Fixing a typo does not restore accuracy.
//!   * **consistency** — how even the per-second raw speed was, derived from the
//!     coefficient of variation. 100% is a metronome.

use std::collections::BTreeMap;

use crate::engine::{Engine, Sample, SlipTally};

/// How often one letter was attempted, and how often it went wrong.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyTally {
    pub attempts: u32,
    pub errors: u32,
}

/// Per-letter tallies, keyed by the letter that should have been typed.
pub type KeyTallies = BTreeMap<char, KeyTally>;

/// A character-by-character comparison of one word against its target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub correct: u32,
    pub incorrect: u32,
    pub extra: u32,
    pub missed: u32,
}

impl Counts {
    fn add(&mut self, other: Counts) {
        self.correct += other.correct;
        self.incorrect += other.incorrect;
        self.extra += other.extra;
        self.missed += other.missed;
    }
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub wpm: f64,
    pub raw: f64,
    pub accuracy: f64,
    pub consistency: Option<f64>,
    pub correct: u32,
    pub incorrect: u32,
    pub extra: u32,
    pub missed: u32,
    pub elapsed_ms: i64,
    pub samples: Vec<Sample>,
    pub keys: KeyTallies,
    pub slips: SlipTally,
}

impl Stats {
    /// Scores a test.
    pub fn compute(engine: &Engine, now: i64) -> Self {
        let elapsed_ms = engine.elapsed_ms(now);
        let counts = character_counts(engine);
        let minutes = elapsed_ms as f64 / 60_000.0;

        Stats {
            wpm: per_minute(counts.correct + engine.correct_spaces, minutes),
            raw: per_minute(
                counts.correct + counts.incorrect + counts.extra + engine.spaces,
                minutes,
            ),
            accuracy: accuracy(engine),
            consistency: consistency(engine.samples()),
            correct: counts.correct,
            incorrect: counts.incorrect,
            extra: counts.extra,
            missed: counts.missed,
            elapsed_ms,
            samples: engine.samples().to_vec(),
            keys: key_tallies(engine),
            slips: engine.slips.clone(),
        }
    }
}

/// Per-letter attempts and mistakes, keyed by the letter that should have been
/// typed.
///
/// Kept as a plain map so it can be merged across tests without knowing
/// anything about how it was collected.
pub fn key_tallies(engine: &Engine) -> KeyTallies {
    engine
        .key_attempts
        .iter()
        .map(|(letter, attempts)| {
            let errors = engine.key_errors.get(letter).copied().unwrap_or(0);
            (
                *letter,
                KeyTally {
                    attempts: *attempts,
                    errors,
                },
            )
        })
        .collect()
}

/// Compares a typed word against its target.
///
/// Returns counts of characters that were correct, incorrect (typed in place of
/// a different letter), extra (typed past the end of the word) and missed
/// (letters the word had that were never typed).
pub fn compare(target: &str, typed: &str) -> Counts {
    let target_length = target.chars().count() as u32;
    let typed_length = typed.chars().count() as u32;
    let overlap = target_length.min(typed_length);

    let correct = target
        .chars()
        .zip(typed.chars())
        .filter(|(expected, actual)| expected == actual)
        .count() as u32;

    Counts {
        correct,
        incorrect: overlap - correct,
        extra: typed_length.saturating_sub(target_length),
        missed: target_length.saturating_sub(typed_length),
    }
}

/// A live WPM estimate for the header during a run.
pub fn live_wpm(engine: &Engine, now: i64) -> f64 {
    let elapsed = engine.elapsed_ms(now);

    if elapsed < 1000 {
        return 0.0;
    }

    let counts = character_counts(engine);
    per_minute(
        counts.correct + engine.correct_spaces,
        elapsed as f64 / 60_000.0,
    )
}

/// How even the per-second speed was, or `None` when there is too little to
/// judge.
pub fn consistency(samples: &[Sample]) -> Option<f64> {
    if samples.len() < 2 {
        return None;
    }

    let count = samples.len() as f64;
    let mean = samples.iter().map(|sample| sample.raw).sum::<f64>() / count;

    if mean <= 0.0 {
        return None;
    }

    let variance = samples
        .iter()
        .map(|sample| (sample.raw - mean) * (sample.raw - mean))
        .sum::<f64>()
        / count;

    Some(kogasa(variance.sqrt() / mean))
}

/// monkeytype's scaling of the coefficient of variation into a friendly 0-100
/// figure: an odd-power series fed through tanh, so small wobbles barely cost
/// anything while a stop-start run falls away quickly.
fn kogasa(cov: f64) -> f64 {
    100.0 * (1.0 - (cov + cov.powi(3) / 3.0 + cov.powi(5) / 5.0).tanh())
}

/// Committed words contribute missed characters; the word still being typed
/// does not, since the typist has not abandoned it yet.
fn character_counts(engine: &Engine) -> Counts {
    let mut totals = Counts::default();

    for (index, typed) in engine.typed().iter().enumerate() {
        let target = engine.words.get(index).map_or("", String::as_str);
        totals.add(compare(target, typed));
    }

    if !engine.buf.is_empty() {
        let mut in_progress = compare(engine.current_target(), &engine.buf);
        in_progress.missed = 0;
        totals.add(in_progress);
    }

    totals
}

fn per_minute(chars: u32, minutes: f64) -> f64 {
    if minutes <= 0.0 {
        return 0.0;
    }

    chars as f64 / 5.0 / minutes
}

fn accuracy(engine: &Engine) -> f64 {
    let total = engine.keys_correct + engine.keys_incorrect;

    if total == 0 {
        return 0.0;
    }

    engine.keys_correct as f64 / total as f64 * 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Backtrack, Key, Mode};

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|word| word.to_string()).collect()
    }

    /// Spreads the keystrokes evenly across `duration`, first key at zero and
    /// last key on the buzzer, so the elapsed time is exactly the duration
    /// asked for.
    fn type_over(engine: &mut Engine, text: &str, duration: i64) {
        let characters: Vec<char> = text.chars().collect();
        let last = (characters.len() as i64 - 1).max(1);

        for (index, character) in characters.into_iter().enumerate() {
            let now = (duration as f64 * index as f64 / last as f64).round() as i64;

            match character {
                ' ' => engine.key(Key::Space, now),
                character => engine.key(Key::Char(character), now),
            }
        }
    }

    fn samples(speeds: &[f64]) -> Vec<Sample> {
        speeds
            .iter()
            .enumerate()
            .map(|(index, raw)| Sample {
                at: (index + 1) as f64,
                raw: *raw,
                errors: 0,
            })
            .collect()
    }

    // compare

    #[test]
    fn compare_counts_a_perfect_word() {
        assert_eq!(
            compare("brown", "brown"),
            Counts {
                correct: 5,
                incorrect: 0,
                extra: 0,
                missed: 0
            }
        );
    }

    #[test]
    fn compare_counts_a_transposition_as_two_wrong_characters() {
        assert_eq!(
            compare("the", "teh"),
            Counts {
                correct: 1,
                incorrect: 2,
                extra: 0,
                missed: 0
            }
        );
    }

    #[test]
    fn compare_counts_characters_typed_past_the_end_of_the_word() {
        assert_eq!(
            compare("the", "thee"),
            Counts {
                correct: 3,
                incorrect: 0,
                extra: 1,
                missed: 0
            }
        );
    }

    #[test]
    fn compare_counts_characters_the_typist_never_got_to() {
        assert_eq!(
            compare("brown", "br"),
            Counts {
                correct: 2,
                incorrect: 0,
                extra: 0,
                missed: 3
            }
        );
    }

    // wpm

    #[test]
    fn a_perfect_minute_of_five_character_words_scores_its_word_count() {
        let list = vec!["acorn"; 20];
        let mut engine = Engine::new(Mode::Words, 20, words(&list), Backtrack::Strict);
        type_over(&mut engine, &list.join(" "), 60_000);

        let stats = Stats::compute(&engine, 60_000);

        // 20 words of five characters, plus the 19 spaces between them.
        assert_eq!(stats.correct, 100);
        assert!((stats.wpm - 23.8).abs() < 0.1, "wpm was {}", stats.wpm);
        assert_eq!(stats.accuracy, 100.0);
    }

    #[test]
    fn a_mistyped_word_earns_nothing_for_its_wrong_characters_but_raw_counts_them() {
        let mut engine = Engine::new(
            Mode::Words,
            2,
            words(&["acorn", "acorn"]),
            Backtrack::Strict,
        );
        type_over(&mut engine, "acorn acxrn", 60_000);
        engine.finish(60_000);

        let stats = Stats::compute(&engine, 60_000);

        assert_eq!(stats.correct, 9);
        assert_eq!(stats.incorrect, 1);
        assert!(stats.wpm < stats.raw);
        assert!(
            (stats.accuracy - 90.9).abs() < 0.1,
            "accuracy was {}",
            stats.accuracy
        );
    }

    #[test]
    fn an_unfinished_word_still_contributes_the_characters_that_were_typed() {
        let mut engine = Engine::new(
            Mode::Time,
            60,
            words(&["acorn", "acorn"]),
            Backtrack::Strict,
        );
        type_over(&mut engine, "acorn ac", 60_000);
        engine.finish(60_000);

        let stats = Stats::compute(&engine, 60_000);

        assert_eq!(stats.correct, 7);
        // The word in progress has not been abandoned, so nothing is "missed".
        assert_eq!(stats.missed, 0);
    }

    #[test]
    fn skipping_a_word_early_counts_the_rest_of_it_as_missed() {
        let mut engine = Engine::new(
            Mode::Words,
            2,
            words(&["acorn", "acorn"]),
            Backtrack::Strict,
        );
        type_over(&mut engine, "ac acorn", 60_000);

        assert_eq!(Stats::compute(&engine, 60_000).missed, 3);
    }

    #[test]
    fn speed_scales_with_the_clock() {
        let list = vec!["acorn"; 10];
        let typing = list.join(" ");

        let mut minute = Engine::new(Mode::Words, 10, words(&list), Backtrack::Strict);
        type_over(&mut minute, &typing, 60_000);

        let mut half_minute = Engine::new(Mode::Words, 10, words(&list), Backtrack::Strict);
        type_over(&mut half_minute, &typing, 30_000);

        let slow = Stats::compute(&minute, 60_000).wpm;
        let fast = Stats::compute(&half_minute, 30_000).wpm;

        assert!((fast - slow * 2.0).abs() < 0.001, "{fast} vs {slow}");
    }

    // consistency

    #[test]
    fn consistency_is_undefined_until_there_are_at_least_two_samples() {
        let mut engine = Engine::new(Mode::Time, 60, words(&["acorn"]), Backtrack::Strict);
        type_over(&mut engine, "acorn", 900);
        engine.finish(900);

        assert_eq!(Stats::compute(&engine, 900).consistency, None);
    }

    #[test]
    fn an_even_pace_scores_higher_than_a_stop_start_one() {
        let steady = consistency(&samples(&[40.0, 40.0, 40.0, 40.0])).unwrap();
        let erratic = consistency(&samples(&[10.0, 70.0, 5.0, 75.0])).unwrap();

        assert!(steady > 99.0, "steady was {steady}");
        assert!(erratic < steady, "{erratic} was not below {steady}");
    }

    #[test]
    fn consistency_is_undefined_when_nothing_was_typed() {
        assert_eq!(consistency(&samples(&[0.0, 0.0, 0.0])), None);
    }

    // live_wpm

    #[test]
    fn live_wpm_stays_at_zero_for_the_first_second_so_it_does_not_spike() {
        let mut engine = Engine::new(Mode::Time, 60, words(&["acorn"]), Backtrack::Strict);
        type_over(&mut engine, "ac", 0);

        assert_eq!(live_wpm(&engine, 200), 0.0);
        assert!(live_wpm(&engine, 2_000) > 0.0);
    }

    // key tallies

    #[test]
    fn key_tallies_pair_attempts_with_errors() {
        let mut engine = Engine::new(Mode::Time, 60, words(&["the"]), Backtrack::Strict);
        type_over(&mut engine, "tje", 1_000);

        let keys = Stats::compute(&engine, 1_000).keys;

        assert_eq!(
            keys[&'t'],
            KeyTally {
                attempts: 1,
                errors: 0
            }
        );
        assert_eq!(
            keys[&'h'],
            KeyTally {
                attempts: 1,
                errors: 1
            }
        );
    }
}
