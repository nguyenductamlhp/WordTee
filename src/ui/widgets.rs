//! The pieces every screen is built from: cards, buttons, chips, answer
//! options, settings rows, the headers and the bottom sheets.
//!
//! Most are painted by hand rather than assembled from egui's stock widgets,
//! because the stock ones carry egui's desktop proportions — 2-point corners,
//! 18-point rows — and a phone wants 44-point targets and one corner radius.

use eframe::egui::{
    self, Align, Color32, CornerRadius, FontId, Layout, Margin, Rect, Response, Sense, Stroke,
    StrokeKind, Ui, Vec2, pos2, vec2,
};

use super::icons::{Icon, paint_icon};
use super::theme::{self, Palette, mix, palette, semibold, size};
use crate::dict::{Band, Pos};
use crate::progress::State;

/// The id every screen's header panel shares. Only one screen draws per
/// frame, so they never meet, and it gives the safe-area test one panel to
/// check whichever screen is showing.
pub const HEADER_ID: &str = "chrome";

/// The smallest a tap target gets.
pub const TOUCH: f32 = 44.0;

/// The page's side gutter.
pub const GUTTER: i8 = 16;

// -------------------------------------------------------------------------
// surfaces
// -------------------------------------------------------------------------

/// The frame of a card: white, a hairline border, 16-point corners.
pub fn card_frame(ui: &Ui) -> egui::Frame {
    let p = palette(ui);
    egui::Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.line))
        .corner_radius(16)
        .inner_margin(16)
        .shadow(egui::Shadow {
            offset: [0, 1],
            blur: 3,
            spread: 0,
            color: p.shadow,
        })
}

/// A full-width card.
pub fn card<R>(ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    card_frame(ui).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        body(ui)
    })
}

/// A card that is one tap target as a whole. Returns whether it was tapped.
pub fn tappable_card<R>(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    frame: egui::Frame,
    body: impl FnOnce(&mut Ui) -> R,
) -> (bool, R) {
    let id = ui.id().with(("tap", id_salt));
    // Last frame's hover, so the card can be tinted before it is drawn.
    let hovered = ui.ctx().read_response(id).is_some_and(|r| r.hovered());
    let p = palette(ui);
    let frame = if hovered {
        frame.fill(mix(frame.fill, p.primary_soft, 0.5))
    } else {
        frame
    };
    let inner = frame.show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        body(ui)
    });
    let response = ui
        .interact(inner.response.rect, id, Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    (response.clicked(), inner.inner)
}

/// A list card: no vertical padding, so its rows can run edge to edge and
/// carry their own.
pub fn list<R>(ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> R {
    card_frame(ui)
        .inner_margin(Margin::symmetric(14, 0))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            body(ui)
        })
        .inner
}

/// Scrolling page content with the side gutter.
pub fn page<R>(ui: &mut Ui, id_salt: &str, body: impl FnOnce(&mut Ui) -> R) -> R {
    egui::ScrollArea::vertical()
        .id_salt(id_salt)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::new()
                .inner_margin(Margin {
                    left: GUTTER,
                    right: GUTTER,
                    top: 4,
                    bottom: 24,
                })
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 12.0;
                    body(ui)
                })
                .inner
        })
        .inner
}

/// A row whose contents are centred as a group.
///
/// `ui.horizontal` always takes the whole width and starts at the left, so
/// a row of IPA and buttons under a centred headword used to sit flush left
/// beneath it. This remembers how wide the row came out last frame and
/// indents by half the difference.
pub fn centered_row<R>(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    let id = ui.id().with(("centered", id_salt));
    let width: Option<f32> = ui.data(|d| d.get_temp(id));
    ui.horizontal(|ui| {
        let pad = ((ui.available_width() - width.unwrap_or(0.0)) / 2.0).max(0.0);
        ui.add_space(pad);
        let start = ui.cursor().left();
        let inner = add(ui);
        let used = ui.min_rect().right() - start;
        if width.is_none_or(|w| (w - used).abs() > 0.5) {
            ui.data_mut(|d| d.insert_temp(id, used));
            ui.ctx().request_discard("centre a row");
        }
        inner
    })
    .inner
}

// -------------------------------------------------------------------------
// headers and bottom bars
// -------------------------------------------------------------------------

fn header_frame(ui: &Ui, left: i8) -> egui::Frame {
    egui::Frame::new()
        .fill(palette(ui).page)
        .inner_margin(Margin {
            left,
            right: GUTTER,
            top: 8,
            bottom: 4,
        })
}

/// A 44-point row, laid out left to right with its contents centred
/// vertically.
fn bar_row<R>(ui: &mut Ui, height: f32, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    let width = ui.available_width();
    ui.allocate_ui_with_layout(
        vec2(width, height),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_min_size(vec2(width, height));
            add(ui)
        },
    )
}

/// A tab's header: its title on the left, anything in `trailing` on the
/// right.
pub fn screen_header(ui: &mut Ui, title: &str, trailing: impl FnOnce(&mut Ui)) {
    egui::Panel::top(HEADER_ID)
        .frame(header_frame(ui, GUTTER))
        .show_separator_line(false)
        .show(ui, |ui| {
            bar_row(ui, TOUCH, |ui| {
                ui.label(theme::title(title));
                ui.with_layout(Layout::right_to_left(Align::Center), trailing);
            });
        });
}

/// The header of a screen inside a tab — a word page, a settings page —
/// where the first thing is usually a back button.
pub fn bar_header(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    egui::Panel::top(HEADER_ID)
        .frame(header_frame(ui, 8))
        .show_separator_line(false)
        .show(ui, |ui| bar_row(ui, TOUCH, add));
}

/// A back button and a title.
pub fn back_header(ui: &mut Ui, title: &str) -> bool {
    let mut back = false;
    bar_header(ui, |ui| {
        back = icon_button(ui, Icon::ChevronLeft, "Back", IconStyle::Plain).clicked();
        ui.label(theme::heading(title));
    });
    back
}

/// A focused flow's header — a session, a test: close, a progress bar and
/// a count. Returns whether close was tapped.
pub fn focus_header(ui: &mut Ui, fraction: f32, count: &str) -> bool {
    let mut close = false;
    bar_header(ui, |ui| {
        close = icon_button(ui, Icon::Cross, "Close", IconStyle::Plain).clicked();
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let p = palette(ui);
            let galley_width = 44.0;
            ui.allocate_ui_with_layout(
                vec2(galley_width, TOUCH),
                Layout::right_to_left(Align::Center),
                |ui| ui.label(theme::label(count).color(p.ink2)),
            );
            ui.add_space(4.0);
            progress_track(ui, fraction, 10.0, p.primary);
        });
    });
    close
}

/// A bar pinned to the bottom of the screen, on the card colour.
pub fn action_bar<R>(ui: &mut Ui, id: &'static str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = palette(ui);
    egui::Panel::bottom(id)
        .frame(
            egui::Frame::new()
                .fill(p.surface)
                .inner_margin(Margin::symmetric(GUTTER, 12)),
        )
        .show(ui, add)
        .inner
}

/// A sheet rising from the bottom edge, with rounded top corners.
pub fn sheet<R>(ui: &mut Ui, id: &'static str, fill: Color32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = palette(ui);
    egui::Panel::bottom(id)
        .frame(
            egui::Frame::new()
                .fill(fill)
                .corner_radius(CornerRadius {
                    nw: 20,
                    ne: 20,
                    sw: 0,
                    se: 0,
                })
                .inner_margin(Margin {
                    left: GUTTER,
                    right: GUTTER,
                    top: 16,
                    bottom: 14,
                })
                .shadow(egui::Shadow {
                    offset: [0, -4],
                    blur: 20,
                    spread: 0,
                    color: p.shadow,
                }),
        )
        .show_separator_line(false)
        .show(ui, add)
        .inner
}

/// The verdict on an answer, with the Continue that moves on. Returns
/// whether Continue was tapped (or Enter pressed).
pub fn feedback_sheet(ui: &mut Ui, right: bool, title: &str, detail: &str, note: &str) -> bool {
    let p = palette(ui);
    let (fill, badge, on_badge, ink, icon) = if right {
        (p.known_soft, p.known, p.on_known, p.known_ink, Icon::Check)
    } else {
        (p.wrong_soft, p.wrong, p.on_wrong, p.wrong_ink, Icon::Cross)
    };
    sheet(ui, "feedback", fill, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::hover());
            ui.painter().circle_filled(rect.center(), 16.0, badge);
            paint_icon(
                ui.painter(),
                Rect::from_center_size(rect.center(), Vec2::splat(18.0)),
                icon,
                on_badge,
                badge,
            );
            ui.label(
                egui::RichText::new(title)
                    .size(18.0)
                    .family(semibold())
                    .color(ink),
            );
        });
        if !detail.is_empty() {
            ui.add_space(4.0);
            ui.label(theme::body(detail));
        }
        if !note.is_empty() {
            ui.label(theme::caption(note).color(p.ink2));
        }
        ui.add_space(10.0);
        let go = primary_button(ui, "Continue").clicked();
        go || ui.input(|i| i.key_pressed(egui::Key::Enter))
    })
}

// -------------------------------------------------------------------------
// buttons
// -------------------------------------------------------------------------

/// What a button is for, which decides how loud it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// The one thing the screen is for. Filled purple.
    Primary,
    /// "I know it": the teal of the state it leads to.
    Know,
    /// Everything else that is a button.
    Secondary,
    /// A lighter action beside a title.
    Outline,
    /// Text only.
    Ghost,
    /// Erasing things.
    Danger,
}

fn button_colors(p: &Palette, kind: Kind) -> (Color32, Color32, Option<Color32>) {
    match kind {
        Kind::Primary => (p.primary, p.on_primary, None),
        Kind::Know => (p.known_soft, p.known_ink, None),
        Kind::Secondary => (p.sunken, p.ink, None),
        Kind::Outline => (p.surface, p.primary_ink, Some(p.line)),
        Kind::Ghost => (Color32::TRANSPARENT, p.primary_ink, None),
        Kind::Danger => (p.wrong_soft, p.wrong_ink, None),
    }
}

/// A button of exactly `size`, with an optional icon before its label.
pub fn button(ui: &mut Ui, kind: Kind, icon: Option<Icon>, text: &str, size: Vec2) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, text));
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let p = palette(ui);
    let (fill, ink, stroke) = button_colors(&p, kind);
    let pressed = enabled && response.is_pointer_button_down_on();
    let hovered = enabled && response.hovered();
    let fill = match (fill == Color32::TRANSPARENT, pressed, hovered) {
        (true, true, _) | (true, _, true) => p.primary_soft,
        (true, ..) => fill,
        (false, true, _) => mix(fill, p.ink, 0.16),
        (false, false, true) => mix(fill, p.ink, 0.07),
        (false, false, false) => fill,
    };
    let mut painter = ui.painter().clone();
    if !enabled {
        painter.multiply_opacity(0.4);
    }
    let radius = if size.y >= 48.0 { 14.0 } else { 12.0 };
    painter.rect(
        rect,
        radius,
        fill,
        stroke.map_or(Stroke::NONE, |c| Stroke::new(1.0, c)),
        StrokeKind::Inside,
    );
    if response.has_focus() {
        painter.rect_stroke(
            rect.expand(2.0),
            radius + 2.0,
            Stroke::new(2.0, p.primary),
            StrokeKind::Outside,
        );
    }
    let font = if size.y >= 48.0 { size::BODY } else { 15.0 };
    let galley = painter.layout(
        text.to_owned(),
        FontId::new(font, semibold()),
        ink,
        (rect.width() - 24.0).max(10.0),
    );
    let icon_width = if icon.is_some() { 28.0 } else { 0.0 };
    let mut x = rect.center().x - (icon_width + galley.size().x) / 2.0;
    if let Some(icon) = icon {
        let at = Rect::from_center_size(pos2(x + 10.0, rect.center().y), Vec2::splat(20.0));
        paint_icon(&painter, at, icon, ink, fill);
        x += icon_width;
    }
    painter.galley(
        pos2(x, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The full-width primary action.
pub fn primary_button(ui: &mut Ui, text: &str) -> Response {
    let width = ui.available_width();
    button(ui, Kind::Primary, None, text, vec2(width, 52.0))
}

/// A full-width secondary action.
pub fn secondary_button(ui: &mut Ui, text: &str) -> Response {
    let width = ui.available_width();
    button(ui, Kind::Secondary, None, text, vec2(width, TOUCH))
}

/// The decision the word page, the map, a new card and Quick Scan all ask
/// for, under the same two names everywhere: `(knew, learn)`.
///
/// It used to be "Should Learn / Already Knew" in one place, "Learn this /
/// Already know it" (in the other order) in another and "Don't know / Known"
/// in a third.
pub fn decision_buttons(ui: &mut Ui) -> (bool, bool) {
    let gap = 10.0;
    let width = ui.available_width();
    let knew_width = ((width - gap) / 2.25).floor();
    let mut out = (false, false);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        out.0 = button(
            ui,
            Kind::Know,
            Some(Icon::Check),
            "I know it",
            vec2(knew_width, 52.0),
        )
        .clicked();
        let rest = ui.available_width();
        out.1 = button(
            ui,
            Kind::Primary,
            Some(Icon::Plus),
            "Learn it",
            vec2(rest, 52.0),
        )
        .clicked();
    });
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IconStyle {
    /// On a pale wash of the action colour.
    Soft,
    /// Just the icon, in ink.
    Plain,
}

/// A round 44-point button carrying one icon. `label` is what a screen
/// reader says, and the tooltip on a desktop.
pub fn icon_button(ui: &mut Ui, icon: Icon, label: &str, style: IconStyle) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(TOUCH), Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    let p = palette(ui);
    let (fill, ink) = match style {
        IconStyle::Soft => (p.primary_soft, p.primary_ink),
        IconStyle::Plain => (Color32::TRANSPARENT, p.ink),
    };
    let fill = if enabled && response.hovered() {
        mix(
            if fill == Color32::TRANSPARENT {
                p.page
            } else {
                fill
            },
            p.ink,
            0.08,
        )
    } else {
        fill
    };
    let mut painter = ui.painter().clone();
    if !enabled {
        painter.multiply_opacity(0.35);
    }
    painter.circle_filled(rect.center(), TOUCH / 2.0, fill);
    paint_icon(
        &painter,
        Rect::from_center_size(rect.center(), Vec2::splat(24.0)),
        icon,
        ink,
        fill,
    );
    response
        .on_hover_text(label)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A pill-shaped button with a short label, on the soft wash: "0.75×".
pub fn pill_button(ui: &mut Ui, text: &str, label: &str) -> Response {
    let p = palette(ui);
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        FontId::new(13.0, semibold()),
        p.primary_ink,
    );
    let size = vec2(galley.size().x + 24.0, TOUCH);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let fill = if response.hovered() {
        mix(p.primary_soft, p.ink, 0.08)
    } else {
        p.primary_soft
    };
    ui.painter().rect_filled(rect, TOUCH / 2.0, fill);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, p.primary_ink);
    response
        .on_hover_text(label)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

// -------------------------------------------------------------------------
// answer options
// -------------------------------------------------------------------------

/// How an answer option looks once the question has been answered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    /// Before the answer.
    Plain,
    /// The right answer, whether or not it was picked.
    Right,
    /// The pick, when it was wrong.
    Wrong,
    /// Neither: stepped back so the two that matter stand out.
    Faded,
}

/// A full-width answer option: a letter key, the text, and after the answer
/// a tick or a cross.
///
/// The verdict tints the whole card *and* carries an icon. A change of hue
/// inside a line of Vietnamese is easy to miss — the eye is reading the
/// words, not watching their colour — and colour alone is no help to anyone
/// who cannot tell the two apart.
pub fn answer_option(ui: &mut Ui, index: usize, text: &str, mark: Mark) -> Response {
    let p = palette(ui);
    let width = ui.available_width();
    let (pad, key, gap) = (12.0, 28.0, 12.0);
    let trailing = if matches!(mark, Mark::Right | Mark::Wrong) {
        34.0
    } else {
        0.0
    };
    let wrap = (width - pad - key - gap - trailing - 14.0).max(40.0);
    let galley = ui.painter().layout(
        text.to_owned(),
        FontId::proportional(size::BODY),
        p.ink,
        wrap,
    );
    let height = (galley.size().y + 22.0).max(56.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let letter = ["A", "B", "C", "D", "E", "F"]
        .get(index)
        .copied()
        .unwrap_or("");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, text));

    let hovered = mark == Mark::Plain && response.hovered();
    let (fill, line, key_fill, key_ink) = match mark {
        Mark::Right => (p.known_soft, p.known_line, p.known, p.on_known),
        Mark::Wrong => (p.wrong_soft, p.wrong_line, p.wrong, p.on_wrong),
        Mark::Plain if hovered => (p.surface, p.primary, p.primary_soft, p.primary_ink),
        Mark::Plain | Mark::Faded => (p.surface, p.line, p.sunken, p.ink2),
    };
    let mut painter = ui.painter().clone();
    if mark == Mark::Faded {
        painter.multiply_opacity(0.45);
    }
    painter.rect(rect, 14.0, fill, Stroke::new(1.5, line), StrokeKind::Inside);
    if response.has_focus() {
        painter.rect_stroke(
            rect.expand(2.0),
            16.0,
            Stroke::new(2.0, p.primary),
            StrokeKind::Outside,
        );
    }
    let key_center = pos2(rect.left() + pad + key / 2.0, rect.center().y);
    painter.circle_filled(key_center, key / 2.0, key_fill);
    let letter = painter.layout_no_wrap(letter.to_owned(), FontId::new(13.0, semibold()), key_ink);
    painter.galley(key_center - letter.size() / 2.0, letter, key_ink);
    painter.galley(
        pos2(
            rect.left() + pad + key + gap,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        p.ink,
    );
    let verdict = match mark {
        Mark::Right => Some((Icon::Check, p.known_ink)),
        Mark::Wrong => Some((Icon::Cross, p.wrong_ink)),
        _ => None,
    };
    if let Some((icon, color)) = verdict {
        let at = Rect::from_center_size(
            pos2(rect.right() - 14.0 - 12.0, rect.center().y),
            Vec2::splat(24.0),
        );
        paint_icon(&painter, at, icon, color, fill);
    }
    if mark == Mark::Plain {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

/// Which mark option `index` gets, given the pick and the answer.
///
/// A wrong pick marks two options, not one: the pick in red *and* the right
/// one in teal. Marking only the mistake says what not to think without ever
/// saying what to.
pub fn mark_for(picked: Option<usize>, index: usize, answer: usize) -> Mark {
    match picked {
        None => Mark::Plain,
        Some(_) if index == answer => Mark::Right,
        Some(chosen) if chosen == index => Mark::Wrong,
        Some(_) => Mark::Faded,
    }
}

// -------------------------------------------------------------------------
// text fields
// -------------------------------------------------------------------------

/// How a typed answer is drawn: before it is checked, or after.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FieldState {
    Typing,
    Right,
    Wrong,
}

fn field_frame(ui: &Ui, id: egui::Id, state: FieldState) -> egui::Frame {
    let p = palette(ui);
    let focused = ui.memory(|m| m.has_focus(id));
    let (fill, line, width) = match state {
        FieldState::Right => (p.known_soft, p.known_line, 1.5),
        FieldState::Wrong => (p.wrong_soft, p.wrong_line, 1.5),
        FieldState::Typing if focused => (p.surface, p.primary, 2.0),
        FieldState::Typing => (p.surface, p.line, 1.0),
    };
    egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(width, line))
        .corner_radius(14)
        .inner_margin(Margin::symmetric(14, 0))
}

/// The box a typed answer goes in.
pub fn answer_field(
    ui: &mut Ui,
    id_salt: &str,
    text: &mut String,
    hint: &str,
    state: FieldState,
) -> Response {
    let id = ui.make_persistent_id(id_salt);
    let frame = field_frame(ui, id, state);
    frame
        .show(ui, |ui| {
            let width = ui.available_width();
            ui.allocate_ui_with_layout(
                vec2(width, 56.0),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.set_min_size(vec2(width, 56.0));
                    ui.add(
                        egui::TextEdit::singleline(text)
                            .id(id)
                            .frame(egui::Frame::new())
                            .font(FontId::proportional(18.0))
                            .hint_text(hint)
                            .desired_width(f32::INFINITY)
                            .interactive(state == FieldState::Typing),
                    )
                },
            )
            .inner
        })
        .inner
}

/// The search box: a magnifier, the field, and a clear button once there is
/// something to clear.
pub fn search_field(ui: &mut Ui, text: &mut String, hint: &str) -> Response {
    let id = ui.make_persistent_id("search");
    let p = palette(ui);
    let frame = field_frame(ui, id, FieldState::Typing).inner_margin(Margin {
        left: 12,
        right: 4,
        top: 0,
        bottom: 0,
    });
    frame
        .show(ui, |ui| {
            let width = ui.available_width();
            ui.allocate_ui_with_layout(
                vec2(width, 52.0),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.set_min_size(vec2(width, 52.0));
                    let (icon, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
                    paint_icon(ui.painter(), icon, Icon::Search, p.ink3, p.surface);
                    let clear_width = if text.is_empty() { 0.0 } else { TOUCH };
                    let field = ui.add(
                        egui::TextEdit::singleline(text)
                            .id(id)
                            .frame(egui::Frame::new())
                            .font(FontId::proportional(17.0))
                            .hint_text(hint)
                            .desired_width(ui.available_width() - clear_width - 8.0),
                    );
                    if !text.is_empty()
                        && icon_button(ui, Icon::Cross, "Clear", IconStyle::Plain).clicked()
                    {
                        text.clear();
                        field.request_focus();
                    }
                    field
                },
            )
            .inner
        })
        .inner
}

// -------------------------------------------------------------------------
// chips
// -------------------------------------------------------------------------

/// What leads a chip's text.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Lead {
    None,
    Dot(Color32),
    /// A four-bar frequency meter, `n` bars lit.
    Meter(u8),
    Icon(Icon, Color32),
}

struct PillStyle {
    fill: Color32,
    ink: Color32,
    stroke: Option<Color32>,
    lead: Lead,
    height: f32,
    font: f32,
}

fn pill(ui: &mut Ui, text: &str, style: PillStyle) -> Response {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        FontId::new(style.font, semibold()),
        style.ink,
    );
    let lead_width = match style.lead {
        Lead::None => 0.0,
        Lead::Dot(_) => 7.0 + 6.0,
        Lead::Meter(_) => 15.0 + 6.0,
        Lead::Icon(..) => 16.0 + 6.0,
    };
    let pad = if style.height > 28.0 { 12.0 } else { 10.0 };
    let size = vec2(pad * 2.0 + lead_width + galley.size().x, style.height);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect(
        rect,
        style.height / 2.0,
        style.fill,
        style.stroke.map_or(Stroke::NONE, |c| Stroke::new(1.5, c)),
        StrokeKind::Inside,
    );
    let x = rect.left() + pad;
    let y = rect.center().y;
    match style.lead {
        Lead::None => {}
        Lead::Dot(color) => {
            painter.circle_filled(pos2(x + 3.5, y), 3.5, color);
        }
        Lead::Meter(lit) => {
            let off = mix(style.fill, style.ink, 0.3);
            for i in 0..4u8 {
                let h = 5.0 + i as f32 * 2.5;
                let bar = Rect::from_min_max(
                    pos2(x + i as f32 * 4.0, y + 6.0 - h),
                    pos2(x + i as f32 * 4.0 + 2.8, y + 6.0),
                );
                painter.rect_filled(bar, 1.0, if i < lit { style.ink } else { off });
            }
        }
        Lead::Icon(icon, color) => {
            let at = Rect::from_center_size(pos2(x + 8.0, y), Vec2::splat(16.0));
            paint_icon(painter, at, icon, color, style.fill);
        }
    }
    painter.galley(
        pos2(x + lead_width, y - galley.size().y / 2.0),
        galley,
        style.ink,
    );
    response
}

/// The colour families a plain chip comes in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Neutral,
    Primary,
    Known,
    Wrong,
    Streak,
}

/// A small label in a pill.
pub fn chip(ui: &mut Ui, text: &str, tone: Tone) -> Response {
    if text.is_empty() {
        return ui.allocate_response(Vec2::ZERO, Sense::hover());
    }
    let p = palette(ui);
    let (fill, ink) = match tone {
        Tone::Neutral => (p.sunken, p.ink2),
        Tone::Primary => (p.primary_soft, p.primary_ink),
        Tone::Known => (p.known_soft, p.known_ink),
        Tone::Wrong => (p.wrong_soft, p.wrong_ink),
        Tone::Streak => (p.streak_soft, p.streak_ink),
    };
    pill(
        ui,
        text,
        PillStyle {
            fill,
            ink,
            stroke: None,
            lead: Lead::None,
            height: 26.0,
            font: size::CHIP,
        },
    )
}

/// The fill a knowledge state gets on the map and in bars.
pub fn state_color(ui: &Ui, state: State) -> Color32 {
    state_fill(&palette(ui), state)
}

pub fn state_fill(p: &Palette, state: State) -> Color32 {
    match state {
        State::Known | State::Mastered => p.known,
        State::AssumedKnown => p.assumed,
        State::Learning | State::Review => p.learning,
        State::Unexplored => p.unexplored,
    }
}

/// `(fill, text, dot, outline)` for a state's chip.
///
/// The text is always a text colour. It used to be the state's fill, which
/// put "New" on the page at 1.2:1 and "Known" at 2:1 — the colours were made
/// to be painted as squares, not read as words.
pub fn state_chip_colors(
    p: &Palette,
    state: State,
) -> (Color32, Color32, Option<Color32>, Option<Color32>) {
    match state {
        State::Mastered => (p.known, p.on_known, None, None),
        State::Known => (p.known_soft, p.known_ink, Some(p.known), None),
        State::AssumedKnown => (p.surface, p.known_ink, None, Some(p.assumed)),
        State::Learning | State::Review => (p.primary_soft, p.primary_ink, Some(p.learning), None),
        State::Unexplored => (p.sunken, p.ink2, Some(p.unexplored_dot), None),
    }
}

pub fn state_chip(ui: &mut Ui, state: State) -> Response {
    let (fill, ink, dot, stroke) = state_chip_colors(&palette(ui), state);
    pill(
        ui,
        state.label(),
        PillStyle {
            fill,
            ink,
            stroke,
            lead: dot.map_or(Lead::None, Lead::Dot),
            height: 26.0,
            font: size::CHIP,
        },
    )
}

/// The part of speech, spelled out.
pub fn pos_chip(ui: &mut Ui, pos: Pos) -> Response {
    chip(ui, pos.label(), Tone::Neutral)
}

/// Spec 1.3's commonness indicator: the band by name, its rank, and a meter.
///
/// Neutral on purpose. It used to be teal for Core and purple for Advanced —
/// the colours of "known" and "learning" — so a card could show two purple
/// chips that meant unrelated things.
pub fn band_chip(ui: &mut Ui, band: Band, rank: u32) -> Response {
    let p = palette(ui);
    let lit = match band {
        Band::Core => 4,
        Band::Advanced => 3,
        Band::Academic => 2,
        Band::Rare => 1,
    };
    let text = match rank {
        0 => band.label().to_owned(),
        r => format!("{} · #{}", band.label(), super::thousands(r)),
    };
    pill(
        ui,
        &text,
        PillStyle {
            fill: p.sunken,
            ink: p.ink2,
            stroke: None,
            lead: Lead::Meter(lit),
            height: 26.0,
            font: size::CHIP,
        },
    )
}

/// The streak, with the ball from the logo.
pub fn streak_pill(ui: &mut Ui, days: u32) -> Response {
    let p = palette(ui);
    let text = if days == 1 {
        "1 day".to_owned()
    } else {
        format!("{days} days")
    };
    pill(
        ui,
        &text,
        PillStyle {
            fill: p.streak_soft,
            ink: p.streak_ink,
            stroke: None,
            lead: Lead::Icon(Icon::Ball, p.streak),
            height: 32.0,
            font: size::LABEL,
        },
    )
}

// -------------------------------------------------------------------------
// bars
// -------------------------------------------------------------------------

/// A rounded progress bar filling the width it is given.
pub fn progress_track(ui: &mut Ui, fraction: f32, height: f32, color: Color32) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let p = palette(ui);
    ui.painter().rect_filled(rect, height / 2.0, p.sunken);
    let fraction = fraction.clamp(0.0, 1.0);
    if fraction > 0.0 {
        let width = (rect.width() * fraction).max(height);
        let fill = Rect::from_min_size(rect.min, vec2(width, height));
        ui.painter().rect_filled(fill, height / 2.0, color);
    }
    response
}

/// Spec 2.3's four-colour bar for a stretch of the list. `counts` is
/// indexed by [`State`]; the inferred part is hatched, so it never reads as
/// confirmed.
pub fn stacked_bar(ui: &mut Ui, counts: [u32; 6], height: f32) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let p = palette(ui);
    let total: u32 = counts.iter().sum();
    let parts = [
        (
            counts[State::Mastered as usize] + counts[State::Known as usize],
            p.known,
            false,
        ),
        (counts[State::AssumedKnown as usize], p.assumed, true),
        (
            counts[State::Review as usize] + counts[State::Learning as usize],
            p.learning,
            false,
        ),
        (counts[State::Unexplored as usize], p.unexplored, false),
    ];
    let painter = ui.painter();
    if total == 0 {
        painter.rect_filled(rect, height / 2.0, p.unexplored);
        return response;
    }
    let shown: Vec<_> = parts.iter().filter(|(n, ..)| *n > 0).collect();
    let gap = 2.0;
    let room = rect.width() - gap * (shown.len().saturating_sub(1)) as f32;
    let mut x = rect.left();
    let r = (height / 2.0).min(255.0) as u8;
    for (i, (n, color, hatched)) in shown.iter().enumerate() {
        let width = (room * *n as f32 / total as f32).max(2.0);
        let part = Rect::from_min_size(pos2(x, rect.top()), vec2(width, height));
        let corners = CornerRadius {
            nw: if i == 0 { r } else { 1 },
            sw: if i == 0 { r } else { 1 },
            ne: if i + 1 == shown.len() { r } else { 1 },
            se: if i + 1 == shown.len() { r } else { 1 },
        };
        painter.rect_filled(part, corners, *color);
        if *hatched {
            hatch(painter, part, p.assumed_hatch);
        }
        x += width + gap;
    }
    response
}

/// Diagonal stripes over `rect`, for "inferred, not confirmed".
pub fn hatch(painter: &egui::Painter, rect: Rect, color: Color32) {
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let stroke = Stroke::new(2.0, color);
    let mut x = rect.left() - rect.height();
    while x < rect.right() {
        painter.line_segment(
            [pos2(x, rect.bottom()), pos2(x + rect.height(), rect.top())],
            stroke,
        );
        x += 6.0;
    }
}

/// A number over its label, on a tile.
pub fn stat_tile(ui: &mut Ui, value: &str, label: &str, value_color: Color32, width: f32) {
    let p = palette(ui);
    egui::Frame::new()
        .fill(p.page)
        .corner_radius(12)
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            // Its own column: inside a row the frame would otherwise lay the
            // number and its label side by side.
            ui.vertical(|ui| {
                ui.set_width(width - 24.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(
                    egui::RichText::new(value)
                        .size(26.0)
                        .family(semibold())
                        .color(value_color),
                );
                ui.label(theme::caption(label).color(p.ink2).family(semibold()));
            });
        });
}

// -------------------------------------------------------------------------
// controls
// -------------------------------------------------------------------------

/// An on/off switch. Returns its response, `changed()` when flipped.
pub fn toggle(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(vec2(46.0, 28.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let value = *on;
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, value, label));
    let p = palette(ui);
    let t = ui.ctx().animate_bool_responsive(response.id, value);
    let track = mix(p.track, p.primary, t);
    ui.painter().rect_filled(rect, 14.0, track);
    let x = egui::lerp((rect.left() + 14.0)..=(rect.right() - 14.0), t);
    ui.painter().circle_filled(
        pos2(x, rect.center().y + 0.5),
        11.0,
        Color32::from_black_alpha(40),
    );
    ui.painter()
        .circle_filled(pos2(x, rect.center().y), 11.0, Color32::WHITE);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn segment_galleys(ui: &Ui, options: &[&str]) -> Vec<std::sync::Arc<egui::Galley>> {
    let p = palette(ui);
    options
        .iter()
        .map(|o| {
            ui.painter().layout_no_wrap(
                (*o).to_owned(),
                FontId::new(size::LABEL, semibold()),
                p.ink,
            )
        })
        .collect()
}

/// Each cell's width: the text plus padding, the padding giving way when
/// the whole pill would not fit in `room`.
fn segment_widths(galleys: &[std::sync::Arc<egui::Galley>], room: f32) -> Vec<f32> {
    let natural: Vec<f32> = galleys
        .iter()
        .map(|g| (g.size().x + 20.0).max(40.0))
        .collect();
    if natural.iter().sum::<f32>() + 6.0 <= room {
        return natural;
    }
    let text: f32 = galleys.iter().map(|g| g.size().x).sum();
    let pad = ((room - 6.0 - text) / galleys.len() as f32).max(6.0);
    galleys.iter().map(|g| g.size().x + pad).collect()
}

/// How wide [`segmented`] draws these options at its natural size.
pub fn segmented_width(ui: &Ui, options: &[&str]) -> f32 {
    segment_widths(&segment_galleys(ui, options), f32::INFINITY)
        .iter()
        .sum::<f32>()
        + 6.0
}

/// A segmented control: two to four choices in a pill, one selected.
pub fn segmented(ui: &mut Ui, options: &[&str], selected: usize) -> Option<usize> {
    let p = palette(ui);
    let galleys = segment_galleys(ui, options);
    let widths = segment_widths(&galleys, ui.available_width());
    let size = vec2(widths.iter().sum::<f32>() + 6.0, 36.0);
    let (rect, pill) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, 10.0, p.sunken);
    let mut picked = None;
    let mut x = rect.left() + 3.0;
    for (i, (galley, width)) in galleys.into_iter().zip(widths).enumerate() {
        let cell = Rect::from_min_size(pos2(x, rect.top() + 3.0), vec2(width, 30.0));
        x += width;
        // Each cell's id comes from the pill's own, which egui numbers per
        // widget like a button's. Not from `ui.id()`: sibling `Ui`s share
        // that, so every row in Settings would have had the same cells, and a
        // tap on one control could flip the others.
        let response = ui
            .interact(cell, pill.id.with(i), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let chosen = i == selected;
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, chosen, galley.text())
        });
        if chosen {
            ui.painter()
                .rect_filled(cell.translate(vec2(0.0, 1.0)), 8.0, p.shadow);
            ui.painter().rect_filled(cell, 8.0, p.surface);
        } else if response.hovered() {
            ui.painter()
                .rect_filled(cell, 8.0, mix(p.sunken, p.ink, 0.06));
        }
        let ink = if chosen { p.ink } else { p.ink2 };
        ui.painter()
            .galley(cell.center() - galley.size() / 2.0, galley, ink);
        if response.clicked() {
            picked = Some(i);
        }
    }
    picked
}

// -------------------------------------------------------------------------
// settings rows
// -------------------------------------------------------------------------

/// A row's height.
const ROW: f32 = 54.0;

/// A group of rows under a heading.
pub fn settings_group<R>(ui: &mut Ui, title: &str, body: impl FnOnce(&mut Ui) -> R) -> R {
    let p = palette(ui);
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(theme::label(title).color(p.ink2));
    });
    list(ui, body)
}

/// The icon on a pale tile that starts a row.
pub fn icon_tile(ui: &mut Ui, icon: Icon) {
    let p = palette(ui);
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::hover());
    ui.painter().rect_filled(rect, 9.0, p.primary_soft);
    paint_icon(
        ui.painter(),
        Rect::from_center_size(rect.center(), Vec2::splat(18.0)),
        icon,
        p.primary_ink,
        p.primary_soft,
    );
}

/// Lays out one row: a hairline above it unless it is the first, the icon
/// tile, the label, and `trailing` against the right edge.
fn row<R>(
    ui: &mut Ui,
    icon: Option<Icon>,
    label: &str,
    trailing: impl FnOnce(&mut Ui) -> R,
) -> (Rect, R) {
    let p = palette(ui);
    let first = ui.cursor().top() <= ui.max_rect().top() + 0.5;
    let inner = bar_row(ui, ROW, |ui| {
        if let Some(icon) = icon {
            icon_tile(ui, icon);
            ui.add_space(4.0);
        }
        ui.label(theme::body(label));
        ui.with_layout(Layout::right_to_left(Align::Center), trailing)
            .inner
    });
    let rect = inner.response.rect;
    if !first {
        ui.painter()
            .hline(rect.x_range(), rect.top(), Stroke::new(1.0, p.line));
    }
    (rect, inner.inner)
}

/// A row whose control is an on/off switch. Returns whether it changed.
pub fn switch_row(ui: &mut Ui, icon: Icon, label: &str, on: &mut bool) -> bool {
    row(ui, Some(icon), label, |ui| toggle(ui, on, label).changed()).1
}

/// A row whose control is a segmented pill. A pill too wide to share the
/// line with its label goes on its own line beneath it, rather than over it.
pub fn choice_row(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    options: &[&str],
    selected: usize,
) -> Option<usize> {
    let label_width = ui
        .painter()
        .layout_no_wrap(
            label.to_owned(),
            FontId::proportional(size::BODY),
            Color32::WHITE,
        )
        .size()
        .x;
    let beside = 32.0 + 12.0 + label_width + 16.0;
    if beside + segmented_width(ui, options) <= ui.available_width() {
        return row(ui, Some(icon), label, |ui| segmented(ui, options, selected)).1;
    }
    row(ui, Some(icon), label, |_| ());
    let mut picked = None;
    bar_row(ui, 48.0, |ui| {
        // Under the label if it fits there, flush left if it does not.
        let indent = (ui.available_width() - segmented_width(ui, options)).clamp(0.0, 44.0);
        ui.add_space((indent - ui.spacing().item_spacing.x).max(0.0));
        picked = segmented(ui, options, selected);
    });
    ui.add_space(6.0);
    picked
}

/// A row that shows a value and opens something when tapped.
pub fn value_row(ui: &mut Ui, icon: Option<Icon>, label: &str, value: &str) -> bool {
    let p = palette(ui);
    let (rect, ()) = row(ui, icon, label, |ui| {
        let (chevron, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
        paint_icon(ui.painter(), chevron, Icon::ChevronRight, p.ink3, p.surface);
        if !value.is_empty() {
            ui.label(egui::RichText::new(value).size(15.0).color(p.ink2));
        }
    });
    ui.interact(rect, ui.id().with(("row", label)), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// A row in a pick-one list: a tick on the chosen one.
pub fn check_row(ui: &mut Ui, label: &str, detail: &str, checked: bool) -> bool {
    let p = palette(ui);
    let (rect, ()) = row(ui, None, label, |ui| {
        if checked {
            let (tick, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
            paint_icon(ui.painter(), tick, Icon::Check, p.primary_ink, p.surface);
        }
        if !detail.is_empty() {
            ui.label(egui::RichText::new(detail).size(15.0).color(p.ink2));
        }
    });
    ui.interact(rect, ui.id().with(("check", label)), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// A destructive action, centred in its own row.
pub fn danger_row(ui: &mut Ui, label: &str) -> bool {
    let p = palette(ui);
    let rect = bar_row(ui, ROW, |ui| {
        ui.with_layout(
            Layout::centered_and_justified(egui::Direction::LeftToRight),
            |ui| {
                ui.label(theme::body_strong(label).color(p.wrong_ink));
            },
        );
    })
    .response
    .rect;
    ui.interact(rect, ui.id().with(("danger", label)), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

// -------------------------------------------------------------------------
// the tab bar
// -------------------------------------------------------------------------

/// One tab: an icon on an indicator pill, its caption under it.
pub fn tab_button(ui: &mut Ui, icon: Icon, selected: bool, label: &str) -> Response {
    let height = 4.0 + 30.0 + 3.0 + 16.0;
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label)
    });
    let p = palette(ui);
    let color = if selected { p.primary_ink } else { p.ink3 };
    let indicator = Rect::from_center_size(
        pos2(rect.center().x, rect.top() + 4.0 + 15.0),
        vec2(56.0, 30.0),
    );
    let behind = if selected {
        ui.painter().rect_filled(indicator, 15.0, p.primary_soft);
        p.primary_soft
    } else if response.hovered() {
        ui.painter().rect_filled(indicator, 15.0, p.sunken);
        p.sunken
    } else {
        p.surface
    };
    paint_icon(
        ui.painter(),
        Rect::from_center_size(indicator.center(), Vec2::splat(24.0)),
        icon,
        color,
        behind,
    );
    // Painted rather than laid out, so a caption cannot wrap and buckle the
    // bar: it is one centred line at any width.
    ui.painter().text(
        pos2(rect.center().x, rect.bottom()),
        egui::Align2::CENTER_BOTTOM,
        label,
        FontId::new(12.0, semibold()),
        color,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

// -------------------------------------------------------------------------
// lists and sections
// -------------------------------------------------------------------------

/// A free-form row inside a [`list`]: a hairline above it unless it is the
/// first, 12 points of padding, and the whole row one tap target. Returns
/// whether it was tapped.
pub fn list_row(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    body: impl FnOnce(&mut Ui),
) -> bool {
    let p = palette(ui);
    let first = ui.cursor().top() <= ui.max_rect().top() + 0.5;
    let id = ui.id().with(("list-row", id_salt));
    let hovered = ui.ctx().read_response(id).is_some_and(|r| r.hovered());
    let background = ui.painter().add(egui::Shape::Noop);
    let inner = egui::Frame::new()
        .inner_margin(Margin::symmetric(0, 12))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            body(ui);
        });
    let rect = inner.response.rect;
    if hovered {
        ui.painter()
            .set(background, egui::Shape::rect_filled(rect, 8.0, p.page));
    }
    if !first {
        ui.painter()
            .hline(rect.x_range(), rect.top(), Stroke::new(1.0, p.line));
    }
    ui.interact(rect, id, Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// A section's heading over a card, with an optional note on the right.
pub fn section_title(ui: &mut Ui, title: &str, note: &str) {
    let p = palette(ui);
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(theme::heading(title));
        if !note.is_empty() {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(4.0);
                ui.label(theme::caption(note).color(p.ink3));
            });
        }
    });
}

/// A 40-point button as wide as its label.
pub fn compact_button(ui: &mut Ui, kind: Kind, icon: Option<Icon>, text: &str) -> Response {
    let width = ui
        .painter()
        .layout_no_wrap(
            text.to_owned(),
            FontId::new(15.0, semibold()),
            Color32::WHITE,
        )
        .size()
        .x;
    let icon_width = if icon.is_some() { 28.0 } else { 0.0 };
    button(ui, kind, icon, text, vec2(width + icon_width + 30.0, 40.0))
}

/// A row that opens and closes what follows it.
pub fn disclosure_row(ui: &mut Ui, label: &str, open: bool) -> bool {
    let p = palette(ui);
    let (rect, ()) = row(ui, None, label, |ui| {
        let (chevron, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
        let icon = if open {
            Icon::ChevronUp
        } else {
            Icon::ChevronDown
        };
        paint_icon(ui.painter(), chevron, icon, p.ink3, p.surface);
    });
    ui.interact(rect, ui.id().with(("disclose", label)), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}
