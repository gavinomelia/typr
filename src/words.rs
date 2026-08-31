//! Word lists and test-text generation.
//!
//! Two vocabularies ship with `typr`: `english`, the most frequent words in the
//! language, and `english_extended`, which adds longer and less common words.
//! Both can be decorated with punctuation and numbers the way monkeytype does.

use crate::rng::Rng;
use crate::words_data::{ENGLISH, ENGLISH_EXTENDED, SENTENCES};

const PUNCTUATION_MARKS: &[&str] = &[".", ".", ".", ",", ",", ";", ":", "!", "?"];
const BRACKETS: &[(&str, &str)] = &[("(", ")"), ("\"", "\""), ("'", "'"), ("[", "]")];
const SENTENCE_ENDINGS: &[&str] = &[".", "!", "?"];

/// Names of the available word lists.
pub fn list_names() -> &'static [&'static str] {
    &["english", "english_extended"]
}

/// The raw vocabulary behind a list name.
pub fn vocabulary(name: &str) -> Option<&'static [&'static str]> {
    match name {
        "english" => Some(ENGLISH),
        "english_extended" => Some(ENGLISH_EXTENDED),
        _ => None,
    }
}

/// A random sentence for quote mode, as a list of words.
pub fn quote_words(rng: &mut Rng) -> Vec<String> {
    rng.choose(SENTENCES)
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// What to mix into otherwise plain text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Decoration {
    pub punctuation: bool,
    pub numbers: bool,
}

/// Generates `count` words from `list`, applying `decoration`.
///
/// Consecutive duplicates are avoided so the text reads like prose rather than
/// a stutter. When punctuation is on, the first word is capitalized and every
/// sentence-ending mark capitalizes the word that follows it.
pub fn generate(list: &str, count: usize, decoration: Decoration, rng: &mut Rng) -> Vec<String> {
    let vocab = vocabulary(list).unwrap_or(ENGLISH);
    let mut words = random_words(vocab, count, rng);

    if decoration.numbers {
        add_numbers(&mut words, rng);
    }

    if decoration.punctuation {
        add_punctuation(&mut words, rng);
    }

    words
}

fn random_words(vocab: &[&str], count: usize, rng: &mut Rng) -> Vec<String> {
    let mut words: Vec<String> = Vec::with_capacity(count);
    let mut previous: Option<&str> = None;

    for _ in 0..count {
        let word = draw(vocab, previous, rng);
        previous = Some(word);
        words.push(word.to_string());
    }

    words
}

fn draw<'a>(vocab: &'a [&str], previous: Option<&str>, rng: &mut Rng) -> &'a str {
    loop {
        let word = *rng.choose(vocab);
        if Some(word) != previous {
            return word;
        }
    }
}

fn add_numbers(words: &mut [String], rng: &mut Rng) {
    for word in words.iter_mut() {
        if rng.fraction() < 0.12 {
            let digits = rng.between(1, 4);
            let ceiling = 10u32.pow(digits) - 1;
            *word = rng.between(0, ceiling).to_string();
        }
    }
}

/// Walks the list carrying "does the next word start a sentence?" so capitals
/// land where a reader would expect them.
fn add_punctuation(words: &mut [String], rng: &mut Rng) {
    let mut sentence_start = true;

    for word in words.iter_mut() {
        let (punctuated, ends_sentence) = punctuate(word, rng);

        *word = if sentence_start {
            capitalize(&punctuated)
        } else {
            punctuated
        };

        sentence_start = ends_sentence;
    }
}

fn punctuate(word: &str, rng: &mut Rng) -> (String, bool) {
    let roll = rng.fraction();

    if roll < 0.06 {
        (wrap(word, rng), false)
    } else if roll < 0.10 {
        (format!("{word}'s"), false)
    } else if roll < 0.30 {
        append_mark(word, rng)
    } else {
        (word.to_string(), false)
    }
}

fn append_mark(word: &str, rng: &mut Rng) -> (String, bool) {
    let mark = *rng.choose(PUNCTUATION_MARKS);
    (format!("{word}{mark}"), SENTENCE_ENDINGS.contains(&mark))
}

fn wrap(word: &str, rng: &mut Rng) -> String {
    let (open, close) = *rng.choose(BRACKETS);
    format!("{open}{word}{close}")
}

/// Capitalizes the first letter even when the word opens with a bracket or
/// quote, and leaves the rest of the word alone.
fn capitalize(word: &str) -> String {
    let mut characters = word.chars();

    match characters.next() {
        None => String::new(),
        Some(opener) if matches!(opener, '(' | '"' | '\'' | '[') => {
            format!("{opener}{}", upcase_first(characters.as_str()))
        }
        Some(_) => upcase_first(word),
    }
}

fn upcase_first(word: &str) -> String {
    let mut characters = word.chars();

    match characters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const PUNCTUATED: Decoration = Decoration {
        punctuation: true,
        numbers: false,
    };
    const NUMBERED: Decoration = Decoration {
        punctuation: false,
        numbers: true,
    };
    const BOTH: Decoration = Decoration {
        punctuation: true,
        numbers: true,
    };

    fn rng() -> Rng {
        Rng::seeded(20260831)
    }

    fn starts_a_sentence(word: &str) -> bool {
        let mut characters = word.chars();

        let first = match characters.next() {
            Some(character) => character,
            None => return false,
        };

        if matches!(first, '(' | '"' | '\'' | '[') {
            characters.next().is_some_and(|next| next.is_uppercase())
        } else {
            first.is_uppercase()
        }
    }

    #[test]
    fn produces_the_requested_number_of_words() {
        assert_eq!(
            generate("english", 25, Decoration::default(), &mut rng()).len(),
            25
        );
    }

    #[test]
    fn never_repeats_a_word_back_to_back() {
        let words = generate("english", 500, Decoration::default(), &mut rng());

        assert!(words.windows(2).all(|pair| pair[0] != pair[1]));
    }

    #[test]
    fn draws_only_from_the_requested_vocabulary() {
        let vocabulary: HashSet<&str> = vocabulary("english_extended")
            .unwrap()
            .iter()
            .copied()
            .collect();

        assert!(
            generate("english_extended", 100, Decoration::default(), &mut rng())
                .iter()
                .all(|word| vocabulary.contains(word.as_str()))
        );
    }

    #[test]
    fn falls_back_to_the_default_vocabulary_for_an_unknown_list() {
        assert_eq!(
            generate("klingon", 10, Decoration::default(), &mut rng()).len(),
            10
        );
    }

    #[test]
    fn capitalises_the_opening_word() {
        for seed in 0..20 {
            let words = generate("english", 10, PUNCTUATED, &mut Rng::seeded(seed));

            assert!(
                starts_a_sentence(&words[0]),
                "{:?} does not open a sentence",
                words[0]
            );
        }
    }

    #[test]
    fn adds_marks_that_plain_generation_never_produces() {
        let marks = ['.', ',', ';', ':', '!', '?'];

        let plain = generate("english", 300, Decoration::default(), &mut rng()).join(" ");
        let punctuated = generate("english", 300, PUNCTUATED, &mut rng()).join(" ");

        assert!(!plain.contains(marks));
        assert!(punctuated.contains(marks));
    }

    #[test]
    fn a_word_after_a_full_stop_starts_a_new_sentence() {
        let words = generate("english", 400, PUNCTUATED, &mut rng());

        for pair in words.windows(2) {
            let ends_sentence = pair[0].ends_with(['.', '!', '?']);

            if ends_sentence {
                assert!(
                    starts_a_sentence(&pair[1]),
                    "{:?} follows {:?} but is not capitalised",
                    pair[1],
                    pair[0]
                );
            }
        }
    }

    #[test]
    fn mixes_digits_in_when_asked_and_never_otherwise() {
        let with = generate("english", 400, NUMBERED, &mut rng()).join("");
        let without = generate("english", 400, Decoration::default(), &mut rng()).join("");

        assert!(with.chars().any(|character| character.is_ascii_digit()));
        assert!(!without.chars().any(|character| character.is_ascii_digit()));
    }

    #[test]
    fn quote_words_returns_a_sentence_as_separate_words() {
        let words = quote_words(&mut rng());

        assert!(words.len() > 5);
        assert!(words.iter().all(|word| !word.contains(' ')));
    }

    #[test]
    fn every_listed_name_resolves() {
        assert!(list_names().iter().all(|name| vocabulary(name).is_some()));
    }

    #[test]
    fn unknown_names_do_not() {
        assert!(vocabulary("nope").is_none());
    }

    #[test]
    fn lists_hold_no_duplicates_or_stray_whitespace() {
        for name in list_names() {
            let vocabulary = vocabulary(name).unwrap();
            let unique: HashSet<&&str> = vocabulary.iter().collect();

            assert_eq!(unique.len(), vocabulary.len(), "{name} has duplicates");
            assert!(vocabulary.iter().all(|word| word.trim() == *word));
            assert!(vocabulary.iter().all(|word| !word.is_empty()));
        }
    }

    #[test]
    fn the_same_seed_generates_the_same_test() {
        let first = generate("english", 50, BOTH, &mut Rng::seeded(99));
        let second = generate("english", 50, BOTH, &mut Rng::seeded(99));

        assert_eq!(first, second);
    }
}
