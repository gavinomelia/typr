//! Colour roles and the SGR sequences that paint them.
//!
//! Each theme maps a role to a `(basic, extended)` colour code so the same theme
//! works on an eight-colour terminal and a 256-colour one. Roles, not colours,
//! are used at the call site, which keeps the renderer free of literals.

pub const RESET: &str = "\x1b[0m";

/// What a piece of text *is*, rather than what colour it should be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Text,
    Dim,
    Untyped,
    Correct,
    Incorrect,
    Extra,
    Accent,
}

impl Role {
    fn index(self) -> usize {
        match self {
            Role::Text => 0,
            Role::Dim => 1,
            Role::Untyped => 2,
            Role::Correct => 3,
            Role::Incorrect => 4,
            Role::Extra => 5,
            Role::Accent => 6,
        }
    }
}

/// Extra styling layered on top of a role's colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Attrs {
    pub bold: bool,
    pub underline: bool,
}

impl Attrs {
    pub const NONE: Attrs = Attrs {
        bold: false,
        underline: false,
    };
    pub const BOLD: Attrs = Attrs {
        bold: true,
        underline: false,
    };
    pub const UNDERLINE: Attrs = Attrs {
        bold: false,
        underline: true,
    };

    fn write(self, out: &mut String) {
        if self.bold {
            out.push_str("\x1b[1m");
        }

        if self.underline {
            out.push_str("\x1b[4m");
        }
    }
}

// Roles in `Role::index` order: text, dim, untyped, correct, incorrect, extra,
// accent. Kept sorted by name so `names` needs no work of its own.
type Palette = [(u16, u16); 7];

const PALETTES: &[(&str, Palette)] = &[
    (
        "default",
        [
            (37, 252),
            (90, 240),
            (90, 245),
            (97, 231),
            (91, 203),
            (31, 131),
            (93, 221),
        ],
    ),
    (
        "matrix",
        [
            (32, 157),
            (32, 22),
            (32, 65),
            (92, 46),
            (91, 196),
            (31, 88),
            (92, 118),
        ],
    ),
    (
        "mono",
        [
            (37, 250),
            (90, 238),
            (90, 243),
            (97, 255),
            (90, 240),
            (90, 236),
            (97, 255),
        ],
    ),
    (
        "ocean",
        [
            (36, 152),
            (34, 24),
            (34, 67),
            (96, 195),
            (91, 210),
            (31, 95),
            (96, 81),
        ],
    ),
];

/// How many colours the terminal can show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColourDepth {
    /// The original eight ANSI colours.
    Basic,
    /// The 256-colour palette.
    Extended,
}

pub struct Theme {
    sequences: [String; 7],
}

impl Theme {
    /// Names of the available themes.
    pub fn names() -> Vec<&'static str> {
        PALETTES.iter().map(|(name, _palette)| *name).collect()
    }

    /// Whether a theme exists.
    pub fn exists(name: &str) -> bool {
        PALETTES.iter().any(|(known, _palette)| *known == name)
    }

    /// Builds a theme's role-to-escape-sequence lookup.
    ///
    /// Colour depth is detected once here rather than on every painted
    /// character.
    pub fn build(name: &str) -> Theme {
        Theme::build_with(name, detect_depth())
    }

    /// Builds a theme at a known colour depth, so tests do not depend on the
    /// environment they run in.
    pub fn build_with(name: &str, depth: ColourDepth) -> Theme {
        let palette = PALETTES
            .iter()
            .find(|(known, _palette)| *known == name)
            .map_or(&PALETTES[0].1, |(_name, palette)| palette);

        let sequences = std::array::from_fn(|index| {
            let (basic, wide) = palette[index];

            match depth {
                ColourDepth::Extended => format!("\x1b[38;5;{wide}m"),
                ColourDepth::Basic => format!("\x1b[{basic}m"),
            }
        });

        Theme { sequences }
    }

    /// The escape sequence that switches to a role's colour.
    pub fn code(&self, role: Role) -> &str {
        &self.sequences[role.index()]
    }

    /// Appends `text` painted in a role's colour plus any extra attributes.
    pub fn paint_into(&self, out: &mut String, role: Role, text: &str, attrs: Attrs) {
        out.push_str(self.code(role));
        attrs.write(out);
        out.push_str(text);
        out.push_str(RESET);
    }
}

fn detect_depth() -> ColourDepth {
    let colorterm = std::env::var("COLORTERM").unwrap_or_default();
    let term = std::env::var("TERM").unwrap_or_default();

    if !colorterm.is_empty() || term.contains("256") || term.contains("direct") {
        ColourDepth::Extended
    } else {
        ColourDepth::Basic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_named_theme_exists() {
        assert!(Theme::names().iter().all(|name| Theme::exists(name)));
    }

    #[test]
    fn names_are_sorted_and_complete() {
        assert_eq!(Theme::names(), ["default", "matrix", "mono", "ocean"]);
    }

    #[test]
    fn unknown_themes_do_not_exist() {
        assert!(!Theme::exists("neon"));
    }

    #[test]
    fn an_unknown_name_falls_back_to_the_default_palette() {
        let fallback = Theme::build_with("neon", ColourDepth::Extended);
        let default = Theme::build_with("default", ColourDepth::Extended);

        assert_eq!(fallback.code(Role::Accent), default.code(Role::Accent));
    }

    #[test]
    fn colour_depth_picks_the_sequence_family() {
        assert_eq!(
            Theme::build_with("default", ColourDepth::Extended).code(Role::Text),
            "\x1b[38;5;252m"
        );
        assert_eq!(
            Theme::build_with("default", ColourDepth::Basic).code(Role::Text),
            "\x1b[37m"
        );
    }

    #[test]
    fn painting_wraps_text_in_colour_and_resets_after() {
        let theme = Theme::build_with("default", ColourDepth::Basic);
        let mut out = String::new();
        theme.paint_into(&mut out, Role::Correct, "ok", Attrs::NONE);

        assert_eq!(out, "\x1b[97mok\x1b[0m");
    }

    #[test]
    fn attributes_are_layered_over_the_colour() {
        let theme = Theme::build_with("default", ColourDepth::Basic);
        let mut out = String::new();
        theme.paint_into(
            &mut out,
            Role::Text,
            "hi",
            Attrs {
                bold: true,
                underline: true,
            },
        );

        assert_eq!(out, "\x1b[37m\x1b[1m\x1b[4mhi\x1b[0m");
    }

    #[test]
    fn every_theme_defines_every_role() {
        let roles = [
            Role::Text,
            Role::Dim,
            Role::Untyped,
            Role::Correct,
            Role::Incorrect,
            Role::Extra,
            Role::Accent,
        ];

        for name in Theme::names() {
            let theme = Theme::build_with(name, ColourDepth::Extended);

            for role in roles {
                assert!(
                    theme.code(role).starts_with("\x1b["),
                    "{name} has no sequence for {role:?}"
                );
            }
        }
    }
}
