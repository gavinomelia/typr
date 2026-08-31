//! The event loop.
//!
//! A reader thread blocks on standard input and forwards each character down a
//! channel, which leaves the loop free to wake on its own timer to advance the
//! clock. `recv_timeout` is the whole trick: one call either hands over the next
//! keystroke or tells us the frame is due.

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use crate::engine::{Backtrack, Engine, Key, Mode};
use crate::history::{Config, History};
use crate::render::{self, View};
use crate::rng::Rng;
use crate::stats::Stats;
use crate::summary::{self, TroubleKey, TroubleOptions};
use crate::terminal::{self, AltScreen, Input, RawMode};
use crate::theme::Theme;
use crate::words;

const LOOKAHEAD: usize = 40;
const INITIAL_WORDS: usize = 60;
const RUNNING_FRAME: Duration = Duration::from_millis(50);
const IDLE_FRAME: Duration = Duration::from_millis(200);
const SIZE_INTERVAL_MS: i64 = 500;
const MIN_COLUMNS: usize = 40;
const MIN_ROWS: usize = 8;

/// How long to wait for the rest of an escape sequence before deciding the
/// typist really did just press escape.
const ESCAPE_GRACE: Duration = Duration::from_millis(30);

/// Everything the CLI decided, handed to the loop as one value.
pub struct Options {
    pub config: Config,
    pub theme: String,
    pub width: usize,
    pub live_wpm: bool,
    pub backtrack: Backtrack,
    pub seed: Option<u64>,
}

/// What a keystroke means, once the escape sequences have been resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Quit,
    Tab,
    Enter,
    Typing(Key),
    Ignore,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Screen {
    Test,
    Results,
}

/// Whether the loop should keep going.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flow {
    Continue,
    Quit,
}

pub struct App<'a> {
    options: &'a Options,
    engine: Engine,
    theme: Theme,
    rng: Rng,
    history: History,
    clock: Instant,
    size: (usize, usize),
    size_checked_at: i64,
    screen: Screen,
    stats: Option<Stats>,
    best: Option<String>,
    comparison: Option<String>,
    trouble: Vec<TroubleKey>,
    last_frame: Option<String>,
}

/// Runs tests until the typist quits.
///
/// Returns the last completed result so the caller can echo it into the scroll
/// back after the alternate screen is torn down.
pub fn run(options: &Options) -> Option<Stats> {
    let mut rng = match options.seed {
        Some(seed) => Rng::seeded(seed),
        None => Rng::from_entropy(),
    };

    let engine = new_engine(options, &mut rng);

    let mut app = App {
        options,
        engine,
        theme: Theme::build(&options.theme),
        rng,
        history: History::open(),
        clock: Instant::now(),
        size: terminal::size(),
        size_checked_at: 0,
        screen: Screen::Test,
        stats: None,
        best: None,
        comparison: None,
        trouble: Vec::new(),
        last_frame: None,
    };

    let input = terminal::start_reader();
    app.event_loop(&input);
    app.stats
}

impl App<'_> {
    fn event_loop(&mut self, input: &Receiver<Input>) {
        loop {
            self.refresh_size();
            self.advance();
            self.draw();

            match input.recv_timeout(self.frame_interval()) {
                Ok(Input::Key(character)) => {
                    if self.handle(classify(character, input)) == Flow::Quit {
                        return;
                    }
                }
                Ok(Input::Closed) | Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }

    fn now(&self) -> i64 {
        self.clock.elapsed().as_millis() as i64
    }

    /// Advances the clock, tops up the word supply, and moves to the results
    /// screen the moment the engine says the test is over.
    fn advance(&mut self) {
        if self.screen == Screen::Results {
            return;
        }

        let now = self.now();
        self.engine.tick(now);

        if self.engine.needs_words(LOOKAHEAD) {
            let more = self.generate(INITIAL_WORDS);
            self.engine.extend(more);
        }

        if self.engine.finished() {
            self.show_results(now);
        }
    }

    fn show_results(&mut self, now: i64) {
        let stats = Stats::compute(&self.engine, now);
        let (best, comparison) = self.record(&stats);

        self.trouble = summary::trouble_keys(
            &stats.keys,
            TroubleOptions {
                min_attempts: 3,
                keys: 4,
                ..TroubleOptions::default()
            },
        );

        self.screen = Screen::Results;
        self.stats = Some(stats);
        self.best = best;
        self.comparison = comparison;
        self.last_frame = None;
    }

    /// A run abandoned after a couple of keystrokes is noise; recording it would
    /// drag every average down for no reason.
    fn record(&mut self, stats: &Stats) -> (Option<String>, Option<String>) {
        if stats.elapsed_ms < 1_000 || stats.correct == 0 {
            return (None, None);
        }

        let config = self.options.config.key();
        let previous = summary::best_for(&self.history.load(), &config).cloned();

        // An unwritable disk should not interrupt someone mid-practice.
        let _ = self.history.append(stats, &self.options.config);

        let results = self.history.load();

        (
            best_message(stats, previous.as_ref()),
            comparison(&results, &config),
        )
    }

    fn handle(&mut self, action: Action) -> Flow {
        match (self.screen, action) {
            (_, Action::Quit) => Flow::Quit,
            // On the results screen the letters are commands, not typing.
            (Screen::Results, Action::Typing(Key::Char('q'))) => Flow::Quit,
            (Screen::Results, Action::Typing(Key::Char('r'))) => self.restart(Restart::Repeat),
            (Screen::Results, Action::Tab | Action::Enter) => self.restart(Restart::New),
            (Screen::Test, Action::Tab) => self.restart(Restart::New),
            (Screen::Test, Action::Typing(key)) => {
                let now = self.now();
                self.engine.key(key, now);
                Flow::Continue
            }
            _ => Flow::Continue,
        }
    }

    fn restart(&mut self, restart: Restart) -> Flow {
        self.engine = match restart {
            Restart::New => new_engine(self.options, &mut self.rng),
            Restart::Repeat => build_engine(self.options, self.engine.words.clone()),
        };

        self.screen = Screen::Test;
        self.stats = None;
        self.best = None;
        self.comparison = None;
        self.trouble = Vec::new();
        self.last_frame = None;

        Flow::Continue
    }

    fn draw(&mut self) {
        let (frame, caret) = self.frame();

        if self.last_frame.as_deref() == Some(frame.as_str()) {
            return;
        }

        terminal::paint(&frame, caret);
        self.last_frame = Some(frame);
    }

    fn frame(&self) -> (String, render::Caret) {
        let view = self.view();
        let (rows, columns) = self.size;

        if columns < MIN_COLUMNS || rows < MIN_ROWS {
            render::too_small_frame(&view)
        } else if self.screen == Screen::Results {
            // The results screen only exists once stats have been computed.
            match &self.stats {
                Some(stats) => render::results_frame(stats, &view),
                None => render::test_frame(&self.engine, &view),
            }
        } else {
            render::test_frame(&self.engine, &view)
        }
    }

    fn view(&self) -> View<'_> {
        let (rows, columns) = self.size;
        let width = self.options.width.min(columns.saturating_sub(4));
        let left = (columns.saturating_sub(width) / 2 + 1).max(1);

        View {
            theme: &self.theme,
            size: (rows, columns),
            width,
            left,
            now: self.now(),
            live_wpm: self.options.live_wpm,
            label: label(self.options),
            best: self.best.clone(),
            comparison: self.comparison.clone(),
            trouble: self.trouble.clone(),
        }
    }

    /// Size is polled rather than pushed. It could be a SIGWINCH handler, but a
    /// cheap ioctl twice a second is simpler than signal-safe bookkeeping.
    fn refresh_size(&mut self) {
        let now = self.now();

        if now - self.size_checked_at < SIZE_INTERVAL_MS {
            return;
        }

        let size = terminal::size();

        if size != self.size {
            self.last_frame = None;
        }

        self.size = size;
        self.size_checked_at = now;
    }

    fn frame_interval(&self) -> Duration {
        if self.screen == Screen::Test && self.engine.started() {
            RUNNING_FRAME
        } else {
            IDLE_FRAME
        }
    }

    fn generate(&mut self, count: usize) -> Vec<String> {
        words::generate(
            &self.options.config.list,
            count,
            self.options.config.decoration,
            &mut self.rng,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Restart {
    New,
    Repeat,
}

/// Puts the terminal into raw mode on the alternate screen, runs the test, and
/// puts everything back whatever happens — including on a panic, since both
/// guards restore on drop.
pub fn play(options: &Options) -> Result<Option<Stats>, String> {
    if !terminal::is_tty() {
        return Err("typr needs an interactive terminal".to_string());
    }

    let _raw = RawMode::enable().map_err(|_| "typr needs an interactive terminal".to_string())?;
    let _screen = AltScreen::enter();

    Ok(run(options))
}

fn new_engine(options: &Options, rng: &mut Rng) -> Engine {
    let words = match options.config.mode {
        Mode::Quote => words::quote_words(rng),
        Mode::Time => generate_with(options, INITIAL_WORDS, rng),
        Mode::Words => generate_with(options, options.config.limit.max(1) as usize, rng),
    };

    build_engine(options, words)
}

fn generate_with(options: &Options, count: usize, rng: &mut Rng) -> Vec<String> {
    words::generate(&options.config.list, count, options.config.decoration, rng)
}

fn build_engine(options: &Options, words: Vec<String>) -> Engine {
    Engine::new(
        options.config.mode,
        options.config.limit,
        words,
        options.backtrack,
    )
}

fn label(options: &Options) -> String {
    let config = &options.config;

    let mut parts = vec![match config.mode {
        Mode::Time => format!("{}s", config.limit),
        Mode::Words => format!("{} words", config.limit),
        Mode::Quote => "quote".to_string(),
    }];

    if config.mode != Mode::Quote {
        parts.push(config.list.clone());
    }

    if config.decoration.punctuation {
        parts.push("punctuation".to_string());
    }

    if config.decoration.numbers {
        parts.push("numbers".to_string());
    }

    parts.join(" · ")
}

fn best_message(stats: &Stats, previous: Option<&crate::history::Record>) -> Option<String> {
    let Some(previous) = previous else {
        return Some("first result for this test — the bar is set".to_string());
    };

    if stats.wpm > previous.wpm {
        Some(format!(
            "new personal best · +{} wpm on {}",
            (stats.wpm - previous.wpm).round(),
            previous.wpm.round()
        ))
    } else {
        None
    }
}

fn comparison(results: &[crate::history::Record], config: &str) -> Option<String> {
    let best = summary::best_for(results, config)?;
    let average = summary::average_for(results, config)?;
    let count = summary::count_for(results, config);

    Some(format!(
        "pb {} · avg {} · {count} {}",
        best.wpm.round(),
        average.round(),
        if count == 1 { "test" } else { "tests" }
    ))
}

/// Maps a character to an action.
///
/// Escape sequences arrive one character at a time, so a lone escape is told
/// apart from an arrow key by waiting briefly for a follow-up that never comes.
fn classify(character: char, input: &Receiver<Input>) -> Action {
    match character {
        '\x1b' => resolve_escape(input),
        '\t' => Action::Tab,
        '\r' | '\n' => Action::Enter,
        ' ' => Action::Typing(Key::Space),
        '\x7f' => Action::Typing(Key::Backspace),
        '\x08' | '\x17' => Action::Typing(Key::BackspaceWord),
        '\x03' | '\x04' => Action::Quit,
        character if (character as u32) < 32 => Action::Ignore,
        character => Action::Typing(Key::Char(character)),
    }
}

fn resolve_escape(input: &Receiver<Input>) -> Action {
    match input.recv_timeout(ESCAPE_GRACE) {
        Ok(Input::Key('[' | 'O')) => drain_sequence(input),
        Ok(Input::Key(_)) => Action::Quit,
        _ => Action::Quit,
    }
}

/// Swallows the rest of a control sequence up to its final byte.
fn drain_sequence(input: &Receiver<Input>) -> Action {
    loop {
        match input.recv_timeout(ESCAPE_GRACE) {
            Ok(Input::Key(character)) if ('@'..='~').contains(&character) => return Action::Ignore,
            Ok(Input::Key(_)) => continue,
            _ => return Action::Ignore,
        }
    }
}
