//! Formats a [`Summary`] for the shell.
//!
//! Plain text rather than the ANSI the test screen uses, so the output survives
//! being piped into a pager or a file.

use crate::datetime;
use crate::summary::{self, Summary};

const SPARKLINE: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
const LABEL_WIDTH: usize = 16;
const VALUE_WIDTH: usize = 10;

/// The least space allowed between a value and the note beside it.
const MIN_GAP: usize = 2;

/// Renders a summary as the text printed by `typr --stats`.
pub fn render(summary: &Summary) -> String {
    if summary.tests == 0 {
        return "no results yet — run typr a few times and come back\n".to_string();
    }

    let mut lines = vec![headline(summary), String::new()];

    lines.extend(overview(summary));
    lines.extend(trend(summary));
    lines.extend(by_config(summary));
    lines.extend(trouble_keys(summary));
    lines.extend(slips(summary));

    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// Draws a sequence of values as a one-line sparkline.
///
/// The scale spans the values themselves rather than starting at zero, so small
/// differences between good scores stay visible.
pub fn sparkline(values: &[f64]) -> String {
    if values.is_empty() {
        return String::new();
    }

    let low = values.iter().copied().fold(f64::INFINITY, f64::min);
    let high = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let span = high - low;

    values
        .iter()
        .map(|value| {
            let position = if span == 0.0 {
                0.5
            } else {
                (value - low) / span
            };
            SPARKLINE[((position * 8.0) as usize).min(7)]
        })
        .collect()
}

fn headline(summary: &Summary) -> String {
    format!(
        "typr — {} · {} typing · {} words",
        pluralize(summary.tests as u64, "test"),
        summary::humanize_ms(summary.typing_ms),
        commify(summary.words_typed)
    )
}

fn overview(summary: &Summary) -> Vec<String> {
    let mut rows = Vec::new();

    if let Some(best) = &summary.best {
        rows.push(row(
            "best",
            &format!("{} wpm", best.wpm.round()),
            Some(&format!("{}, {}", best.config, ago(&best.at))),
        ));
    }

    rows.push(row("average", &wpm(summary.average_wpm), None));
    rows.push(row(
        "last 10",
        &wpm(summary.recent_average),
        improvement(summary.improvement).as_deref(),
    ));
    rows.push(row("accuracy", &percent(summary.average_accuracy), None));
    rows.push(row(
        "consistency",
        &percent(summary.average_consistency),
        None,
    ));
    rows.push(row(
        "practised",
        &pluralize(summary.days_practiced as u64, "day"),
        streak(summary.streak).as_deref(),
    ));
    rows.push(row(
        "last test",
        &summary.last_at.as_deref().map_or("never".to_string(), ago),
        None,
    ));

    rows
}

fn trend(summary: &Summary) -> Vec<String> {
    if summary.trend.len() < 2 {
        return Vec::new();
    }

    let low = summary.trend.iter().copied().fold(f64::INFINITY, f64::min);
    let high = summary
        .trend
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);

    vec![
        String::new(),
        row(
            "recent",
            &sparkline(&summary.trend),
            Some(&format!("{}–{} wpm", low.round(), high.round())),
        ),
    ]
}

fn by_config(summary: &Summary) -> Vec<String> {
    let width = summary
        .by_config
        .iter()
        .map(|config| config.config.chars().count())
        .max()
        .unwrap_or(0);

    let mut lines = vec![String::new(), "by test".to_string()];

    lines.extend(summary.by_config.iter().map(|config| {
        format!(
            "  {:width$}  {:>4} {}   best {:>3}   avg {:>3}",
            config.config,
            config.tests,
            if config.tests == 1 { "test " } else { "tests" },
            config.best.round(),
            config.average.round(),
        )
    }));

    lines
}

fn trouble_keys(summary: &Summary) -> Vec<String> {
    if summary.trouble_keys.is_empty() {
        return Vec::new();
    }

    let mut lines = vec![String::new(), "trouble keys".to_string()];

    lines.extend(summary.trouble_keys.iter().map(|key| {
        format!(
            "  {}   {:>6}   {} missed of {}",
            display_key(key.key),
            percent(Some(key.accuracy)),
            commify(u64::from(key.errors)),
            commify(u64::from(key.attempts)),
        )
    }));

    lines
}

fn slips(summary: &Summary) -> Vec<String> {
    if summary.slips.is_empty() {
        return Vec::new();
    }

    let mut lines = vec![String::new(), "most often typed instead".to_string()];

    lines.extend(summary.slips.iter().map(|slip| {
        format!(
            "  {} → {}   {}",
            display_key(slip.expected),
            display_key(slip.actual),
            commify(u64::from(slip.count)),
        )
    }));

    lines
}

fn row(label: &str, value: &str, note: Option<&str>) -> String {
    match note {
        None => format!("  {label:LABEL_WIDTH$}{value}"),
        Some(note) => {
            // The note usually starts at a fixed column, but a value wider than
            // that column — a long sparkline — would otherwise run straight into
            // it, so a gap is always left.
            let gap = VALUE_WIDTH
                .saturating_sub(value.chars().count())
                .max(MIN_GAP);

            format!("  {label:LABEL_WIDTH$}{value}{}{note}", " ".repeat(gap))
        }
    }
}

fn wpm(value: Option<f64>) -> String {
    value.map_or_else(
        || "--".to_string(),
        |value| format!("{} wpm", value.round()),
    )
}

fn improvement(delta: Option<f64>) -> Option<String> {
    let delta = delta?;
    let rounded = delta.round();

    Some(if rounded >= 0.0 {
        format!("+{rounded} on the 10 before")
    } else {
        format!("{rounded} on the 10 before")
    })
}

fn streak(days: u32) -> Option<String> {
    match days {
        0 => None,
        1 => Some("typed today".to_string()),
        days => Some(format!("{days} day streak")),
    }
}

/// Space and punctuation need naming, or the column silently swallows them.
fn display_key(key: char) -> String {
    if key == ' ' {
        "space".to_string()
    } else {
        key.to_string()
    }
}

fn percent(value: Option<f64>) -> String {
    value.map_or_else(
        || "--".to_string(),
        |value| format!("{:.1}%", (value * 10.0).round() / 10.0),
    )
}

fn ago(at: &str) -> String {
    let Some(date) = datetime::parse_date(at) else {
        return at.to_string();
    };

    match datetime::today().diff(date) {
        0 => "today".to_string(),
        1 => "yesterday".to_string(),
        days if days < 30 => format!("{days} days ago"),
        days => format!("{} months ago", days / 30),
    }
}

fn pluralize(count: u64, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{} {noun}s", commify(count))
    }
}

/// Groups thousands so long numbers can be read at a glance.
fn commify(number: u64) -> String {
    let digits = number.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);

    for (position, digit) in digits.chars().enumerate() {
        if position > 0 && (digits.len() - position).is_multiple_of(3) {
            out.push(',');
        }

        out.push(digit);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datetime::days_ago;
    use crate::engine::{Mode, SlipTally};
    use crate::history::Record;
    use crate::stats::{KeyTallies, KeyTally};
    use crate::summary::TroubleOptions;
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

    fn summarise(history: &[Record]) -> Summary {
        Summary::build(history, TroubleOptions::default())
    }

    fn rendered(history: &[Record]) -> String {
        render(&summarise(history))
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

    #[test]
    fn says_so_plainly_when_there_is_nothing_to_report() {
        assert!(render(&summarise(&[])).contains("no results yet"));
    }

    #[test]
    fn reports_the_headline_figures() {
        let history = [record(60.0), record(90.0), record(72.0)];
        let output = rendered(&history);

        assert!(output.contains("3 tests"), "{output}");
        assert!(output.contains("best"));
        assert!(output.contains("90 wpm"));
        assert!(output.contains("average"));
        assert!(output.contains("74 wpm"));
        assert!(output.contains("accuracy"));
        assert!(output.contains("96.0%"));
        assert!(output.contains("time-30-english"));
    }

    #[test]
    fn lists_trouble_keys_and_slips_when_there_are_any() {
        let history = [Record {
            keys: keys(&[('e', 100, 12), ('t', 100, 0)]),
            slips: [(('e', 'r'), 12)].into_iter().collect(),
            ..record(70.0)
        }];

        let output = rendered(&history);

        assert!(output.contains("trouble keys"));
        assert!(output.contains("88.0%"));
        assert!(output.contains("12 missed of 100"));
        assert!(output.contains("most often typed instead"));
        assert!(output.contains("e → r"));
    }

    #[test]
    fn leaves_out_sections_that_have_nothing_in_them() {
        let output = rendered(&[record(70.0)]);

        assert!(!output.contains("trouble keys"));
        assert!(!output.contains("most often typed instead"));
    }

    #[test]
    fn names_the_space_bar_rather_than_printing_a_blank() {
        let history = [Record {
            keys: keys(&[(' ', 200, 20)]),
            slips: [((' ', 'n'), 20)].into_iter().collect(),
            ..record(70.0)
        }];

        let output = rendered(&history);

        assert!(output.contains("space"));
        assert!(output.contains("space → n"));
    }

    #[test]
    fn groups_long_numbers_so_they_can_be_read_at_a_glance() {
        let history: Vec<Record> = (0..40)
            .map(|_| Record {
                correct: 5_000,
                ..record(70.0)
            })
            .collect();

        assert!(rendered(&history).contains("40,000 words"));
    }

    #[test]
    fn a_note_keeps_its_column_when_the_value_is_short() {
        assert_eq!(
            row("average", "72 wpm", Some("note")),
            "  average         72 wpm    note"
        );
    }

    #[test]
    fn a_value_wider_than_its_column_does_not_run_into_the_note() {
        // A full 20-test sparkline is twice the width of the value column.
        let line = row("recent", &"▃".repeat(20), Some("62–87 wpm"));

        assert!(line.ends_with("▃  62–87 wpm"), "{line}");
    }

    #[test]
    fn commify_groups_thousands() {
        assert_eq!(commify(0), "0");
        assert_eq!(commify(999), "999");
        assert_eq!(commify(1_000), "1,000");
        assert_eq!(commify(1_234_567), "1,234,567");
    }

    // sparkline

    #[test]
    fn sparkline_is_empty_for_no_values() {
        assert_eq!(sparkline(&[]), "");
    }

    #[test]
    fn sparkline_draws_one_mark_per_value_lowest_to_highest() {
        let line = sparkline(&[10.0, 20.0, 30.0, 40.0]);
        let marks: Vec<char> = line.chars().collect();

        assert_eq!(marks.len(), 4);
        assert_eq!(marks[0], '▁');
        assert_eq!(marks[3], '█');
    }

    #[test]
    fn a_flat_run_sits_in_the_middle_rather_than_dividing_by_zero() {
        assert_eq!(sparkline(&[50.0, 50.0, 50.0]), "▅▅▅");
    }
}
