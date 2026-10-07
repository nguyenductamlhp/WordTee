//! The look: colours by role, the type scale, and the egui style built from
//! both.
//!
//! Every colour on screen comes from a [`Palette`], and every palette entry
//! has one job. That is the rule the old palette broke: its teal meant
//! "known", "correct", "a settings heading" and "switched on" at once, so a
//! chip could not be read without its label. Here the knowledge-state colours
//! are only ever states; the purple of an action is also the purple of
//! "learning" on purpose, because pressing "Learn it" is what turns a square
//! purple.

use eframe::egui::{self, Color32, FontFamily, RichText};

/// One theme's colours, by role.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    // --- surfaces ---
    /// Behind everything.
    pub page: Color32,
    /// Cards, sheets, the tab bar.
    pub surface: Color32,
    /// A well inside a surface: secondary buttons, segmented tracks, tiles.
    pub sunken: Color32,
    /// Hairlines between rows, card borders.
    pub line: Color32,

    // --- text ---
    pub ink: Color32,
    /// Secondary text.
    pub ink2: Color32,
    /// Tertiary text: captions on the page or a card, never on `sunken`.
    pub ink3: Color32,

    // --- the one action colour ---
    /// Filled buttons, the active tab, the selection.
    pub primary: Color32,
    /// Text on [`Self::primary`].
    pub on_primary: Color32,
    /// The action colour as text and links.
    pub primary_ink: Color32,
    /// A pale wash of it, behind icons and chips.
    pub primary_soft: Color32,

    // --- knowledge states (spec 2.3) and right answers ---
    pub known: Color32,
    /// Text on [`Self::known`].
    pub on_known: Color32,
    pub known_ink: Color32,
    pub known_soft: Color32,
    /// The border of a right answer.
    pub known_line: Color32,
    /// Inferred, not confirmed: drawn hatched with [`Self::assumed_hatch`].
    pub assumed: Color32,
    pub assumed_hatch: Color32,
    pub learning: Color32,
    pub unexplored: Color32,
    /// The dot on a "New" chip, which would vanish in `unexplored` itself.
    pub unexplored_dot: Color32,

    // --- wrong answers ---
    pub wrong: Color32,
    /// Text on [`Self::wrong`].
    pub on_wrong: Color32,
    pub wrong_ink: Color32,
    pub wrong_soft: Color32,
    pub wrong_line: Color32,

    // --- the streak, in the colour of the ball on the logo's tee ---
    pub streak: Color32,
    pub streak_ink: Color32,
    pub streak_soft: Color32,

    /// Cautionary text: the review backlog, the guessing warning.
    pub warn: Color32,
    /// An off switch's track.
    pub track: Color32,
    /// Card shadow; transparent where a shadow would not show.
    pub shadow: Color32,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

pub const LIGHT: Palette = Palette {
    page: rgb(0xF6F4FA),
    surface: rgb(0xFFFFFF),
    sunken: rgb(0xEFEBF5),
    line: rgb(0xE4DEED),
    ink: rgb(0x1E1430),
    ink2: rgb(0x5E5470),
    ink3: rgb(0x736983),
    primary: rgb(0xA722EC),
    on_primary: rgb(0xFFFFFF),
    primary_ink: rgb(0x8317C4),
    primary_soft: rgb(0xF3E6FD),
    known: rgb(0x30BFD0),
    on_known: rgb(0x06363D),
    known_ink: rgb(0x0B6C78),
    known_soft: rgb(0xDCF4F7),
    known_line: rgb(0x30BFD0),
    assumed: rgb(0x9EDDE6),
    assumed_hatch: rgb(0xC6EDF2),
    learning: rgb(0xA722EC),
    unexplored: rgb(0xE2DCEB),
    unexplored_dot: rgb(0xA99FB8),
    wrong: rgb(0xC01E4B),
    on_wrong: rgb(0xFFFFFF),
    wrong_ink: rgb(0xC01E4B),
    wrong_soft: rgb(0xFDE7EC),
    wrong_line: rgb(0xE58AA0),
    streak: rgb(0xF5B341),
    streak_ink: rgb(0x8A5300),
    streak_soft: rgb(0xFFF1D6),
    warn: rgb(0x9A5B00),
    track: rgb(0xD9D2E3),
    shadow: Color32::from_black_alpha(14),
};

/// The same roles on near-black. The fills stay; the text colours lighten,
/// since a purple that reads on white washes out on black and the reverse.
pub const DARK: Palette = Palette {
    page: rgb(0x120D19),
    surface: rgb(0x1D1626),
    sunken: rgb(0x2A2136),
    line: rgb(0x352A43),
    ink: rgb(0xF1EBF7),
    ink2: rgb(0xBDB2CA),
    ink3: rgb(0xA397B0),
    primary: rgb(0xA722EC),
    on_primary: rgb(0xFFFFFF),
    primary_ink: rgb(0xCF94FF),
    primary_soft: rgb(0x3A1D52),
    known: rgb(0x30BFD0),
    on_known: rgb(0x06363D),
    known_ink: rgb(0x6FDCEA),
    known_soft: rgb(0x0F3A40),
    known_line: rgb(0x2C8C99),
    assumed: rgb(0x2C8490),
    assumed_hatch: rgb(0x23707A),
    learning: rgb(0xA722EC),
    unexplored: rgb(0x332943),
    unexplored_dot: rgb(0x6E6280),
    wrong: rgb(0xFF8DA1),
    on_wrong: rgb(0x3A0A18),
    wrong_ink: rgb(0xFF8DA1),
    wrong_soft: rgb(0x45172A),
    wrong_line: rgb(0xA23A57),
    streak: rgb(0xF5B341),
    streak_ink: rgb(0xFFCF74),
    streak_soft: rgb(0x3D2E12),
    warn: rgb(0xF5B341),
    track: rgb(0x4A3E5A),
    shadow: Color32::TRANSPARENT,
};

/// The palette for the theme `ui` is drawn in.
pub fn palette(ui: &egui::Ui) -> Palette {
    if ui.visuals().dark_mode { DARK } else { LIGHT }
}

/// Mixes `a` towards `b` by `t`, 0 to 1.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

// -------------------------------------------------------------------------
// type
// -------------------------------------------------------------------------

/// The family that draws in Noto Sans SemiBold.
///
/// egui has no font weights, only families, and `RichText::strong` only
/// changes the colour — so before this family existed, nothing in the app
/// was ever bold, and every heading was told apart by size alone.
pub const SEMIBOLD: &str = "semibold";

pub fn semibold() -> FontFamily {
    FontFamily::Name(SEMIBOLD.into())
}

/// The six sizes, and nothing between them.
pub mod size {
    /// The headword on a card.
    pub const DISPLAY: f32 = 34.0;
    /// A screen's title.
    pub const TITLE: f32 = 22.0;
    /// A card's or a section's heading.
    pub const HEADING: f32 = 17.0;
    /// Meanings, options, row labels.
    pub const BODY: f32 = 16.0;
    /// Short labels above or beside a control.
    pub const LABEL: f32 = 14.0;
    /// Secondary lines.
    pub const CAPTION: f32 = 13.0;
    /// Chips and the tab bar, the one place smaller than a caption.
    pub const CHIP: f32 = 12.5;
}

pub fn display(text: impl Into<String>) -> RichText {
    RichText::new(text).size(size::DISPLAY).family(semibold())
}

pub fn title(text: impl Into<String>) -> RichText {
    RichText::new(text).size(size::TITLE).family(semibold())
}

pub fn heading(text: impl Into<String>) -> RichText {
    RichText::new(text).size(size::HEADING).family(semibold())
}

pub fn body(text: impl Into<String>) -> RichText {
    RichText::new(text).size(size::BODY)
}

/// Body size, in the semibold family.
pub fn body_strong(text: impl Into<String>) -> RichText {
    RichText::new(text).size(size::BODY).family(semibold())
}

pub fn label(text: impl Into<String>) -> RichText {
    RichText::new(text).size(size::LABEL).family(semibold())
}

pub fn caption(text: impl Into<String>) -> RichText {
    RichText::new(text).size(size::CAPTION)
}

// -------------------------------------------------------------------------
// the egui style
// -------------------------------------------------------------------------

/// Paints egui's own surfaces and widgets from the two palettes, and sets
/// the type scale and spacing. Set once per theme, not per frame.
pub fn apply(ctx: &egui::Context) {
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let p = if theme == egui::Theme::Dark {
            DARK
        } else {
            LIGHT
        };
        ctx.style_mut_of(theme, |style| {
            use egui::{FontId, TextStyle};
            style.text_styles = [
                (TextStyle::Small, FontId::proportional(size::CHIP)),
                (TextStyle::Body, FontId::proportional(size::BODY)),
                (TextStyle::Button, FontId::new(size::LABEL, semibold())),
                (TextStyle::Heading, FontId::new(size::TITLE, semibold())),
                (TextStyle::Monospace, FontId::monospace(size::LABEL)),
            ]
            .into();
            style.spacing.button_padding = egui::vec2(14.0, 8.0);
            style.spacing.item_spacing = egui::vec2(8.0, 8.0);
            style.spacing.interact_size.y = 28.0;

            let v = &mut style.visuals;
            v.panel_fill = p.page;
            v.window_fill = p.surface;
            v.window_stroke = egui::Stroke::new(1.0, p.line);
            v.extreme_bg_color = p.surface;
            v.faint_bg_color = p.surface;
            v.code_bg_color = p.sunken;
            v.override_text_color = Some(p.ink);
            v.weak_text_color = Some(p.ink2);
            v.hyperlink_color = p.primary_ink;
            v.warn_fg_color = p.warn;
            v.error_fg_color = p.wrong_ink;
            v.selection.bg_fill = p.primary_soft;
            v.selection.stroke = egui::Stroke::new(1.5, p.primary);
            v.slider_trailing_fill = true;
            v.window_corner_radius = 16.into();
            v.menu_corner_radius = 12.into();
            v.popup_shadow.color = p.shadow;
            v.window_shadow.color = p.shadow;

            let radius = egui::CornerRadius::same(12);
            let w = &mut v.widgets;
            w.noninteractive.bg_fill = p.surface;
            w.noninteractive.weak_bg_fill = p.surface;
            w.noninteractive.bg_stroke = egui::Stroke::new(1.0, p.line);
            w.noninteractive.fg_stroke.color = p.ink;
            w.noninteractive.corner_radius = radius;
            for (state, fill) in [
                (&mut w.inactive, p.sunken),
                (&mut w.hovered, mix(p.sunken, p.ink, 0.06)),
                (&mut w.active, p.primary_soft),
                (&mut w.open, p.sunken),
            ] {
                state.bg_fill = fill;
                state.weak_bg_fill = fill;
                state.bg_stroke = egui::Stroke::NONE;
                state.fg_stroke.color = p.ink;
                state.corner_radius = radius;
                state.expansion = 0.0;
            }
            w.hovered.bg_stroke = egui::Stroke::new(1.0, p.line);
            w.active.fg_stroke.color = p.primary_ink;
        });
    }
}
