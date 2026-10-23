//! «Напоминания»: cards of the employees whose leave starts soon, whose planned leave is not in
//! the journal, or whose maternity / childcare leave is about to end.

use super::filters;
use crate::app::{page, top_bar, App};
use crate::theme::Pal;
use crate::ui::{self, Kind, Tone};
use egui::{vec2, Align, CornerRadius, Frame, Layout, Margin, Stroke, Ui};
use egui_phosphor::regular as icon;
use otpuska_core::date;
use otpuska_core::engine::{self, Snapshot};
use otpuska_core::filter::{ReminderFilter, ReminderShow};
use otpuska_core::model::{Leave, LeaveType};
use otpuska_core::schedule::{Band, Reminder, ReminderKind};

#[derive(Default)]
pub struct State {
    pub filter: ReminderFilter,
}

const CARD_W: f32 = 372.0;
const GAP: f32 = 12.0;

fn band_tone(b: Band) -> Tone {
    match b {
        Band::Missed => Tone::Danger,
        Band::Near => Tone::Orange,
        Band::Far => Tone::Warn,
    }
}

/// What a card asks the screen to do.
#[derive(Default)]
struct Act {
    open: Option<i64>,
    /// Put the leave of this schedule entry into the journal.
    book: Option<usize>,
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.pal;
    let snap = app.snap.clone();
    let mut print = false;
    top_bar(
        ui,
        |ui| {
            ui.label(ui::title("Напоминания", 17.0, p.text));
            let (urgent, early) = (snap.reminders_urgent(), snap.reminders_early());
            if urgent + early > 0 {
                ui.add_space(8.0);
                ui::pill(ui, &format!("{}", urgent + early), if urgent > 0 { Tone::Orange } else { Tone::Warn });
            }
        },
        |ui| {
            if !snap.reminders.is_empty() {
                print = ui::button(ui, Kind::Secondary, Some(icon::PRINTER), "Печать", true).clicked();
            }
        },
    );

    let mut act = Act::default();
    let mut shown: Vec<usize> = Vec::new();
    page(ui, "reminders", 1280.0, |ui| {
        let st = &mut app.f.reminders;
        let prm = &snap.params;
        ui.add(
            egui::Label::new(ui::text(
                format!(
                    "Работника нужно письменно уведомить о начале отпуска под роспись не позднее чем за {} дн. (ст. 123 ТК РФ). Сроки напоминаний задаются в «Настройках» → «Расчёт».",
                    otpuska_core::schedule::NOTICE_DAYS
                ),
                12.5,
                p.text2,
            ))
            .wrap(),
        );
        ui.add_space(10.0);

        // ---- the switch: all / 14 days / month / end of leave / not in the journal ----
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        let counts = |f: ReminderShow| snap.reminder_rows(&ReminderFilter { show: f, dept: st.filter.dept, search: st.filter.search.clone() }).len();
        let labels: Vec<(ReminderShow, String)> = ReminderShow::ALL
            .iter()
            .map(|&s| {
                let n = counts(s);
                (s, if n > 0 { format!("{} · {n}", s.label(prm)) } else { s.label(prm) })
            })
            .collect();
        ui.horizontal(|ui| {
            ui::search_input(ui, &mut st.filter.search, "Поиск по ФИО, отделу, должности", 270.0);
            ui::dept_picker(ui, "reminders-dept", &snap, &mut st.filter.dept, 220.0, "Все подразделения");
        });
        ui.horizontal(|ui| {
            let options: Vec<(ReminderShow, &str)> = labels.iter().map(|(s, l)| (*s, l.as_str())).collect();
            ui::segmented(ui, &mut st.filter.show, &options);
        });
        let rows = snap.reminder_rows(&st.filter);
        let narrowed = st.filter.is_active();
        if filters::footer(ui, narrowed, narrowed, rows.len(), snap.reminders.len(), "") {
            st.filter = ReminderFilter::default();
        }
        ui.add_space(8.0);
        shown = rows.clone();

        if rows.is_empty() {
            ui::card(ui, |ui| {
                if snap.reminders.is_empty() {
                    ui::empty_state(
                        ui,
                        icon::BELL,
                        "Пока напоминать не о чем",
                        &format!(
                            "Здесь появятся отпуска, которые начнутся в ближайшие {} дн. и {} мес., не внесённые в журнал отпуска из графика, а также окончание отпуска по беременности и родам и по уходу за ребёнком.",
                            prm.remind_near_days, prm.remind_far_months
                        ),
                    );
                } else {
                    ui::empty_state(ui, icon::FUNNEL, "Ничего не найдено", "Измените фильтры или строку поиска");
                }
            });
            return;
        }

        // ---- cards, in groups by urgency (rows are already sorted that way) ----
        let width = ui.available_width();
        let cols = (((width + GAP) / (CARD_W + GAP)).floor() as usize).max(1);
        let card_w = ((width - GAP * (cols as f32 - 1.0)) / cols as f32).max(280.0);
        let mut i = 0;
        while i < rows.len() {
            let band = snap.reminders[rows[i]].band;
            let end = rows[i..].iter().position(|&r| snap.reminders[r].band != band).map_or(rows.len(), |n| i + n);
            let title = match band {
                Band::Missed => "Не оформлено в журнале".to_string(),
                Band::Near => format!("Ближайшие {} дн.", prm.remind_near_days),
                Band::Far => format!("Дальше — до {} мес.", prm.remind_far_months),
            };
            ui.horizontal(|ui| {
                ui.label(ui::title(&title, 14.0, p.text));
                ui.label(ui::text(format!("{}", end - i), 12.0, p.text3));
            });
            ui.add_space(6.0);
            for chunk in rows[i..end].chunks(cols) {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = GAP;
                    for &r in chunk {
                        card(ui, &p, &snap, &snap.reminders[r], card_w, &mut act);
                    }
                });
                ui.add_space(GAP);
            }
            ui.add_space(6.0);
            i = end;
        }
    });

    if let Some(pi) = act.book {
        if let Some(pl) = snap.plans.get(pi) {
            let leave = Leave { employee_id: pl.plan.employee_id, start: pl.plan.start, base_days: pl.plan.base_days, extra_days: pl.plan.extra_days, note: pl.plan.note.clone(), ..Default::default() };
            if app.save_leave(&leave) {
                app.toast(true, "Отпуск внесён в журнал по графику");
            }
        }
    }
    if let Some(id) = act.open {
        app.open_employee(id);
    }
    if print {
        let html = otpuska_core::print::reminders(&snap, &shown, &app.f.reminders.filter.describe(&snap));
        app.print(html, "napominaniya");
    }
}

fn card(ui: &mut Ui, p: &Pal, snap: &Snapshot, r: &Reminder, width: f32, act: &mut Act) {
    let e = &snap.emps[r.emp_ix];
    let tone = band_tone(r.band);
    let color = tone.color(p);
    let today = snap.params.as_of;
    let resp = Frame::new()
        .fill(p.panel)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin { left: 16, right: 14, top: 12, bottom: 12 })
        .show(ui, |ui| {
            // The cards sit in a horizontal row; their own content runs top to bottom.
            ui.vertical(|ui| {
            ui.set_width(width - 32.0); // the margins and the outline add the rest
            ui.set_min_height(132.0);
            ui.spacing_mut().item_spacing.y = 4.0;
            // Name and the countdown.
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(ui::strong(&e.emp.fio, 14.0, p.text)).truncate());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui::pill(ui, &r.when(), tone);
                });
            });
            let sub = [e.dept.as_str(), e.emp.position.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
            if !sub.is_empty() {
                ui.add(egui::Label::new(ui::text(sub, 12.0, p.text3)).truncate());
            }
            ui.add_space(2.0);
            ui.label(ui::strong(r.kind_label(), 12.5, color));
            // Dates.
            let dates = match r.kind {
                ReminderKind::End(_) => format!("последний день {} · выход на работу {}", date::fmt(r.date), date::fmt_opt(r.back_to_work())),
                _ => match r.end {
                    Some(end) => format!("{} – {} · {} дн.", date::fmt(r.date), date::fmt(end), engine::fmt_days(r.leave_days)),
                    None => date::fmt(r.date),
                },
            };
            ui.add(egui::Label::new(ui::text(dates, 12.5, p.text)).wrap());
            // What to do about it.
            match r.kind {
                ReminderKind::Start | ReminderKind::PlanStart => {
                    if let Some(by) = r.notify_by() {
                        let late = by < today;
                        let text = if late {
                            format!("Уведомить под роспись нужно было до {} — срок прошёл", date::fmt(by))
                        } else {
                            format!("Уведомить работника под роспись до {}", date::fmt(by))
                        };
                        ui.add(egui::Label::new(ui::text(text, 12.0, if late { p.danger } else { p.text2 })).wrap());
                    }
                    if r.kind == ReminderKind::PlanStart {
                        ui.add(egui::Label::new(ui::text("Есть в графике, в журнале записи нет", 12.0, p.text3)).wrap());
                    } else if r.plan_ix.is_some() {
                        ui.add(egui::Label::new(ui::text("Совпадает с графиком", 12.0, p.text3)).wrap());
                    }
                }
                ReminderKind::PlanMissed => {
                    ui.add(egui::Label::new(ui::text("По графику отпуск уже должен был начаться, а в журнале его нет: внесите или перенесите в графике", 12.0, p.text2)).wrap());
                }
                ReminderKind::End(kind) => {
                    let what = if kind == LeaveType::Childcare { "по уходу за ребёнком" } else { "по беременности и родам" };
                    ui.add(egui::Label::new(ui::text(format!("Отпуск {what}. После выхода по желанию женщины ей предоставляется ежегодный отпуск (ст. 260 ТК РФ)"), 12.0, p.text3)).wrap());
                }
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if matches!(r.kind, ReminderKind::PlanStart | ReminderKind::PlanMissed) {
                    if let Some(pi) = r.plan_ix {
                        if ui::button(ui, Kind::Primary, Some(icon::BOOK_OPEN_TEXT), "Внести в журнал", true).clicked() {
                            act.book = Some(pi);
                        }
                    }
                }
                if ui::button(ui, Kind::Secondary, None, "Карточка", true).clicked() {
                    act.open = Some(e.emp.id);
                }
            });
            });
        })
        .response;
    // A colored edge on the left shows how urgent the card is.
    let edge = egui::Rect::from_min_size(resp.rect.left_top() + vec2(0.0, 10.0), vec2(3.0, resp.rect.height() - 20.0));
    ui.painter().rect_filled(edge, CornerRadius::same(2), color);
}
