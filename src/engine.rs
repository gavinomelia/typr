//! The typing test as a state machine.
//!
//! Time is always passed in rather than read, and nothing here touches a
//! terminal, so a whole test can be replayed keystroke by keystroke in a unit
//! test. `App` owns the clock; this module owns the rules.
//!
//! ## Rules
//!
//!   * A word is committed by pressing space, which advances to the next word
//!     whether or not the word was typed correctly.
//!   * Characters typed past the end of a word are kept as "extra" characters,
//!     the way monkeytype does, rather than being swallowed.
//!   * Backspace at the start of a word steps back to the previous word only if
//!     that word contains a mistake, unless [`Backtrack::Free`] is in force.
//!   * In word and quote modes, typing the final word correctly ends the test
//!     without needing a trailing space.

use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Time,
    Words,
    Quote,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Time => "time",
            Mode::Words => "words",
            Mode::Quote => "quote",
        }
    }
}

/// A keystroke, already interpreted. `App` turns bytes into these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Space,
    Backspace,
    BackspaceWord,
}

/// How far the typist has got with a particular word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Done,
    Current,
    Pending,
}

/// Whether the typist may return to a word they already typed correctly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backtrack {
    /// Only words containing a mistake can be revisited.
    Strict,
    /// Any previous word can be revisited.
    Free,
}

/// What to leave in the buffer after stepping back into a previous word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Restore {
    /// Put the previous attempt back, ready to be corrected.
    Text,
    /// Start the word again from nothing.
    Cleared,
}

/// A per-second slice of the test, used for the results graph.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub at: f64,
    pub raw: f64,
    pub errors: u32,
}

/// One word paired with what was typed for it, for rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Word<'a> {
    pub index: usize,
    pub target: &'a str,
    pub typed: &'a str,
    pub status: Status,
}

/// Per-letter tallies, keyed by the letter that *should* have been typed.
pub type LetterTally = BTreeMap<char, u32>;

/// Letter confusions, keyed by `(expected, actual)`.
pub type SlipTally = BTreeMap<(char, char), u32>;

pub struct Engine {
    pub mode: Mode,
    pub limit: i64,
    pub words: Vec<String>,
    pub buf: String,
    pub index: usize,
    pub keys_correct: u32,
    pub keys_incorrect: u32,
    pub spaces: u32,
    pub correct_spaces: u32,
    pub key_attempts: LetterTally,
    pub key_errors: LetterTally,
    pub slips: SlipTally,
    typed: Vec<String>,
    started_at: Option<i64>,
    finished_at: Option<i64>,
    samples: Vec<Sample>,
    sample_chars: u32,
    sample_errors: u32,
    sampled_ms: i64,
    backtrack: Backtrack,
}

impl Engine {
    pub fn new(mode: Mode, limit: i64, words: Vec<String>, backtrack: Backtrack) -> Self {
        Engine {
            mode,
            limit,
            words,
            buf: String::new(),
            index: 0,
            keys_correct: 0,
            keys_incorrect: 0,
            spaces: 0,
            correct_spaces: 0,
            key_attempts: BTreeMap::new(),
            key_errors: BTreeMap::new(),
            slips: BTreeMap::new(),
            typed: Vec::new(),
            started_at: None,
            finished_at: None,
            samples: Vec::new(),
            sample_chars: 0,
            sample_errors: 0,
            sampled_ms: 0,
            backtrack,
        }
    }

    /// The words typed so far, in order, excluding the word in progress.
    pub fn typed(&self) -> &[String] {
        &self.typed
    }

    /// The word currently being typed.
    pub fn current_target(&self) -> &str {
        self.words.get(self.index).map_or("", String::as_str)
    }

    /// Whether the test has started; the clock starts on the first keystroke.
    pub fn started(&self) -> bool {
        self.started_at.is_some()
    }

    /// Whether the test is over.
    pub fn finished(&self) -> bool {
        self.finished_at.is_some()
    }

    /// Milliseconds elapsed since the first keystroke.
    pub fn elapsed_ms(&self, now: i64) -> i64 {
        match (self.started_at, self.finished_at) {
            (None, _) => 0,
            (Some(started), None) => now - started,
            (Some(started), Some(finished)) => finished - started,
        }
    }

    /// Seconds left in a timed test, rounded up. `None` in other modes.
    pub fn remaining_seconds(&self, now: i64) -> Option<i64> {
        if self.mode != Mode::Time {
            return None;
        }

        let left = (self.limit * 1000 - self.elapsed_ms(now)).max(0);
        Some((left as f64 / 1000.0).ceil() as i64)
    }

    /// How far through the words the typist is, as `(done, total)`.
    pub fn progress(&self) -> (usize, usize) {
        (self.index, self.words.len())
    }

    /// True when the word supply is running low and should be topped up.
    pub fn needs_words(&self, lookahead: usize) -> bool {
        self.mode == Mode::Time && self.words.len().saturating_sub(self.index) < lookahead
    }

    /// Appends more words to an in-flight timed test.
    pub fn extend(&mut self, more: Vec<String>) {
        self.words.extend(more);
    }

    /// Graph samples in chronological order.
    pub fn samples(&self) -> &[Sample] {
        &self.samples
    }

    /// Applies a keystroke.
    ///
    /// Keystrokes after the test has finished are ignored, so a fast typist's
    /// trailing characters cannot corrupt the result.
    pub fn key(&mut self, key: Key, now: i64) {
        if self.finished() {
            return;
        }

        match key {
            Key::Char(' ') | Key::Space => self.space(now),
            Key::Char(character) => self.character(character, now),
            Key::Backspace if self.buf.is_empty() => self.step_back(Restore::Text),
            Key::BackspaceWord if self.buf.is_empty() => self.step_back(Restore::Cleared),
            Key::Backspace => {
                self.buf.pop();
            }
            Key::BackspaceWord => self.buf.clear(),
        }
    }

    fn character(&mut self, character: char, now: i64) {
        self.start(now);

        let expected = self.current_target().chars().nth(self.buf.chars().count());

        self.buf.push(character);
        self.count_key(expected == Some(character));
        self.track_letter(expected, character);
        self.maybe_finish_on_last_word(now);
    }

    fn space(&mut self, now: i64) {
        if self.buf.is_empty() {
            return;
        }

        self.commit(true, now);
    }

    fn start(&mut self, now: i64) {
        self.started_at.get_or_insert(now);
    }

    fn count_key(&mut self, correct: bool) {
        self.sample_chars += 1;

        if correct {
            self.keys_correct += 1;
        } else {
            self.keys_incorrect += 1;
            self.sample_errors += 1;
        }
    }

    /// Mistakes are attributed to the letter that should have been typed, which
    /// is what makes them actionable: "you miss `e`" is advice, "you press `r` a
    /// lot" is not. Characters typed past the end of a word have no expected
    /// letter, so they count against accuracy but not against any key.
    fn track_letter(&mut self, expected: Option<char>, actual: char) {
        let Some(expected) = expected else {
            return;
        };

        *self.key_attempts.entry(expected).or_insert(0) += 1;

        if expected != actual {
            *self.key_errors.entry(expected).or_insert(0) += 1;
            *self.slips.entry((expected, actual)).or_insert(0) += 1;
        }
    }

    /// In word and quote modes the last word needs no trailing space.
    fn maybe_finish_on_last_word(&mut self, now: i64) {
        if self.mode == Mode::Time {
            return;
        }

        let last = self.index + 1 == self.words.len();

        if last && self.buf == self.current_target() {
            self.commit(false, now);
            self.finish(now);
        }
    }

    fn commit(&mut self, space_pressed: bool, now: i64) {
        let correct = self.buf == self.current_target();

        if space_pressed {
            self.count_key(correct);
            self.spaces += 1;

            if correct {
                self.correct_spaces += 1;
            }
        }

        self.typed.push(std::mem::take(&mut self.buf));
        self.index += 1;

        if self.mode != Mode::Time && self.index >= self.words.len() {
            self.finish(now);
        }
    }

    fn step_back(&mut self, restore: Restore) {
        if self.index == 0 {
            return;
        }

        let previous_index = self.index - 1;
        let Some(previous) = self.typed.last().cloned() else {
            return;
        };

        let has_mistake = self.words.get(previous_index) != Some(&previous);

        if self.backtrack == Backtrack::Free || has_mistake {
            self.index = previous_index;
            self.typed.pop();
            self.buf = match restore {
                Restore::Text => previous,
                Restore::Cleared => String::new(),
            };
        }
    }

    /// Advances the clock.
    ///
    /// Closes off any whole seconds that have passed into graph samples, and
    /// ends a timed test once its limit is reached.
    pub fn tick(&mut self, now: i64) {
        if !self.started() || self.finished() {
            return;
        }

        self.collect_samples(now);

        if self.mode == Mode::Time && self.elapsed_ms(now) >= self.limit * 1000 {
            self.finish(now);
        }
    }

    /// Ends the test early, as when the typist quits mid-run.
    pub fn finish(&mut self, now: i64) {
        if self.finished() {
            return;
        }

        let Some(started_at) = self.started_at else {
            self.started_at = Some(now);
            self.finished_at = Some(now);
            return;
        };

        // A timed test always reports exactly its limit, so a late tick cannot
        // deflate the WPM by stretching the denominator.
        let finished_at = if self.mode == Mode::Time {
            now.min(started_at + self.limit * 1000)
        } else {
            now
        };

        self.collect_samples(finished_at);
        self.close_partial_sample(finished_at);
        self.finished_at = Some(finished_at);
    }

    /// Closes off every whole second that has elapsed since the last sample.
    fn collect_samples(&mut self, now: i64) {
        let elapsed = self.elapsed_ms(now);

        while elapsed - self.sampled_ms >= 1000 {
            let at = self.sampled_ms + 1000;

            self.samples.push(Sample {
                at: at as f64 / 1000.0,
                raw: wpm_from(self.sample_chars, 1000.0),
                errors: self.sample_errors,
            });

            self.sample_chars = 0;
            self.sample_errors = 0;
            self.sampled_ms = at;
        }
    }

    /// The tail end of a test is rarely a whole second; keep it if it is long
    /// enough to be meaningful, otherwise its characters are dropped from the
    /// graph only — never from the score.
    fn close_partial_sample(&mut self, now: i64) {
        let leftover = self.elapsed_ms(now) - self.sampled_ms;

        if leftover >= 250 && self.sample_chars > 0 {
            self.samples.push(Sample {
                at: (self.sampled_ms + leftover) as f64 / 1000.0,
                raw: wpm_from(self.sample_chars, leftover as f64),
                errors: self.sample_errors,
            });

            self.sample_chars = 0;
            self.sample_errors = 0;
        }
    }

    /// The words paired with what was typed for them, for rendering.
    pub fn annotate(&self) -> Vec<Word<'_>> {
        self.words
            .iter()
            .enumerate()
            .map(|(index, target)| {
                let (typed, status) = if index < self.index {
                    (
                        self.typed.get(index).map_or("", String::as_str),
                        Status::Done,
                    )
                } else if index == self.index {
                    (self.buf.as_str(), Status::Current)
                } else {
                    ("", Status::Pending)
                };

                Word {
                    index,
                    target: target.as_str(),
                    typed,
                    status,
                }
            })
            .collect()
    }
}

fn wpm_from(chars: u32, ms: f64) -> f64 {
    chars as f64 / 5.0 * 60_000.0 / ms
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|word| word.to_string()).collect()
    }

    fn engine() -> Engine {
        Engine::new(
            Mode::Time,
            30,
            words(&["the", "quick", "brown", "fox"]),
            Backtrack::Strict,
        )
    }

    fn engine_with(mode: Mode, limit: i64, list: &[&str]) -> Engine {
        Engine::new(mode, limit, words(list), Backtrack::Strict)
    }

    fn type_at(engine: &mut Engine, text: &str, now: i64) {
        for character in text.chars() {
            engine.key(Key::Char(character), now);
        }
    }

    fn type_text(engine: &mut Engine, text: &str) {
        type_at(engine, text, 0);
    }

    fn tally(pairs: &[(char, u32)]) -> LetterTally {
        pairs.iter().copied().collect()
    }

    // typing

    #[test]
    fn the_clock_starts_on_the_first_keystroke_not_on_creation() {
        let mut engine = engine();
        assert!(!engine.started());

        type_at(&mut engine, "t", 1_000);

        assert!(engine.started());
        assert_eq!(engine.elapsed_ms(3_000), 2_000);
    }

    #[test]
    fn space_commits_a_word_and_moves_to_the_next() {
        let mut engine = engine();
        type_text(&mut engine, "the");
        engine.key(Key::Space, 0);

        assert_eq!(engine.index, 1);
        assert_eq!(engine.typed(), ["the"]);
        assert_eq!(engine.buf, "");
        assert_eq!(engine.current_target(), "quick");
    }

    #[test]
    fn space_is_ignored_before_anything_has_been_typed() {
        let mut engine = engine();
        engine.key(Key::Space, 0);

        assert_eq!(engine.index, 0);
        assert!(!engine.started());
    }

    #[test]
    fn characters_typed_past_the_end_of_a_word_are_kept_as_extras() {
        let mut engine = engine();
        type_text(&mut engine, "theee");

        assert_eq!(engine.buf, "theee");
        assert_eq!(engine.keys_correct, 3);
        assert_eq!(engine.keys_incorrect, 2);
    }

    #[test]
    fn a_word_can_be_committed_wrong_and_left_behind() {
        let mut engine = engine();
        type_text(&mut engine, "teh");
        engine.key(Key::Space, 0);

        assert_eq!(engine.typed(), ["teh"]);
        assert_eq!(engine.index, 1);
        assert_eq!(engine.correct_spaces, 0);
        assert_eq!(engine.spaces, 1);
    }

    #[test]
    fn accuracy_is_judged_at_the_moment_a_key_is_pressed() {
        let mut engine = engine();
        type_text(&mut engine, "th");
        engine.key(Key::Backspace, 0);
        type_text(&mut engine, "e");

        // The typo was corrected, but it still happened.
        assert_eq!(engine.buf, "te");
        assert_eq!(engine.keys_correct, 2);
        assert_eq!(engine.keys_incorrect, 1);
    }

    #[test]
    fn a_typed_space_is_treated_as_a_commit() {
        let mut engine = engine();
        type_text(&mut engine, "the");
        engine.key(Key::Char(' '), 0);

        assert_eq!(engine.index, 1);
        assert_eq!(engine.typed(), ["the"]);
    }

    // backspace

    #[test]
    fn backspace_deletes_one_character_at_a_time() {
        let mut engine = engine();
        type_text(&mut engine, "the");
        engine.key(Key::Backspace, 0);

        assert_eq!(engine.buf, "th");
    }

    #[test]
    fn ctrl_w_clears_the_whole_word() {
        let mut engine = engine();
        type_text(&mut engine, "the");
        engine.key(Key::BackspaceWord, 0);

        assert_eq!(engine.buf, "");
        assert_eq!(engine.index, 0);
    }

    #[test]
    fn steps_back_into_a_previous_word_that_has_a_mistake() {
        let mut engine = engine();
        type_text(&mut engine, "teh");
        engine.key(Key::Space, 0);
        engine.key(Key::Backspace, 0);

        assert_eq!(engine.index, 0);
        assert_eq!(engine.buf, "teh");
        assert!(engine.typed().is_empty());
    }

    #[test]
    fn refuses_to_step_back_into_a_correctly_typed_word() {
        let mut engine = engine();
        type_text(&mut engine, "the");
        engine.key(Key::Space, 0);
        engine.key(Key::Backspace, 0);

        assert_eq!(engine.index, 1);
        assert_eq!(engine.buf, "");
    }

    #[test]
    fn free_backspace_allows_stepping_back_into_a_correct_word() {
        let mut engine = Engine::new(Mode::Time, 30, words(&["the", "quick"]), Backtrack::Free);
        type_text(&mut engine, "the");
        engine.key(Key::Space, 0);
        engine.key(Key::Backspace, 0);

        assert_eq!(engine.index, 0);
        assert_eq!(engine.buf, "the");
    }

    #[test]
    fn backspace_does_nothing_at_the_very_start() {
        let mut engine = engine();
        engine.key(Key::Backspace, 0);

        assert_eq!(engine.index, 0);
        assert_eq!(engine.buf, "");
    }

    #[test]
    fn stepping_back_a_whole_word_clears_the_buffer() {
        let mut engine = engine();
        type_text(&mut engine, "teh");
        engine.key(Key::Space, 0);
        engine.key(Key::BackspaceWord, 0);

        assert_eq!(engine.index, 0);
        assert_eq!(engine.buf, "");
    }

    // finishing

    #[test]
    fn word_mode_ends_when_the_last_word_is_typed_correctly_without_a_space() {
        let mut engine = engine_with(Mode::Words, 4, &["a", "b", "c", "d"]);

        for word in ["a", "b", "c"] {
            type_text(&mut engine, word);
            engine.key(Key::Space, 0);
        }
        type_text(&mut engine, "d");

        assert!(engine.finished());
        assert_eq!(engine.typed(), ["a", "b", "c", "d"]);
    }

    #[test]
    fn word_mode_ends_on_a_space_after_the_last_word_even_when_it_is_wrong() {
        let mut engine = engine_with(Mode::Words, 1, &["alpha"]);
        type_text(&mut engine, "alpga");
        engine.key(Key::Space, 0);

        assert!(engine.finished());
    }

    #[test]
    fn timed_mode_ends_once_the_limit_passes() {
        let mut engine = engine_with(Mode::Time, 30, &["the", "quick"]);
        type_at(&mut engine, "the", 0);

        engine.tick(29_999);
        assert!(!engine.finished());

        engine.tick(30_100);
        assert!(engine.finished());
    }

    #[test]
    fn a_timed_test_reports_exactly_its_limit_even_if_the_tick_lands_late() {
        let mut engine = engine_with(Mode::Time, 15, &["the", "quick"]);
        type_at(&mut engine, "the", 0);
        engine.tick(15_400);

        assert_eq!(engine.elapsed_ms(20_000), 15_000);
    }

    #[test]
    fn keystrokes_after_the_end_are_ignored() {
        let mut engine = engine_with(Mode::Time, 5, &["the", "quick"]);
        type_at(&mut engine, "the", 0);
        engine.tick(5_000);
        type_at(&mut engine, "xxxx", 5_100);

        assert_eq!(engine.buf, "the");
        assert_eq!(engine.keys_incorrect, 0);
    }

    #[test]
    fn finishing_a_test_that_never_started_still_ends_it() {
        let mut engine = engine();
        engine.finish(1_234);

        assert!(engine.finished());
        assert_eq!(engine.elapsed_ms(9_999), 0);
    }

    // word supply

    #[test]
    fn a_timed_test_asks_for_more_words_as_it_runs_low() {
        let mut engine = engine_with(Mode::Time, 60, &["the", "quick", "brown", "fox"]);

        assert!(engine.needs_words(10));
        assert!(!engine.needs_words(3));

        engine.extend(words(&["five", "six", "seven", "eight"]));
        assert_eq!(engine.words.len(), 8);
    }

    #[test]
    fn fixed_length_tests_never_ask_for_more() {
        assert!(!engine_with(Mode::Words, 4, &["a", "b"]).needs_words(100));
    }

    // samples

    #[test]
    fn one_sample_is_recorded_per_elapsed_second() {
        let mut engine = engine_with(Mode::Time, 60, &["the", "quick"]);
        type_at(&mut engine, "the", 0);
        engine.tick(3_200);

        let samples = engine.samples();

        assert_eq!(samples.len(), 3);
        assert_eq!(
            samples.iter().map(|sample| sample.at).collect::<Vec<_>>(),
            [1.0, 2.0, 3.0]
        );
        // Three characters landed in the first second: 3/5 of a word in 1/60 min.
        assert_eq!(samples[0].raw, 36.0);
    }

    #[test]
    fn errors_are_attributed_to_the_second_they_happened_in() {
        let mut engine = engine_with(Mode::Time, 60, &["the", "quick"]);
        type_at(&mut engine, "teh", 0);
        engine.tick(1_000);

        assert_eq!(engine.samples().len(), 1);
        assert_eq!(engine.samples()[0].errors, 2);
    }

    #[test]
    fn a_long_enough_tail_is_kept_as_a_partial_sample() {
        let mut engine = engine_with(Mode::Time, 60, &["the", "quick"]);
        type_at(&mut engine, "the", 0);
        engine.tick(1_000);
        type_at(&mut engine, "qu", 1_200);
        engine.finish(1_500);

        assert_eq!(
            engine
                .samples()
                .iter()
                .map(|sample| sample.at)
                .collect::<Vec<_>>(),
            [1.0, 1.5]
        );
    }

    #[test]
    fn a_negligible_tail_is_dropped() {
        let mut engine = engine_with(Mode::Time, 60, &["the", "quick"]);
        type_at(&mut engine, "the", 0);
        engine.tick(1_000);
        type_at(&mut engine, "q", 1_050);
        engine.finish(1_100);

        assert_eq!(engine.samples().len(), 1);
    }

    // Characters are counted when typed but only bucketed when the clock is
    // advanced, so an unticked test puts everything in the first second.
    #[test]
    fn a_tail_with_no_characters_of_its_own_is_not_a_sample() {
        let mut engine = engine_with(Mode::Time, 60, &["the", "quick"]);
        type_at(&mut engine, "the", 0);
        engine.finish(1_500);

        assert_eq!(engine.samples().len(), 1);
    }

    // annotate

    #[test]
    fn annotate_labels_every_word_by_how_far_the_typist_has_got() {
        let mut engine = engine();
        type_text(&mut engine, "the");
        engine.key(Key::Space, 0);
        type_text(&mut engine, "qu");

        assert_eq!(
            engine.annotate(),
            vec![
                Word {
                    index: 0,
                    target: "the",
                    typed: "the",
                    status: Status::Done
                },
                Word {
                    index: 1,
                    target: "quick",
                    typed: "qu",
                    status: Status::Current
                },
                Word {
                    index: 2,
                    target: "brown",
                    typed: "",
                    status: Status::Pending
                },
                Word {
                    index: 3,
                    target: "fox",
                    typed: "",
                    status: Status::Pending
                },
            ]
        );
    }

    // per-letter tracking

    #[test]
    fn counts_every_letter_that_was_attempted() {
        let mut engine = engine();
        type_text(&mut engine, "the");

        assert_eq!(engine.key_attempts, tally(&[('t', 1), ('h', 1), ('e', 1)]));
        assert!(engine.key_errors.is_empty());
        assert!(engine.slips.is_empty());
    }

    #[test]
    fn blames_the_letter_that_should_have_been_typed_not_the_one_that_was() {
        let mut engine = engine();
        type_text(&mut engine, "tje");

        assert_eq!(engine.key_attempts, tally(&[('t', 1), ('h', 1), ('e', 1)]));
        assert_eq!(engine.key_errors, tally(&[('h', 1)]));
        assert_eq!(
            engine.slips,
            [(('h', 'j'), 1)].into_iter().collect::<SlipTally>()
        );
    }

    #[test]
    fn tallies_repeats_of_the_same_mistake() {
        let mut engine = engine_with(Mode::Time, 30, &["the", "the"]);
        type_text(&mut engine, "tje");
        engine.key(Key::Space, 0);
        type_text(&mut engine, "tje");

        assert_eq!(engine.key_errors, tally(&[('h', 2)]));
        assert_eq!(
            engine.slips,
            [(('h', 'j'), 2)].into_iter().collect::<SlipTally>()
        );
    }

    #[test]
    fn characters_typed_past_the_end_of_a_word_are_blamed_on_no_letter() {
        let mut engine = engine();
        type_text(&mut engine, "theee");

        assert_eq!(engine.key_attempts, tally(&[('t', 1), ('h', 1), ('e', 1)]));
        assert!(engine.key_errors.is_empty());
        // The extras still count against accuracy, they just have no owner.
        assert_eq!(engine.keys_incorrect, 2);
    }

    #[test]
    fn a_corrected_letter_keeps_its_mistake_on_the_record() {
        let mut engine = engine();
        type_text(&mut engine, "tj");
        engine.key(Key::Backspace, 0);
        type_text(&mut engine, "h");

        assert_eq!(engine.key_attempts, tally(&[('t', 1), ('h', 2)]));
        assert_eq!(engine.key_errors, tally(&[('h', 1)]));
    }

    // remaining_seconds

    #[test]
    fn remaining_seconds_counts_down_and_stops_at_zero() {
        let mut engine = engine_with(Mode::Time, 30, &["the", "quick"]);
        type_at(&mut engine, "t", 0);

        assert_eq!(engine.remaining_seconds(0), Some(30));
        assert_eq!(engine.remaining_seconds(500), Some(30));
        assert_eq!(engine.remaining_seconds(29_500), Some(1));
        assert_eq!(engine.remaining_seconds(30_000), Some(0));
    }

    #[test]
    fn remaining_seconds_is_undefined_outside_timed_mode() {
        assert_eq!(
            engine_with(Mode::Words, 10, &["a"]).remaining_seconds(0),
            None
        );
    }
}
