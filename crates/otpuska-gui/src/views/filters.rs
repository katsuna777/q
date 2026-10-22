//! Filter controls shared by the screens. Every screen keeps its filter in a core
//! `*Filter` value; the same value (through `Snapshot::*_rows`) decides which rows are shown
//! and which rows are printed.

use crate::ui::{self, Kind};
use egui::{vec2, Align2, CornerRadius, FontId, Popup, PopupCloseBehavior, Sense, Stroke, StrokeKind, Ui};
use egui_phosphor::regular as icon;
use otpuska_core::engine::State;
use otpuska_core::filter::*;
use otpuska_core::Snapshot;

/// Which optional controls an employee filter bar has.
#[derive(Clone, Copy)]
pub struct EmpBar {
    pub work: bool,
    pub states: bool,
    pub took: bool,
    /// Dismissed employees can be picked in the state list.
    pub dismissed: bool,
}

/// A button that looks like a dropdown; the caller draws the popup.
fn select_button(ui: &mut Ui, label: &str, width: f32, active: bool) -> egui::Response {
    let p = ui::pal(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
    let hot = resp.hovered();
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let stroke = if active { p.accent } else if hot { p.text3 } else { p.border_strong };
    ui.painter().rect(rect, CornerRadius::same(7), if hot { p.sunken } else { p.panel }, Stroke::new(1.0, stroke), StrokeKind::Inside);
    let clip = egui::Rect::from_min_max(rect.min + vec2(10.0, 0.0), rect.max - vec2(26.0, 0.0));
    let galley = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(13.0), if active { p.text } else { p.text2 });
    ui.painter().with_clip_rect(clip).galley(egui::pos2(clip.left(), rect.center().y - galley.size().y / 2.0), galley, p.text);
    ui.painter().text(rect.right_center() - vec2(13.0, 0.0), Align2::CENTER_CENTER, icon::CARET_DOWN, FontId::new(12.0, crate::theme::icons()), p.text3);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    resp
}

/// Multi-select of states. An empty selection means "any". Returns `true` when changed.
pub fn state_picker(ui: &mut Ui, id: &str, states: &mut Vec<State>, dismissed: bool, width: f32) -> bool {
    let label = match states.len() {
        0 => "Все состояния".to_string(),
        1 => states[0].label().to_string(),
        n => format!("Состояний: {n}"),
    };
    let btn = select_button(ui, &label, width, !states.is_empty());
    let mut changed = false;
    Popup::from_response(&btn)
        .id(ui.id().with((id, "states")))
        .open_memory(btn.clicked().then_some(egui::SetOpenCommand::Toggle))
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .gap(4.0)
        .show(|ui| {
            ui.set_min_width(230.0);
            ui.spacing_mut().item_spacing.y = 4.0;
            for s in State::ALL.iter().filter(|s| dismissed || **s != State::Dismissed) {
                ui.horizontal(|ui| {
                    let mut on = states.contains(s);
                    if ui::checkbox(ui, &mut on, "").changed() {
                        changed = true;
                        if on {
                            states.push(*s);
                        } else {
                            states.retain(|x| x != s);
                        }
                    }
                    ui::state_pill(ui, *s);
                });
            }
            ui.add_space(2.0);
            if ui::link(ui, None, "Сбросить выбор").clicked() {
                changed |= !states.is_empty();
                states.clear();
            }
        });
    changed
}

/// Period presets with a pair of dates for "own dates".
pub fn period_picker(ui: &mut Ui, id: &str, f: &mut PeriodFilter) {
    let p = ui::pal(ui);
    egui::ComboBox::from_id_salt(ui.id().with((id, "period")))
        .width(150.0)
        .selected_text(ui::text(f.preset.label(), 13.0, p.text))
        .show_ui(ui, |ui| {
            for preset in PeriodPreset::ALL {
                if ui.selectable_label(f.preset == preset, preset.label()).clicked() {
                    f.preset = preset;
                }
            }
        });
    if f.preset == PeriodPreset::Custom {
        ui.label(ui::text("с", 12.5, p.text3));
        ui::date_field(ui, (id, "from"), &mut f.from, "дд.мм.гггг", 138.0);
        ui.label(ui::text("по", 12.5, p.text3));
        ui::date_field(ui, (id, "to"), &mut f.to, "дд.мм.гггг", 138.0);
    }
}

fn took_picker(ui: &mut Ui, id: &str, took: &mut Took) {
    let p = ui::pal(ui);
    ui.label(ui::text("Отпуск", 12.5, p.text3));
    egui::ComboBox::from_id_salt(ui.id().with((id, "took")))
        .width(170.0)
        .selected_text(ui::text(took.label(), 13.0, if *took == Took::Any { p.text2 } else { p.text }))
        .show_ui(ui, |ui| {
            for t in Took::ALL {
                if ui.selectable_label(*took == t, t.label()).clicked() {
                    *took = t;
                }
            }
        });
}

/// "Показано 5 из 108 — печать выведет только эти строки" + the reset button.
/// `narrowed`: the list is smaller than everything; `can_reset`: the filter differs from its default.
pub fn footer(ui: &mut Ui, narrowed: bool, can_reset: bool, shown: usize, total: usize, note: &str) -> bool {
    let p = ui::pal(ui);
    let mut reset = false;
    ui.horizontal(|ui| {
        ui.label(ui::text(
            if narrowed { format!("Показано {shown} из {total} — печать выведет только эти строки") } else { format!("Всего {total} — печать выведет все строки") },
            12.0,
            if narrowed { p.text2 } else { p.text3 },
        ));
        if can_reset {
            reset = ui::button(ui, Kind::Ghost, Some(icon::X), "Сбросить фильтры", true).clicked();
        }
        if !note.is_empty() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(ui::text(note, 12.0, p.text3));
            });
        }
    });
    reset
}

/// Filter bar for lists of employees: search, department, optional work status, states and
/// "took leave in the period". Two explicit rows: a combo box does not wrap by itself.
pub fn emp_bar(ui: &mut Ui, id: &str, snap: &Snapshot, f: &mut EmpFilter, opt: EmpBar) {
    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
    ui.horizontal(|ui| {
        ui::search_input(ui, &mut f.search, "Поиск по ФИО, должности, отделу", 270.0);
        ui::dept_picker(ui, (id, "dept"), snap, &mut f.dept, 220.0, "Все подразделения");
        if opt.work {
            ui::segmented(
                ui,
                &mut f.work,
                &[(WorkFilter::All, "Все"), (WorkFilter::Working, "Работают"), (WorkFilter::Suspended, "Приостановка ТД"), (WorkFilter::Dismissed, "Уволены")],
            );
        } else if opt.states {
            state_picker(ui, id, &mut f.states, opt.dismissed, 190.0);
        }
    });
    if (opt.work && opt.states) || opt.took {
        ui.horizontal(|ui| {
            if opt.work && opt.states {
                state_picker(ui, id, &mut f.states, opt.dismissed, 190.0);
            }
            if opt.took {
                took_picker(ui, id, &mut f.took);
                if f.took != Took::Any {
                    period_picker(ui, id, &mut f.period);
                }
            }
        });
    }
}

/// Filter bar for lists of leaves: search, department, status, kind and period (two rows).
pub fn leave_bar(ui: &mut Ui, id: &str, snap: &Snapshot, f: &mut LeaveFilter, shows: &[LeaveShow], with_kind: bool, with_search: bool) {
    let p = ui::pal(ui);
    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
    ui.horizontal(|ui| {
        if with_search {
            ui::search_input(ui, &mut f.search, "Поиск по ФИО, отделу, примечанию", 270.0);
        }
        ui::dept_picker(ui, (id, "dept"), snap, &mut f.dept, 220.0, "Все подразделения");
        let options: Vec<(LeaveShow, &str)> = shows.iter().map(|s| (*s, s.label())).collect();
        ui::segmented(ui, &mut f.show, &options);
    });
    ui.horizontal(|ui| {
        if with_kind {
            ui.label(ui::text("Вид отпуска", 12.5, p.text3));
            egui::ComboBox::from_id_salt(ui.id().with((id, "kind")))
                .width(140.0)
                .selected_text(ui::text(f.kind.label(), 13.0, if f.kind == LeaveKind::Any { p.text2 } else { p.text }))
                .show_ui(ui, |ui| {
                    for k in LeaveKind::ALL {
                        if ui.selectable_label(f.kind == k, k.label()).clicked() {
                            f.kind = k;
                        }
                    }
                });
            ui.add_space(8.0);
        }
        ui.label(ui::text("Период", 12.5, p.text3));
        period_picker(ui, id, &mut f.period);
    });
}
