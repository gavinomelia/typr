//! Draws frames.
//!
//! Every function here is pure: given a state it returns the frame and the
//! screen position the terminal's own cursor should sit at. Using the real
//! cursor as the caret means it blinks the way the rest of the terminal does,
//! for free.
//!
//! Text is assembled as [`Segment`]s and only turned into escape sequences at
//! the last moment, so widths can be measured without counting invisible bytes.

use crate::engine::{Engine, Mode, Sample, Status, Word};
use crate::stats::{self, Stats};
use crate::summary::TroubleKey;
use crate::theme::{Attrs, Role, Theme};

const VISIBLE_LINES: usize = 3;
const BLOCKS: [char; 7] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇'];
const LABEL_WIDTH: usize = 6;

/// Where the cursor should sit, as a 1-indexed row and column.
pub type Caret = Option<(usize, usize)>;

/// A run of text that shares one colour and set of attributes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub role: Role,
    pub attrs: Attrs,
}

impl Segment {
    fn new(text: impl Into<String>, role: Role, attrs: Attrs) -> Self {
        Segment {
            text: text.into(),
            role,
            attrs,
        }
    }

    fn plain(text: impl Into<String>, role: Role) -> Self {
        Segment::new(text, role, Attrs::NONE)
    }
}

/// Everything the renderer needs to know that is not the test itself.
pub struct View<'a> {
    pub theme: &'a Theme,
    pub size: (usize, usize),
    pub width: usize,
    pub left: usize,
    pub now: i64,
    pub live_wpm: bool,
    pub label: String,
    pub best: Option<String>,
    pub comparison: Option<String>,
    pub trouble: Vec<TroubleKey>,
}

/// A word with the column it starts at, once a line has been laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placed<'a> {
    pub word: Word<'a>,
    pub column: usize,
}

/// Moves the cursor to a 1-indexed row and column.
pub fn move_to(row: usize, column: usize) -> String {
    format!("\x1b[{row};{column}H")
}

/// Draws the typing screen.
///
/// The word window scrolls so the active line sits in the middle once the
/// typist is past the first line, which keeps the eye in one place.
pub fn test_frame(engine: &Engine, view: &View) -> (String, Caret) {
    let annotated = engine.annotate();
    let lines = layout(&annotated, view.width);
    let current_line = line_of(&lines, engine.index);
    let top = current_line.saturating_sub(1);
    let first_row = words_row(view);

    let mut frame = header(engine, view);

    for (offset, line) in lines.iter().skip(top).take(VISIBLE_LINES).enumerate() {
        frame.push_str(&move_to(first_row + offset, view.left));
        emit(&mut frame, &line_segments(line), view.theme);
    }

    frame.push_str(&footer(view, "tab restart · esc quit"));

    let caret = caret_position(&lines, engine, current_line, top, first_row, view);

    (frame, caret)
}

/// Draws the results screen.
///
/// Blocks are stacked in order and the whole stack is centred, so a short
/// terminal simply loses the graph rather than pushing the numbers off screen.
pub fn results_frame(stats: &Stats, view: &View) -> (String, Caret) {
    let (rows, _columns) = view.size;

    let mut blocks = figures_block(stats, view);
    blocks.extend(chart_block(stats, view));
    blocks.extend(detail_block(stats, view));

    let top = (rows.saturating_sub(blocks.len()) / 2).max(1);
    let mut frame = String::new();

    for (offset, segments) in blocks.iter().enumerate() {
        frame.push_str(&move_to(top + offset, view.left));
        emit(&mut frame, segments, view.theme);
    }

    frame.push_str(&footer(view, "tab new test · r repeat · esc quit"));

    (frame, None)
}

/// Draws the message shown when the window is too small to type in.
pub fn too_small_frame(view: &View) -> (String, Caret) {
    let (rows, columns) = view.size;
    let message = format!("terminal too small ({columns}x{rows}) — needs at least 40x8");

    let mut frame = move_to((rows / 2).max(1), 1);
    emit(
        &mut frame,
        &[Segment::plain(message, Role::Incorrect)],
        view.theme,
    );

    (frame, None)
}

/// Packs words into lines that fit `width`.
///
/// A word's footprint is the longer of its target and what was typed for it, so
/// a word that has grown past its length pushes its neighbours along instead of
/// overflowing the column.
pub fn layout<'a>(words: &[Word<'a>], width: usize) -> Vec<Vec<Placed<'a>>> {
    let mut lines: Vec<Vec<Placed<'a>>> = Vec::new();
    let mut line: Vec<Placed<'a>> = Vec::new();
    let mut column = 0;

    for word in words {
        let size = word_width(word);
        let needed = if line.is_empty() { size } else { size + 1 };

        if column + needed > width && !line.is_empty() {
            lines.push(std::mem::take(&mut line));
            line.push(Placed {
                word: *word,
                column: 0,
            });
            column = size;
        } else {
            let start = if line.is_empty() { column } else { column + 1 };
            line.push(Placed {
                word: *word,
                column: start,
            });
            column = start + size;
        }
    }

    if !line.is_empty() {
        lines.push(line);
    }

    lines
}

/// Renders per-second speed as a bar chart.
///
/// The graph always fills the width it is given, so a two minute test and a
/// fifteen second one occupy the same space: a long test averages several
/// seconds into each column, a short one gives each second several columns.
pub fn chart(samples: &[Sample], width: usize, height: usize) -> Vec<Vec<Segment>> {
    if samples.is_empty() {
        return Vec::new();
    }

    let columns = width.saturating_sub(LABEL_WIDTH).max(1);
    let buckets = resample(samples, columns);
    let ceiling = ceiling_for(&buckets);

    let mut rows: Vec<Vec<Segment>> = Vec::new();

    for row in 0..height {
        let level = height - row;

        let cells: String = buckets
            .iter()
            .map(|bucket| bar_cell(bucket.raw / ceiling * height as f64, level))
            .collect();

        let label = if row == 0 {
            pad_left(&ceiling.to_string(), LABEL_WIDTH - 1)
        } else {
            pad_left("", LABEL_WIDTH - 1)
        };

        rows.push(vec![
            Segment::plain(format!("{label} "), Role::Dim),
            Segment::plain(cells, Role::Accent),
        ]);
    }

    rows.push(vec![
        Segment::plain(format!("{} ", pad_left("0", LABEL_WIDTH - 1)), Role::Dim),
        Segment::plain("─".repeat(buckets.len()), Role::Dim),
    ]);

    rows.push(error_row(&buckets));
    rows.push(time_axis(samples, &buckets));

    rows
}

fn figures_block(stats: &Stats, view: &View) -> Vec<Vec<Segment>> {
    let figures = [
        ("wpm", format_number(stats.wpm)),
        ("acc", format!("{}%", format_number(stats.accuracy))),
        ("raw", format_number(stats.raw)),
        ("consistency", percent_or_dash(stats.consistency)),
    ];

    let column = (view.width / figures.len()).clamp(12, 20);

    vec![
        figures
            .iter()
            .map(|(label, _value)| Segment::plain(pad_right(label, column), Role::Dim))
            .collect(),
        figures
            .iter()
            .map(|(_label, value)| {
                Segment::new(pad_right(value, column), Role::Accent, Attrs::BOLD)
            })
            .collect(),
        Vec::new(),
    ]
}

/// The graph is the first thing to go when there is no room for it: the numbers
/// underneath are what people actually read.
fn chart_block(stats: &Stats, view: &View) -> Vec<Vec<Segment>> {
    let (rows, _columns) = view.size;

    if stats.samples.is_empty() {
        return Vec::new();
    }

    let height = rows.saturating_sub(14).min(8);

    if height < 3 {
        return Vec::new();
    }

    let mut block = chart(&stats.samples, view.width, height);
    block.push(Vec::new());
    block
}

fn detail_block(stats: &Stats, view: &View) -> Vec<Vec<Segment>> {
    let mut blocks = vec![detail_segments(stats, view)];

    if let Some(comparison) = &view.comparison {
        blocks.push(vec![Segment::plain(comparison.clone(), Role::Dim)]);
    }

    if let Some(best) = &view.best {
        blocks.push(vec![Segment::plain(best.clone(), Role::Accent)]);
    }

    if !view.trouble.is_empty() {
        blocks.push(trouble_segments(&view.trouble));
    }

    blocks
}

fn trouble_segments(keys: &[TroubleKey]) -> Vec<Segment> {
    let mut segments = vec![Segment::plain("trouble ", Role::Dim)];

    for key in keys {
        segments.push(Segment::plain(
            format!(" {}", display_key(key.key)),
            Role::Incorrect,
        ));
        segments.push(Segment::plain(
            format!(" {}%", format_number(key.accuracy)),
            Role::Dim,
        ));
    }

    segments
}

fn detail_segments(stats: &Stats, view: &View) -> Vec<Segment> {
    let characters = format!(
        "{}/{}/{}/{}",
        stats.correct, stats.incorrect, stats.extra, stats.missed
    );

    vec![
        Segment::plain("chars ", Role::Dim),
        Segment::plain(characters, Role::Text),
        Segment::plain("  ·  time ", Role::Dim),
        Segment::plain(format_duration(stats.elapsed_ms), Role::Text),
        Segment::plain("  ·  ", Role::Dim),
        Segment::plain(view.label.clone(), Role::Text),
    ]
}

fn header(engine: &Engine, view: &View) -> String {
    let row = words_row(view) - 2;

    let left_text = match engine.mode {
        Mode::Time => engine.remaining_seconds(view.now).unwrap_or(0).to_string(),
        _ => {
            let (done, total) = engine.progress();
            format!("{done}/{total}")
        }
    };

    let right_text = if view.live_wpm && engine.started() {
        format!("{} wpm", format_number(stats::live_wpm(engine, view.now)))
    } else {
        view.label.clone()
    };

    let mut out = move_to(row, view.left);
    emit(
        &mut out,
        &[Segment::plain(left_text, Role::Accent)],
        view.theme,
    );

    let right_column = view.left + view.width - right_text.chars().count();
    out.push_str(&move_to(row, right_column));
    emit(
        &mut out,
        &[Segment::plain(right_text, Role::Dim)],
        view.theme,
    );

    out
}

fn footer(view: &View, text: &str) -> String {
    let (rows, _columns) = view.size;
    let column = view.left + view.width.saturating_sub(text.chars().count()) / 2;

    let mut out = move_to(rows - 1, column);
    emit(&mut out, &[Segment::plain(text, Role::Dim)], view.theme);
    out
}

/// Turns one laid-out line into coloured segments, padding the gaps between
/// words so a single move sequence per line is enough.
fn line_segments(line: &[Placed]) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut column = 0;

    for placed in line {
        let gap = " ".repeat(placed.column.saturating_sub(column));
        segments.push(Segment::plain(gap, Role::Untyped));
        segments.extend(word_segments(placed));
        column = placed.column + word_width(&placed.word);
    }

    merge_segments(segments)
}

fn word_segments(placed: &Placed) -> Vec<Segment> {
    let word = &placed.word;
    let typed: Vec<char> = word.typed.chars().collect();
    let target: Vec<char> = word.target.chars().collect();

    let attrs = if word.status == Status::Done && word.typed != word.target {
        Attrs::UNDERLINE
    } else {
        Attrs::NONE
    };

    let mut segments: Vec<Segment> = target
        .iter()
        .enumerate()
        .map(|(index, expected)| {
            let role = match typed.get(index) {
                None => Role::Untyped,
                Some(actual) if actual == expected => Role::Correct,
                Some(_) => Role::Incorrect,
            };

            Segment::new(expected.to_string(), role, attrs)
        })
        .collect();

    segments.extend(
        typed
            .iter()
            .skip(target.len())
            .map(|extra| Segment::new(extra.to_string(), Role::Extra, attrs)),
    );

    segments
}

/// Collapses runs of identically styled characters into one segment.
fn merge_segments(segments: Vec<Segment>) -> Vec<Segment> {
    let mut merged: Vec<Segment> = Vec::new();

    for segment in segments {
        match merged.last_mut() {
            Some(last) if last.role == segment.role && last.attrs == segment.attrs => {
                last.text.push_str(&segment.text);
            }
            _ => merged.push(segment),
        }
    }

    merged
}

fn caret_position(
    lines: &[Vec<Placed>],
    engine: &Engine,
    current_line: usize,
    top: usize,
    first_row: usize,
    view: &View,
) -> Caret {
    if current_line - top >= VISIBLE_LINES {
        return None;
    }

    let word = lines
        .get(current_line)?
        .iter()
        .find(|placed| placed.word.index == engine.index)?;

    let row = first_row + (current_line - top);
    let column = view.left + word.column + engine.buf.chars().count();

    Some((row, column.min(view.left + view.width)))
}

fn line_of(lines: &[Vec<Placed>], index: usize) -> usize {
    lines
        .iter()
        .position(|line| line.iter().any(|placed| placed.word.index == index))
        .unwrap_or(0)
}

fn words_row(view: &View) -> usize {
    let (rows, _columns) = view.size;
    (rows.saturating_sub(VISIBLE_LINES) / 2).max(3)
}

fn word_width(word: &Word) -> usize {
    word.target.chars().count().max(word.typed.chars().count())
}

fn bar_cell(cells: f64, level: usize) -> char {
    let full = cells.trunc();
    let fraction = cells - full;

    if full >= level as f64 {
        '█'
    } else if full == (level - 1) as f64 && fraction > 0.08 {
        BLOCKS[((fraction * 7.0) as usize).min(6)]
    } else {
        ' '
    }
}

fn error_row(buckets: &[Sample]) -> Vec<Segment> {
    let marks: String = buckets
        .iter()
        .map(|bucket| if bucket.errors > 0 { '•' } else { ' ' })
        .collect();

    vec![
        Segment::plain(format!("{} ", pad_left("", LABEL_WIDTH - 1)), Role::Dim),
        Segment::plain(marks, Role::Incorrect),
    ]
}

fn time_axis(samples: &[Sample], buckets: &[Sample]) -> Vec<Segment> {
    let finish = samples.last().map_or(0.0, |sample| sample.at).round();
    let label = format!("{finish}s");
    let gap = buckets
        .len()
        .saturating_sub(label.chars().count() + 1)
        .max(1);

    vec![
        Segment::plain(format!("{} ", pad_left("", LABEL_WIDTH - 1)), Role::Dim),
        Segment::plain(format!("0{}{label}", " ".repeat(gap)), Role::Dim),
    ]
}

/// Spreads the samples across exactly `columns` buckets, so the graph fills its
/// width whether there are more seconds than columns or fewer.
///
/// Each column covers an equal slice of the test. A long test averages several
/// seconds into one column, keeping any error in that slice visible rather than
/// averaging it away; a short test gives one second several columns, which
/// widens its bar rather than inventing readings between them.
fn resample(samples: &[Sample], columns: usize) -> Vec<Sample> {
    if samples.is_empty() {
        return Vec::new();
    }

    (0..columns)
        .map(|column| {
            let start = column * samples.len() / columns;
            let end = (((column + 1) * samples.len()) / columns)
                .max(start + 1)
                .min(samples.len());

            let slice = &samples[start..end];

            Sample {
                at: slice.last().map_or(0.0, |sample| sample.at),
                raw: slice.iter().map(|sample| sample.raw).sum::<f64>() / slice.len() as f64,
                errors: slice.iter().map(|sample| sample.errors).sum(),
            }
        })
        .collect()
}

/// Rounds the top of the scale up to something readable.
fn ceiling_for(buckets: &[Sample]) -> f64 {
    let peak = buckets.iter().map(|bucket| bucket.raw).fold(1.0, f64::max);

    let step: f64 = if peak > 200.0 { 50.0 } else { 20.0 };

    step.max((peak / step).ceil() * step)
}

fn emit(out: &mut String, segments: &[Segment], theme: &Theme) {
    for segment in segments {
        theme.paint_into(out, segment.role, &segment.text, segment.attrs);
    }
}

fn display_key(key: char) -> String {
    if key == ' ' {
        "space".to_string()
    } else {
        key.to_string()
    }
}

fn pad_left(text: &str, width: usize) -> String {
    format!("{text:>width$}")
}

fn pad_right(text: &str, width: usize) -> String {
    format!("{text:<width$}")
}

fn format_number(value: f64) -> String {
    format!("{}", value.round() as i64)
}

fn percent_or_dash(value: Option<f64>) -> String {
    value.map_or_else(
        || "--".to_string(),
        |value| format!("{}%", format_number(value)),
    )
}

fn format_duration(ms: i64) -> String {
    if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        let seconds = ms / 1000;
        format!("{}m{}s", seconds / 60, seconds % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Backtrack, Key};
    use crate::theme::ColourDepth;

    fn theme() -> Theme {
        Theme::build_with("default", ColourDepth::Extended)
    }

    fn view(theme: &Theme) -> View<'_> {
        View {
            theme,
            size: (24, 80),
            width: 40,
            left: 5,
            now: 0,
            live_wpm: true,
            label: "30s · english".to_string(),
            best: None,
            comparison: None,
            trouble: Vec::new(),
        }
    }

    fn engine(list: &[&str]) -> Engine {
        Engine::new(
            Mode::Time,
            30,
            list.iter().map(|word| word.to_string()).collect(),
            Backtrack::Strict,
        )
    }

    fn pending<'a>(list: &'a [&'a str]) -> Vec<Word<'a>> {
        list.iter()
            .enumerate()
            .map(|(index, target)| Word {
                index,
                target,
                typed: "",
                status: Status::Pending,
            })
            .collect()
    }

    /// Strips the escape sequences, leaving what a person sees.
    fn visible(frame: &str) -> String {
        let mut out = String::new();
        let mut characters = frame.chars().peekable();

        while let Some(character) = characters.next() {
            if character != '\x1b' {
                out.push(character);
                continue;
            }

            // Skip "[", the parameter bytes, and the final letter.
            for escaped in characters.by_ref() {
                if escaped.is_ascii_alphabetic() {
                    break;
                }
            }
        }

        out
    }

    fn text_of(segments: &[Segment]) -> String {
        segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect()
    }

    fn targets<'a>(lines: &[Vec<Placed<'a>>]) -> Vec<Vec<&'a str>> {
        lines
            .iter()
            .map(|line| line.iter().map(|placed| placed.word.target).collect())
            .collect()
    }

    fn columns(line: &[Placed]) -> Vec<usize> {
        line.iter().map(|placed| placed.column).collect()
    }

    fn commit(engine: &mut Engine, word: &str) {
        for character in word.chars() {
            engine.key(Key::Char(character), 0);
        }

        engine.key(Key::Space, 0);
    }

    // layout

    #[test]
    fn breaks_lines_at_the_column_width() {
        let words = ["aaaa", "bbbb", "cccc", "dddd"];
        let lines = layout(&pending(&words), 10);

        assert_eq!(targets(&lines), [["aaaa", "bbbb"], ["cccc", "dddd"]]);
    }

    #[test]
    fn positions_words_with_a_single_space_between_them() {
        let words = ["one", "two"];
        let lines = layout(&pending(&words), 40);

        assert_eq!(columns(&lines[0]), [0, 4]);
    }

    #[test]
    fn a_word_longer_than_the_width_gets_a_line_to_itself_rather_than_vanishing() {
        let words = ["a", "supercalifragilistic", "b"];
        let lines = layout(&pending(&words), 10);

        assert_eq!(
            targets(&lines),
            [vec!["a"], vec!["supercalifragilistic"], vec!["b"]]
        );
    }

    #[test]
    fn overtyped_words_push_their_neighbours_along() {
        let words = [
            Word {
                index: 0,
                target: "ab",
                typed: "abcdefgh",
                status: Status::Done,
            },
            Word {
                index: 1,
                target: "cd",
                typed: "",
                status: Status::Pending,
            },
        ];

        let lines = layout(&words, 40);

        assert_eq!(columns(&lines[0]), [0, 9]);
    }

    #[test]
    fn an_empty_word_list_lays_out_to_nothing() {
        assert!(layout(&[], 40).is_empty());
    }

    // test_frame

    #[test]
    fn shows_the_words_and_puts_the_caret_where_the_typist_is() {
        let theme = theme();
        let mut engine = engine(&["the", "quick", "brown"]);
        engine.key(Key::Char('t'), 0);

        let (frame, caret) = test_frame(&engine, &view(&theme));
        let output = visible(&frame);

        assert!(output.contains("the quick brown"), "{output}");
        assert!(output.contains("tab restart · esc quit"));
        // One character in, so the caret sits one column past the word's start.
        assert_eq!(caret.map(|(_row, column)| column), Some(6));
    }

    #[test]
    fn counts_down_in_timed_mode_and_counts_words_otherwise() {
        let theme = theme();
        let mut view = view(&theme);
        view.live_wpm = false;

        let timed = engine(&["the", "quick"]);
        let counted = Engine::new(
            Mode::Words,
            2,
            vec!["the".to_string(), "quick".to_string()],
            Backtrack::Strict,
        );

        assert!(visible(&test_frame(&timed, &view).0).contains("30"));
        assert!(visible(&test_frame(&counted, &view).0).contains("0/2"));
    }

    #[test]
    fn scrolls_so_the_active_line_is_never_the_last_one_on_screen() {
        let theme = theme();
        let words: Vec<&str> = vec!["acorn"; 60];
        let mut engine = engine(&words);

        // Skip far enough ahead that the active word is well past the first line.
        for _ in 0..20 {
            commit(&mut engine, "acorn");
        }

        let view = view(&theme);
        let (_frame, caret) = test_frame(&engine, &view);
        let (rows, _columns) = view.size;

        assert!(caret.unwrap().0 < rows - 1);
    }

    #[test]
    fn the_frame_changes_as_characters_are_typed() {
        let theme = theme();
        let view = view(&theme);
        let mut engine = engine(&["the", "quick", "brown"]);

        let before = visible(&test_frame(&engine, &view).0);
        engine.key(Key::Char('x'), 0);
        let after = visible(&test_frame(&engine, &view).0);

        assert_ne!(before, after);
    }

    #[test]
    fn a_wrong_character_is_painted_differently_from_a_right_one() {
        let theme = theme();
        let view = view(&theme);
        let mut engine = engine(&["the"]);
        engine.key(Key::Char('x'), 0);

        let frame = test_frame(&engine, &view).0;

        assert!(frame.contains(theme.code(Role::Incorrect)));
    }

    // results_frame

    #[test]
    fn reports_the_headline_figures() {
        let theme = theme();
        let mut view = view(&theme);
        view.best = Some("new personal best".to_string());

        let mut engine = Engine::new(Mode::Words, 1, vec!["acorn".to_string()], Backtrack::Strict);
        engine.key(Key::Char('a'), 0);
        engine.finish(60_000);

        let stats = Stats::compute(&engine, 60_000);
        let (frame, caret) = results_frame(&stats, &view);
        let output = visible(&frame);

        assert!(output.contains("wpm"));
        assert!(output.contains("acc"));
        assert!(output.contains("consistency"));
        assert!(output.contains("chars"));
        assert!(output.contains("new personal best"));
        assert!(output.contains("tab new test · r repeat · esc quit"));
        assert_eq!(caret, None);
    }

    #[test]
    fn consistency_shows_a_dash_when_there_was_not_enough_data() {
        let theme = theme();
        let view = view(&theme);

        let mut engine = Engine::new(Mode::Words, 1, vec!["acorn".to_string()], Backtrack::Strict);
        engine.key(Key::Char('a'), 0);
        engine.finish(500);

        let stats = Stats::compute(&engine, 500);

        assert!(visible(&results_frame(&stats, &view).0).contains("--"));
    }

    #[test]
    fn trouble_keys_are_listed_when_there_are_any() {
        let theme = theme();
        let mut view = view(&theme);
        view.trouble = vec![TroubleKey {
            key: 'e',
            attempts: 40,
            errors: 4,
            accuracy: 90.0,
        }];

        let stats = Stats::default();
        let output = visible(&results_frame(&stats, &view).0);

        assert!(output.contains("trouble"));
        assert!(output.contains("90%"));
    }

    // too_small_frame

    #[test]
    fn the_too_small_message_names_the_size_it_needs() {
        let theme = theme();
        let mut view = view(&theme);
        view.size = (5, 20);

        let output = visible(&too_small_frame(&view).0);

        assert!(output.contains("20x5"), "{output}");
        assert!(output.contains("40x8"));
    }

    // chart

    #[test]
    fn the_chart_is_empty_without_samples() {
        assert!(chart(&[], 40, 8).is_empty());
    }

    #[test]
    fn draws_the_speed_and_marks_the_seconds_with_errors() {
        let samples = [
            Sample {
                at: 1.0,
                raw: 60.0,
                errors: 0,
            },
            Sample {
                at: 2.0,
                raw: 30.0,
                errors: 2,
            },
            Sample {
                at: 3.0,
                raw: 90.0,
                errors: 0,
            },
        ];

        let rows: Vec<String> = chart(&samples, 40, 8)
            .iter()
            .map(|row| text_of(row))
            .collect();

        assert!(rows.iter().any(|row| row.contains('█')));
        assert!(rows.iter().any(|row| row.contains('•')));
        assert!(rows.iter().any(|row| row.contains("3s")));
    }

    fn flat_samples(seconds: u32) -> Vec<Sample> {
        (1..=seconds)
            .map(|second| Sample {
                at: f64::from(second),
                raw: 50.0,
                errors: 0,
            })
            .collect()
    }

    /// The width of a chart row, counting the label gutter.
    fn row_width(samples: &[Sample], width: usize) -> usize {
        text_of(&chart(samples, width, 8)[0]).chars().count()
    }

    #[test]
    fn a_long_test_is_averaged_down_to_the_available_width() {
        assert_eq!(row_width(&flat_samples(120), 40), 40);
    }

    #[test]
    fn a_short_test_is_stretched_out_to_the_available_width() {
        // Fifteen seconds used to leave two thirds of the column empty.
        assert_eq!(row_width(&flat_samples(15), 72), 72);
    }

    #[test]
    fn every_test_length_fills_the_width_on_every_row() {
        for seconds in [1u32, 2, 3, 5, 7, 15, 29, 30, 31, 60, 97, 120, 300, 1000] {
            for (index, row) in chart(&flat_samples(seconds), 72, 8).iter().enumerate() {
                let width = text_of(row).chars().count();

                assert_eq!(width, 72, "{seconds}s: row {index} is {width} wide, not 72");
            }
        }
    }

    #[test]
    fn stretching_repeats_a_second_rather_than_inventing_readings() {
        let samples = [
            Sample {
                at: 1.0,
                raw: 20.0,
                errors: 0,
            },
            Sample {
                at: 2.0,
                raw: 80.0,
                errors: 0,
            },
        ];

        let speeds: Vec<f64> = resample(&samples, 6)
            .iter()
            .map(|bucket| bucket.raw)
            .collect();

        // Each second owns three columns; nothing appears between 20 and 80.
        assert_eq!(speeds, [20.0, 20.0, 20.0, 80.0, 80.0, 80.0]);
    }

    #[test]
    fn averaging_down_keeps_an_error_in_the_slice_visible() {
        let samples = [
            Sample {
                at: 1.0,
                raw: 40.0,
                errors: 0,
            },
            Sample {
                at: 2.0,
                raw: 60.0,
                errors: 3,
            },
        ];

        let buckets = resample(&samples, 1);

        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].raw, 50.0);
        assert_eq!(buckets[0].errors, 3);
    }

    #[test]
    fn resampling_nothing_gives_nothing() {
        assert!(resample(&[], 10).is_empty());
    }
}
