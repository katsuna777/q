//! Custom widgets drawn with the app palette: buttons, pills, cards, toggles, segmented
//! controls, text/number/date fields, the employee picker and table helpers.

use crate::theme::{icons, medium, semibold, Pal};
use egui::text::{LayoutJob, TextFormat};
use egui::{
    vec2, Align, Align2, Color32, CornerRadius, FontFamily, FontId, Frame, Id, Layout, Margin, Popup, PopupCloseBehavior,
    Rect, Response, RichText, Sense, Stroke, StrokeKind, TextEdit, Ui, UiBuilder, WidgetInfo, WidgetType,
};
use egui_phosphor::regular as icon;
use otpuska_core::date::{self, Date};
use otpuska_core::engine::{self, PeriodTone, Snapshot, State};
use otpuska_core::model::Status;
use std::hash::Hash;
use std::sync::Arc;

pub fn pal(ui: &Ui) -> Pal {
    ui.ctx().data(|d| d.get_temp::<Pal>(Id::NULL)).unwrap_or_else(|| crate::theme::palette(true, "mono"))
}

// ---- text -----------------------------------------------------------------

pub fn text(s: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(s).size(size).color(color)
}

pub fn strong(s: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(s).size(size).color(color).family(medium())
}

pub fn title(s: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(s).size(size).color(color).family(semibold())
}

pub fn icon_text(glyph: &str, size: f32, color: Color32) -> RichText {
    RichText::new(glyph).size(size).color(color).family(icons())
}

/// Icon + label as one galley, the icon drawn with the icon font. Color is a placeholder,
/// set at paint time.
pub fn icon_label(ui: &Ui, glyph: Option<&str>, label: &str, font: FontId) -> Arc<egui::Galley> {
    let mut job = LayoutJob::default();
    let fmt = |font_id: FontId| TextFormat {
        font_id,
        color: Color32::PLACEHOLDER,
        valign: Align::Center,
        ..Default::default()
    };
    if let Some(g) = glyph {
        job.append(g, 0.0, fmt(FontId::new(font.size + 1.5, icons())));
    }
    if !label.is_empty() {
        job.append(label, if glyph.is_some() { 7.0 } else { 0.0 }, fmt(font));
    }
    ui.painter().layout_job(job)
}

pub fn section_title(ui: &mut Ui, s: &str) {
    let p = pal(ui);
    ui.label(RichText::new(s.to_uppercase()).size(11.0).color(p.text3).family(medium()).extra_letter_spacing(0.6));
}

// ---- buttons --------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Primary,
    Secondary,
    Ghost,
    Danger,
    DangerSolid,
}

pub fn button(ui: &mut Ui, kind: Kind, glyph: Option<&str>, label: &str, enabled: bool) -> Response {
    let p = pal(ui);
    let galley = icon_label(ui, glyph, label, FontId::new(13.0, medium()));
    let pad = if label.is_empty() { 8.0 } else { 13.0 };
    let size = vec2((galley.size().x + pad * 2.0).max(30.0), 30.0);
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    if ui.is_rect_visible(rect) {
        let hot = enabled && resp.hovered();
        let down = enabled && resp.is_pointer_button_down_on();
        let (fill, stroke, fg) = match kind {
            Kind::Primary => (if hot { p.accent_hover } else { p.accent }, Color32::TRANSPARENT, p.on_accent),
            Kind::Secondary => (if hot { p.sunken } else { p.panel }, p.border_strong, p.text),
            Kind::Ghost => (
                if hot { p.hover } else { Color32::TRANSPARENT },
                Color32::TRANSPARENT,
                if hot { p.text } else { p.text2 },
            ),
            Kind::Danger => {
                (if hot { Pal::soft(p.danger) } else { Color32::TRANSPARENT }, Color32::TRANSPARENT, p.danger)
            }
            Kind::DangerSolid => (p.danger, Color32::TRANSPARENT, Color32::WHITE),
        };
        let alpha = if enabled { 1.0 } else { 0.4 };
        let rect = if down { rect.translate(vec2(0.0, 0.5)) } else { rect };
        ui.painter().rect(
            rect,
            CornerRadius::same(7),
            fill.gamma_multiply(alpha),
            Stroke::new(1.0, stroke.gamma_multiply(alpha)),
            StrokeKind::Inside,
        );
        let pos = rect.center() - galley.size() / 2.0;
        ui.painter().galley(pos, galley, fg.gamma_multiply(alpha));
    }
    if enabled && resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

pub fn icon_button(ui: &mut Ui, glyph: &str, tooltip: &str, enabled: bool) -> Response {
    let p = pal(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, tooltip));
    let hot = enabled && resp.hovered();
    if hot {
        ui.painter().rect_filled(rect, CornerRadius::same(7), p.hover);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let color = if !enabled {
        p.text3.gamma_multiply(0.6)
    } else if hot {
        p.text
    } else {
        p.text2
    };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, glyph, FontId::new(16.0, icons()), color);
    if enabled {
        resp.on_hover_text(tooltip)
    } else {
        resp
    }
}

/// Borderless link-style button (`← Сотрудники`).
pub fn link(ui: &mut Ui, glyph: Option<&str>, label: &str) -> Response {
    let p = pal(ui);
    let galley = icon_label(ui, glyph, label, FontId::new(12.5, medium()));
    let (rect, resp) = ui.allocate_exact_size(galley.size(), Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    ui.painter().galley(rect.min, galley, if resp.hovered() { p.text } else { p.text2 });
    resp
}

// ---- pills, cards ---------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Muted,
    Ok,
    Warn,
    Danger,
    Orange,
    Info,
    Violet,
    Salmon,
}

impl Tone {
    pub fn color(self, p: &Pal) -> Color32 {
        match self {
            Tone::Neutral => p.text2,
            Tone::Muted => p.text3,
            Tone::Ok => p.ok,
            Tone::Warn => p.warn,
            Tone::Danger => p.danger,
            Tone::Orange => p.orange,
            Tone::Info => p.info,
            Tone::Violet => p.violet,
            Tone::Salmon => p.salmon,
        }
    }

    pub fn bg(self, p: &Pal) -> Color32 {
        match self {
            Tone::Neutral | Tone::Muted => p.sunken,
            other => Pal::soft(other.color(p)),
        }
    }
}

pub fn pill(ui: &mut Ui, label: &str, tone: Tone) -> Response {
    let p = pal(ui);
    let (bg, fg) = (tone.bg(&p), tone.color(&p));
    let galley = ui.painter().layout_no_wrap(label.to_string(), FontId::new(11.5, medium()), fg);
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 18.0, 21.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(11), bg);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, fg);
    resp
}

pub fn state_tone(s: State) -> Tone {
    match s {
        State::Overdue => Tone::Danger,
        State::Burning => Tone::Orange,
        State::Soon => Tone::Warn,
        State::Normal | State::AllUsed => Tone::Ok,
        State::Advance => Tone::Info,
        State::CheckBooks => Tone::Salmon,
        State::Suspended => Tone::Violet,
        State::Dismissed | State::NoRight => Tone::Muted,
        State::NoHireDate => Tone::Danger,
    }
}

pub fn state_pill(ui: &mut Ui, s: State) -> Response {
    pill(ui, s.label(), state_tone(s)).on_hover_text(s.hint())
}

pub fn period_tone(t: PeriodTone) -> Tone {
    match t {
        PeriodTone::Future => Tone::Muted,
        PeriodTone::Suspended => Tone::Violet,
        PeriodTone::Used => Tone::Ok,
        PeriodTone::Overdue => Tone::Danger,
        PeriodTone::Burning => Tone::Orange,
        PeriodTone::Soon => Tone::Warn,
        PeriodTone::Open => Tone::Info,
    }
}

pub fn card_frame(ui: &Ui) -> Frame {
    let p = pal(ui);
    Frame::new()
        .fill(p.panel)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(16))
}

/// A full-width card.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    card_frame(ui)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// A card without inner padding, for tables.
pub fn flush_card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    card_frame(ui)
        .inner_margin(Margin::ZERO)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// A small inset card of a fixed width.
pub fn sunken_card<R>(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = pal(ui);
    Frame::new()
        .fill(p.sunken)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(width - 24.0);
            add(ui)
        })
        .inner
}

pub fn divider(ui: &mut Ui) {
    let p = pal(ui);
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, p.border);
}

/// A banner line (warning / info) above a table.
pub fn banner(ui: &mut Ui, tone: Tone, glyph: &str, msg: &str) {
    let p = pal(ui);
    let color = tone.color(&p);
    Frame::new()
        .fill(Pal::soft(color))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.35)))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.label(icon_text(glyph, 15.0, color));
                ui.add(egui::Label::new(text(msg, 12.5, p.text)).wrap());
            });
        });
}

// ---- KPI tile ---------------------------------------------------------------

pub fn kpi(ui: &mut Ui, label: &str, value: &str, sub: &str, tone: Tone, active: bool) -> Response {
    let p = pal(ui);
    let color = tone.color(&p);
    let stroke = if active { Stroke::new(1.5, color) } else { Stroke::new(1.0, p.border) };
    let r = card_frame(ui).stroke(stroke).inner_margin(Margin::symmetric(14, 12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 3.0;
        ui.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
            ui.painter().circle_filled(dot.center(), 4.0, color);
            section_title(ui, label);
        });
        ui.label(title(value, 24.0, p.text));
        ui.add(egui::Label::new(text(sub, 11.5, p.text3)).truncate());
    });
    let resp = r.response.interact(Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

// ---- toggles, segments ------------------------------------------------------------

pub fn checkbox(ui: &mut Ui, checked: &mut bool, label: &str) -> Response {
    let p = pal(ui);
    let galley = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(13.0), p.text);
    let extra = if label.is_empty() { 0.0 } else { galley.size().x + 8.0 };
    let (rect, mut resp) = ui.allocate_exact_size(vec2(16.0 + extra, 22.0), Sense::click());
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, *checked, label));
    if resp.clicked() {
        *checked = !*checked;
        resp.mark_changed();
    }
    let boxr = Rect::from_min_size(egui::pos2(rect.left(), rect.center().y - 8.0), vec2(16.0, 16.0));
    if *checked {
        ui.painter().rect_filled(boxr, CornerRadius::same(5), p.accent);
        let s = Stroke::new(1.8, p.on_accent);
        let a = boxr.left_top() + vec2(4.0, 8.2);
        let b = boxr.left_top() + vec2(7.0, 11.0);
        let c = boxr.left_top() + vec2(12.0, 5.2);
        ui.painter().line_segment([a, b], s);
        ui.painter().line_segment([b, c], s);
    } else {
        let stroke = if resp.hovered() { p.text3 } else { p.border_strong };
        ui.painter().rect(boxr, CornerRadius::same(5), p.panel, Stroke::new(1.5, stroke), StrokeKind::Inside);
    }
    if !label.is_empty() {
        ui.painter().galley(egui::pos2(boxr.right() + 8.0, rect.center().y - galley.size().y / 2.0), galley, p.text);
    }
    resp
}

pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let p = pal(ui);
    let (rect, mut resp) = ui.allocate_exact_size(vec2(34.0, 20.0), Sense::click());
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, *on, ""));
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, *on, 0.12);
    let bg = if *on { p.accent } else { p.border_strong };
    ui.painter().rect_filled(rect, CornerRadius::same(10), bg);
    let x = egui::lerp(rect.left() + 10.0..=rect.right() - 10.0, t);
    let knob = if *on { p.on_accent } else { Color32::WHITE };
    ui.painter().circle_filled(egui::pos2(x, rect.center().y), 7.0, knob);
    resp
}

pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let p = pal(ui);
    let font = FontId::new(12.5, medium());
    let galleys: Vec<_> = options
        .iter()
        .map(|(_, l)| ui.painter().layout_no_wrap(l.to_string(), font.clone(), Color32::PLACEHOLDER))
        .collect();
    let widths: Vec<f32> = galleys.iter().map(|g| g.size().x + 24.0).collect();
    let total = widths.iter().sum::<f32>() + 4.0;
    let (rect, base) = ui.allocate_exact_size(vec2(total, 30.0), Sense::hover());
    ui.painter().rect(rect, CornerRadius::same(8), p.sunken, Stroke::new(1.0, p.border), StrokeKind::Inside);
    let mut x = rect.left() + 2.0;
    let mut changed = false;
    for (i, ((val, label), galley)) in options.iter().zip(galleys).enumerate() {
        let r = Rect::from_min_size(egui::pos2(x, rect.top() + 2.0), vec2(widths[i], rect.height() - 4.0));
        let resp = ui.interact(r, base.id.with(i), Sense::click());
        resp.widget_info(|| WidgetInfo::selected(WidgetType::RadioButton, true, *value == *val, *label));
        let selected = *value == *val;
        if selected {
            ui.painter().rect(r, CornerRadius::same(6), p.panel, Stroke::new(1.0, p.border_strong), StrokeKind::Inside);
        } else if resp.hovered() {
            ui.painter().rect_filled(r, CornerRadius::same(6), p.hover);
        }
        let color = if selected { p.text } else { p.text2 };
        ui.painter().galley(r.center() - galley.size() / 2.0, galley, color);
        if resp.clicked() && !selected {
            *value = *val;
            changed = true;
        }
        x += widths[i];
    }
    changed
}

// ---- text fields ---------------------------------------------------------------

pub fn input(ui: &mut Ui, value: &mut String, hint: &str, width: f32) -> Response {
    ui.add(
        TextEdit::singleline(value)
            .hint_text(hint)
            .desired_width(width)
            .margin(Margin::symmetric(10, 7))
            .font(FontId::proportional(13.0)),
    )
}

pub fn multiline(ui: &mut Ui, value: &mut String, hint: &str, width: f32, rows: usize) -> Response {
    ui.add(
        TextEdit::multiline(value)
            .hint_text(hint)
            .desired_width(width)
            .desired_rows(rows)
            .margin(Margin::symmetric(10, 7))
            .font(FontId::proportional(13.0)),
    )
}

pub fn search_input(ui: &mut Ui, value: &mut String, hint: &str, width: f32) -> Response {
    let p = pal(ui);
    let resp = ui.add(
        TextEdit::singleline(value)
            .hint_text(hint)
            .desired_width(width)
            .margin(Margin { left: 10, right: 30, top: 7, bottom: 7 })
            .font(FontId::proportional(13.0)),
    );
    if value.is_empty() {
        ui.painter().text(
            egui::pos2(resp.rect.right() - 16.0, resp.rect.center().y),
            Align2::CENTER_CENTER,
            icon::MAGNIFYING_GLASS,
            FontId::new(14.0, icons()),
            p.text3,
        );
    } else {
        let r = Rect::from_center_size(egui::pos2(resp.rect.right() - 16.0, resp.rect.center().y), vec2(20.0, 20.0));
        let x = ui.interact(r, resp.id.with("clear"), Sense::click());
        ui.painter().text(
            r.center(),
            Align2::CENTER_CENTER,
            icon::X_CIRCLE,
            FontId::new(14.0, icons()),
            if x.hovered() { p.text } else { p.text3 },
        );
        if x.clicked() {
            value.clear();
        }
    }
    resp
}

fn invalid_outline(ui: &Ui, resp: &Response) {
    let p = pal(ui);
    ui.painter().rect_stroke(resp.rect, CornerRadius::same(7), Stroke::new(1.5, p.danger), StrokeKind::Inside);
}

pub fn parse_num(s: &str) -> Option<f64> {
    let v: f64 = s.trim().replace(',', ".").parse().ok()?;
    (v.is_finite() && v >= 0.0).then_some(v)
}

/// Optional number (days). Empty text means "not set". Returns `true` when the value changed.
pub fn num_field(ui: &mut Ui, id: impl Hash + std::fmt::Debug, value: &mut Option<f64>, hint: &str, width: f32) -> bool {
    let id = ui.id().with(id);
    let focused = ui.memory(|m| m.has_focus(id));
    let mut buf: String = if focused {
        ui.data(|d| d.get_temp::<String>(id)).unwrap_or_default()
    } else {
        value.map(engine::fmt_days).unwrap_or_default()
    };
    let resp = ui.add(
        TextEdit::singleline(&mut buf)
            .id(id)
            .hint_text(hint)
            .desired_width(width)
            .margin(Margin::symmetric(10, 7))
            .font(FontId::proportional(13.0)),
    );
    let mut changed = false;
    if resp.changed() {
        let new = if buf.trim().is_empty() { Some(None) } else { parse_num(&buf).map(Some) };
        if let Some(new) = new {
            changed = *value != new;
            *value = new;
        }
    }
    if !buf.trim().is_empty() && parse_num(&buf).is_none() {
        invalid_outline(ui, &resp);
    }
    if focused || resp.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, buf));
    }
    changed
}

/// Required number (days), never negative.
pub fn num_field_req(ui: &mut Ui, id: impl Hash + std::fmt::Debug, value: &mut f64, hint: &str, width: f32) -> bool {
    let mut opt = Some(*value);
    let changed = num_field(ui, id, &mut opt, hint, width);
    if changed {
        *value = opt.unwrap_or(0.0);
    }
    changed
}

/// Date entry: type `дд.мм.гггг` (or `ддммгггг`) or pick in the calendar.
/// Returns `true` when the value changed.
pub fn date_field(ui: &mut Ui, id: impl Hash + std::fmt::Debug, value: &mut Option<Date>, hint: &str, width: f32) -> bool {
    let p = pal(ui);
    let id = ui.id().with(id);
    let focused = ui.memory(|m| m.has_focus(id));
    let mut buf: String = if focused {
        ui.data(|d| d.get_temp::<String>(id)).unwrap_or_default()
    } else {
        date::fmt_opt(*value)
    };
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let resp = ui.add(
            TextEdit::singleline(&mut buf)
                .id(id)
                .hint_text(hint)
                .desired_width(width - 30.0)
                .margin(Margin { left: 10, right: 4, top: 7, bottom: 7 })
                .font(FontId::proportional(13.0)),
        );
        if resp.changed() {
            let new = if buf.trim().is_empty() { Some(None) } else { date::parse_ru(&buf).map(Some) };
            if let Some(new) = new {
                changed |= *value != new;
                *value = new;
            }
        }
        if !buf.trim().is_empty() && date::parse_ru(&buf).is_none() {
            invalid_outline(ui, &resp);
        }
        if focused || resp.has_focus() {
            ui.data_mut(|d| d.insert_temp(id, buf.clone()));
        }

        // Calendar button.
        let (rect, btn) = ui.allocate_exact_size(vec2(28.0, resp.rect.height()), Sense::click());
        btn.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Календарь"));
        let hot = btn.hovered();
        if hot {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            icon::CALENDAR_BLANK,
            FontId::new(15.0, icons()),
            if hot { p.text } else { p.text3 },
        );
        let popup_id = id.with("cal");
        Popup::from_response(&btn)
            .id(popup_id)
            .open_memory(btn.clicked().then_some(egui::SetOpenCommand::Toggle))
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .gap(4.0)
            .show(|ui| {
                if calendar(ui, popup_id, value) {
                    changed = true;
                    ui.close();
                }
            });
    });
    changed
}

/// Month grid. Returns `true` when a day (or "today"/"clear") was chosen.
fn calendar(ui: &mut Ui, id: Id, value: &mut Option<Date>) -> bool {
    use chrono::Datelike;
    let p = pal(ui);
    let today = date::today();
    let anchor = value.unwrap_or(today);
    let (mut year, mut month): (i32, u32) =
        ui.data(|d| d.get_temp(id.with("ym"))).unwrap_or((anchor.year(), anchor.month()));
    let mut picked = false;
    ui.set_width(252.0);
    ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
    ui.horizontal(|ui| {
        if icon_button(ui, icon::CARET_LEFT, "Предыдущий месяц", true).clicked() {
            if month == 1 {
                (year, month) = (year - 1, 12);
            } else {
                month -= 1;
            }
        }
        ui.allocate_ui_with_layout(vec2(168.0, 28.0), Layout::centered_and_justified(egui::Direction::LeftToRight), |ui| {
            ui.label(strong(format!("{} {}", date::MONTHS[month as usize - 1], year), 13.0, p.text));
        });
        if icon_button(ui, icon::CARET_RIGHT, "Следующий месяц", true).clicked() {
            if month == 12 {
                (year, month) = (year + 1, 1);
            } else {
                month += 1;
            }
        }
    });
    ui.add_space(2.0);
    let cell = vec2(34.0, 26.0);
    ui.horizontal(|ui| {
        for w in date::WEEKDAYS {
            let (r, _) = ui.allocate_exact_size(cell, Sense::hover());
            ui.painter().text(r.center(), Align2::CENTER_CENTER, w, FontId::new(11.0, medium()), p.text3);
        }
    });
    let first = Date::from_ymd_opt(year, month, 1).unwrap_or(today);
    let lead = first.weekday().num_days_from_monday() as i64;
    let days = date::days_in_month(year, month) as i64;
    let weeks = (lead + days + 6) / 7;
    for week in 0..weeks {
        ui.horizontal(|ui| {
            for dow in 0..7 {
                let n = week * 7 + dow - lead + 1;
                let (r, resp) = ui.allocate_exact_size(cell, Sense::click());
                if n < 1 || n > days {
                    continue;
                }
                let d = Date::from_ymd_opt(year, month, n as u32).unwrap_or(first);
                let selected = *value == Some(d);
                if selected {
                    ui.painter().rect_filled(r.shrink(1.0), CornerRadius::same(6), p.accent);
                } else if resp.hovered() {
                    ui.painter().rect_filled(r.shrink(1.0), CornerRadius::same(6), p.hover);
                } else if d == today {
                    ui.painter().rect_stroke(r.shrink(1.0), CornerRadius::same(6), Stroke::new(1.0, p.border_strong), StrokeKind::Inside);
                }
                let color = if selected {
                    p.on_accent
                } else if dow >= 5 {
                    p.text2
                } else {
                    p.text
                };
                ui.painter().text(r.center(), Align2::CENTER_CENTER, n.to_string(), FontId::new(12.5, medium()), color);
                if resp.clicked() {
                    *value = Some(d);
                    picked = true;
                }
            }
        });
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if link(ui, None, "Сегодня").clicked() {
            *value = Some(today);
            picked = true;
        }
        ui.add_space(12.0);
        if link(ui, None, "Очистить").clicked() {
            *value = None;
            picked = true;
        }
    });
    ui.data_mut(|d| d.insert_temp(id.with("ym"), (year, month)));
    picked
}

/// Searchable employee selector. Returns `true` when the choice changed.
pub fn emp_picker(ui: &mut Ui, id: impl Hash + std::fmt::Debug, snap: &Snapshot, value: &mut Option<i64>, hint: &str, width: f32, only_active: bool) -> bool {
    let p = pal(ui);
    let id = ui.id().with(id);
    let current = value.and_then(|i| snap.emp_by_id(i));
    let label = current.map(|r| r.emp.fio.clone()).unwrap_or_else(|| hint.to_string());

    let (rect, btn) = ui.allocate_exact_size(vec2(width, 32.0), Sense::click());
    btn.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, format!("Выбор сотрудника: {label}")));
    let hot = btn.hovered();
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    ui.painter().rect(
        rect,
        CornerRadius::same(7),
        if hot { p.sunken } else { p.panel },
        Stroke::new(1.0, if hot { p.text3 } else { p.border_strong }),
        StrokeKind::Inside,
    );
    let clip = Rect::from_min_max(rect.min + vec2(10.0, 0.0), rect.max - vec2(28.0, 0.0));
    let galley = ui.painter().layout_no_wrap(label, FontId::proportional(13.0), if current.is_some() { p.text } else { p.text3 });
    ui.painter().with_clip_rect(clip).galley(egui::pos2(clip.left(), rect.center().y - galley.size().y / 2.0), galley, p.text);
    ui.painter().text(
        rect.right_center() - vec2(14.0, 0.0),
        Align2::CENTER_CENTER,
        icon::CARET_UP_DOWN,
        FontId::new(13.0, icons()),
        p.text3,
    );

    let mut changed = false;
    let popup_id = id.with("pop");
    Popup::from_response(&btn)
        .id(popup_id)
        .open_memory(btn.clicked().then_some(egui::SetOpenCommand::Toggle))
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .gap(4.0)
        .width(width.max(340.0))
        .show(|ui| {
            let filter_id = id.with("filter");
            let mut filter: String = ui.data(|d| d.get_temp(filter_id)).unwrap_or_default();
            let r = ui.add(
                TextEdit::singleline(&mut filter)
                    .hint_text("Поиск по ФИО…")
                    .desired_width(f32::INFINITY)
                    .margin(Margin::symmetric(10, 7))
                    .font(FontId::proportional(13.0)),
            );
            // The box is rebuilt every frame from this copy, so what is typed has to be written back.
            ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
            if !r.has_focus() && ui.data(|d| d.get_temp::<bool>(id.with("focused")).is_none()) {
                r.request_focus();
                ui.data_mut(|d| d.insert_temp(id.with("focused"), true));
            }
            let needle = filter.trim().to_lowercase();
            let matches: Vec<usize> = snap
                .emps
                .iter()
                .enumerate()
                .filter(|(_, r)| !only_active || r.emp.status != Status::Dismissed || Some(r.emp.id) == *value)
                .filter(|(_, r)| needle.is_empty() || r.emp.fio.to_lowercase().contains(&needle))
                .map(|(i, _)| i)
                .collect();
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let cur = *value;
            let mut choose = |ui: &mut Ui, i: usize| {
                *value = Some(snap.emps[i].emp.id);
                changed = true;
                ui.data_mut(|d| {
                    d.remove::<String>(filter_id);
                    d.remove::<bool>(id.with("focused"));
                });
                ui.close();
            };
            if enter {
                if let Some(&first) = matches.first() {
                    choose(ui, first);
                    return;
                }
            }
            ui.add_space(4.0);
            egui::ScrollArea::vertical().max_height(264.0).auto_shrink([false, true]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                if matches.is_empty() {
                    ui.add_space(6.0);
                    ui.label(text("Никого не найдено", 12.5, p.text3));
                }
                for &i in matches.iter().take(200) {
                    let row = &snap.emps[i];
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
                    let selected = cur == Some(row.emp.id);
                    if selected {
                        ui.painter().rect_filled(rect, CornerRadius::same(6), p.active);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(rect, CornerRadius::same(6), p.hover);
                    }
                    let name_color = if row.emp.status == Status::Dismissed { p.text3 } else { p.text };
                    ui.painter().text(
                        rect.left_top() + vec2(10.0, 9.0),
                        Align2::LEFT_CENTER,
                        &row.emp.fio,
                        FontId::new(13.0, medium()),
                        name_color,
                    );
                    let sub = if row.dept.is_empty() { row.emp.position.clone() } else { row.dept.clone() };
                    ui.painter().with_clip_rect(rect).text(
                        rect.left_top() + vec2(10.0, 24.0),
                        Align2::LEFT_CENTER,
                        sub,
                        FontId::new(11.0, FontFamily::Proportional),
                        p.text3,
                    );
                    if resp.clicked() {
                        choose(ui, i);
                    }
                }
                if matches.len() > 200 {
                    ui.add_space(6.0);
                    ui.label(text(format!("Показаны первые 200 из {} — уточните поиск", matches.len()), 11.5, p.text3));
                }
            });
        });
    if !Popup::is_id_open(ui.ctx(), popup_id) {
        // Closed (by a choice or by a click outside): the next opening starts with an empty search.
        ui.data_mut(|d| {
            d.remove::<bool>(id.with("focused"));
            d.remove::<String>(id.with("filter"));
        });
    }
    changed
}

/// Department dropdown with a "not set" entry. Returns `true` when the choice changed.
pub fn dept_picker(ui: &mut Ui, id: impl Hash + std::fmt::Debug, snap: &Snapshot, value: &mut Option<i64>, width: f32, any_label: &str) -> bool {
    let p = pal(ui);
    let mut changed = false;
    let shown = if value.is_some() { snap.dept_name(*value).to_string() } else { any_label.to_string() };
    egui::ComboBox::from_id_salt(ui.id().with(id))
        .width(width)
        .selected_text(text(shown, 13.0, if value.is_some() { p.text } else { p.text3 }))
        .show_ui(ui, |ui| {
            ui.set_min_width(width);
            if ui.selectable_label(value.is_none(), any_label).clicked() {
                *value = None;
                changed = true;
            }
            for d in &snap.departments {
                if ui.selectable_label(*value == Some(d.id), &d.name).clicked() {
                    *value = Some(d.id);
                    changed = true;
                }
            }
        });
    changed
}

/// A collapsible block: a clickable title row, the content below while open.
/// The open state is remembered per `id`.
pub fn accordion(ui: &mut Ui, id: impl Hash + std::fmt::Debug, glyph: &str, title: &str, default_open: bool, add: impl FnOnce(&mut Ui)) {
    let p = pal(ui);
    let id = ui.id().with(id);
    let mut open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(default_open);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, open, title));
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(7), p.hover);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    ui.painter().text(rect.left_center() + vec2(8.0, 0.0), Align2::LEFT_CENTER, glyph, FontId::new(17.0, icons()), p.text2);
    ui.painter().text(rect.left_center() + vec2(36.0, 0.0), Align2::LEFT_CENTER, title, FontId::new(13.5, medium()), p.text);
    ui.painter().text(
        rect.right_center() - vec2(10.0, 0.0),
        Align2::RIGHT_CENTER,
        if open { icon::CARET_UP } else { icon::CARET_DOWN },
        FontId::new(14.0, icons()),
        p.text3,
    );
    if resp.clicked() {
        open = !open;
    }
    ui.data_mut(|d| d.insert_temp(id, open));
    if open {
        Frame::new().inner_margin(Margin { left: 36, right: 8, top: 2, bottom: 12 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
    }
}

// ---- layout helpers ---------------------------------------------------------------

/// A fixed-height row: `left` vertically centered on the left, `right` right-aligned.
pub fn split_row(ui: &mut Ui, height: f32, left: impl FnOnce(&mut Ui), right: impl FnOnce(&mut Ui)) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let mut r = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::right_to_left(Align::Center)));
    right(&mut r);
    let left_rect = Rect::from_min_max(rect.min, egui::pos2((r.min_rect().left() - 16.0).max(rect.left()), rect.max.y));
    let mut l = ui.new_child(UiBuilder::new().max_rect(left_rect).layout(Layout::left_to_right(Align::Center)));
    l.set_clip_rect(left_rect.intersect(ui.clip_rect()));
    left(&mut l);
}

/// Lay out `add` in a column of at most `max_width`, centered horizontally.
pub fn centered<R>(ui: &mut Ui, max_width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let full = ui.available_rect_before_wrap();
    let w = full.width().min(max_width);
    let rect = Rect::from_min_size(egui::pos2(full.left() + (full.width() - w) / 2.0, full.top()), vec2(w, full.height()));
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Min)));
    let r = add(&mut child);
    let used = child.min_rect();
    ui.allocate_rect(Rect::from_min_max(full.min, egui::pos2(full.right(), used.bottom())), Sense::hover());
    r
}

pub fn empty_state(ui: &mut Ui, glyph: &str, heading: &str, body: &str) {
    let p = pal(ui);
    ui.vertical_centered(|ui| {
        ui.add_space(28.0);
        ui.label(icon_text(glyph, 30.0, p.text3));
        ui.add_space(6.0);
        ui.label(title(heading, 15.0, p.text));
        ui.label(text(body, 12.5, p.text2));
        ui.add_space(20.0);
    });
}

/// A labelled form field: small caption above the control.
pub fn field<R>(ui: &mut Ui, label: &str, hint: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = pal(ui);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.horizontal(|ui| {
            ui.label(strong(label, 12.0, p.text2));
            if !hint.is_empty() {
                ui.label(text(hint, 11.0, p.text3));
            }
        });
        add(ui)
    })
    .inner
}

// ---- tables -----------------------------------------------------------------------

pub const ROW_H: f32 = 38.0;
pub const HEAD_H: f32 = 34.0;

use egui_extras::{Column, TableBuilder, TableRow};

pub fn table<'a>(ui: &'a mut Ui, id: &str) -> TableBuilder<'a> {
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    TableBuilder::new(ui)
        .id_salt(id)
        .striped(false)
        .resizable(false)
        .sense(Sense::click())
        .cell_layout(Layout::left_to_right(Align::Center))
        .auto_shrink([false, true])
}

pub fn exact(w: f32) -> Column {
    Column::exact(w)
}

pub fn rest(min: f32) -> Column {
    Column::remainder().at_least(min).clip(true)
}

/// Header cell: small uppercase caption with a bottom rule. Returns the click response
/// (use `sortable` for the sort arrow).
pub fn head_cell(row: &mut TableRow, label: &str, align_right: bool) {
    row.col(|ui| head_content(ui, label, align_right, None));
}

/// Clickable header; `sort` is `Some(ascending)` for the active sort column.
pub fn head_sort(row: &mut TableRow, label: &str, align_right: bool, sort: Option<bool>) -> bool {
    let (_, resp) = row.col(|ui| head_content(ui, label, align_right, Some(sort)));
    if resp.hovered() {
        resp.ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

fn head_content(ui: &mut Ui, label: &str, align_right: bool, sort: Option<Option<bool>>) {
    let p = pal(ui);
    let rect = ui.max_rect();
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, p.border));
    let arrow = match sort {
        Some(Some(true)) => format!("{} ↑", label.to_uppercase()),
        Some(Some(false)) => format!("{} ↓", label.to_uppercase()),
        _ => label.to_uppercase(),
    };
    let color = if matches!(sort, Some(Some(_))) { p.text } else { p.text3 };
    let rt = RichText::new(arrow).size(10.5).color(color).family(medium()).extra_letter_spacing(0.5);
    if align_right {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(10.0);
            ui.add(egui::Label::new(rt).truncate().selectable(false));
        });
    } else {
        ui.add_space(10.0);
        ui.add(egui::Label::new(rt).truncate().selectable(false));
    }
}

/// Header cell with an explanation shown on hover.
pub fn head_tip(row: &mut TableRow, label: &str, align_right: bool, tip: &str) {
    let (_, resp) = row.col(|ui| head_content(ui, label, align_right, None));
    resp.on_hover_text(tip);
}

/// A body cell with the row separator painted at the bottom.
pub fn cell(row: &mut TableRow, add: impl FnOnce(&mut Ui)) -> Response {
    let (_, resp) = row.col(|ui| {
        let p = pal(ui);
        let rect = ui.max_rect();
        ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, p.border));
        ui.add_space(10.0);
        add(ui);
    });
    resp
}

pub fn cell_text(row: &mut TableRow, s: impl Into<String>, color: Color32) -> Response {
    let s: String = s.into();
    cell(row, |ui| {
        ui.add(egui::Label::new(text(s, 13.0, color)).truncate().selectable(false));
    })
}

pub fn cell_strong(row: &mut TableRow, s: impl Into<String>, color: Color32) -> Response {
    let s: String = s.into();
    cell(row, |ui| {
        ui.add(egui::Label::new(strong(s, 13.0, color)).truncate().selectable(false));
    })
}

pub fn cell_right(row: &mut TableRow, s: impl Into<String>, color: Color32) -> Response {
    let s: String = s.into();
    let (_, resp) = row.col(|ui| {
        let p = pal(ui);
        let rect = ui.max_rect();
        ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, p.border));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(10.0);
            ui.add(egui::Label::new(text(s, 13.0, color)).truncate().selectable(false));
        });
    });
    resp
}

pub fn cell_pill(row: &mut TableRow, label: &str, tone: Tone) -> Response {
    cell(row, |ui| {
        pill(ui, label, tone);
    })
}

pub fn cell_state(row: &mut TableRow, s: State) -> Response {
    cell(row, |ui| {
        state_pill(ui, s);
    })
}

/// "28 дн." etc. with the unit left out; negative values are red.
pub fn days_color(p: &Pal, v: Option<f64>) -> Color32 {
    match v {
        Some(v) if v < 0.0 => p.danger,
        Some(_) => p.text,
        None => p.text3,
    }
}

/// `/Users/me/docs/x` → `~/docs/x`.
pub fn tildify(path: &std::path::Path) -> String {
    let s = path.to_string_lossy().into_owned();
    match std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
        Ok(home) if !home.is_empty() && s.starts_with(&home) => format!("~{}", &s[home.len()..]),
        _ => s,
    }
}

pub const ICON_BACK: &str = icon::ARROW_LEFT;
