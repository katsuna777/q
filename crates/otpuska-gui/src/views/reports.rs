//! «Отчёты»: who is on leave now / soon, and a summary by department.

use super::filters;
use super::journal::status_tone;
use crate::app::{page, top_bar, App};
use crate::theme::Pal;
use crate::ui::{self, Kind};
use egui::{vec2, CornerRadius, Sense, Ui};
use egui_phosphor::regular as icon;
use otpuska_core::date;
use otpuska_core::engine::{self, DeptReport};
use otpuska_core::filter::{LeaveFilter, LeaveShow};

pub struct State {
    pub filter: LeaveFilter,
}

impl Default for State {
    fn default() -> Self {
        State { filter: LeaveFilter::upcoming() }
    }
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.pal;
    let snap = app.snap.clone();
    let mut print = false;
    top_bar(
        ui,
        |ui| {
            ui.label(ui::title("Отчёты", 17.0, p.text));
        },
        |ui| {
            print = ui::button(ui, Kind::Secondary, Some(icon::PRINTER), "Печать", true).clicked();
        },
    );

    let mut open = None;
    let mut shown_leaves: Vec<usize> = Vec::new();
    page(ui, "reports", 1280.0, |ui| {
        // ---- leaves: now and upcoming by default, any status / period on demand ----
        let st = &mut app.f.reports;
        ui.label(ui::title("Отпуска", 15.0, p.text));
        ui.add_space(8.0);
        filters::leave_bar(ui, "reports", &snap, &mut st.filter, &[LeaveShow::Active, LeaveShow::All, LeaveShow::Done], false, false);
        let mut upcoming = snap.leave_rows(&st.filter);
        // Chronologically; undated records last.
        upcoming.sort_by_key(|&i| (snap.leaves[i].leave.start.is_none(), snap.leaves[i].leave.start, snap.leaves[i].leave.id));
        let active = st.filter.is_active(LeaveShow::Active, snap.params.as_of);
        if filters::footer(ui, true, active, upcoming.len(), snap.leaves.len(), "По умолчанию — идущие сейчас и запланированные отпуска") {
            st.filter = LeaveFilter::upcoming();
        }
        ui.add_space(6.0);
        shown_leaves = upcoming.clone();
        if upcoming.is_empty() {
            ui::card(ui, |ui| {
                ui.label(ui::text("По выбранным фильтрам отпусков нет.", 13.0, p.text2));
            });
        } else {
            ui::flush_card(ui, |ui| {
                ui::table(ui, "reports-upcoming")
                    .vscroll(false)
                    .column(ui::exact(44.0))
                    .column(ui::rest(170.0))
                    .column(ui::exact(200.0))
                    .column(ui::exact(150.0))
                    .column(ui::exact(120.0))
                    .column(ui::exact(60.0))
                    .column(ui::exact(170.0))
                    .column(ui::exact(140.0))
                    .header(ui::HEAD_H, |mut h| {
                        ui::head_cell(&mut h, "№", false);
                        ui::head_cell(&mut h, "ФИО", false);
                        ui::head_cell(&mut h, "Подразделение", false);
                        ui::head_cell(&mut h, "Вид", false);
                        ui::head_cell(&mut h, "Статус", false);
                        ui::head_cell(&mut h, "Дней", true);
                        ui::head_cell(&mut h, "Даты отпуска", false);
                        ui::head_cell(&mut h, "Примечание", false);
                    })
                    .body(|body| {
                        body.rows(ui::ROW_H, upcoming.len(), |mut row| {
                            let n = row.index();
                            let l = &snap.leaves[upcoming[n]];
                            ui::cell_text(&mut row, (n + 1).to_string(), p.text3);
                            ui::cell_strong(&mut row, &l.fio, p.text);
                            ui::cell_text(&mut row, &l.dept, p.text2);
                            if l.leave.kind.is_paid() {
                                ui::cell_text(&mut row, l.leave.kind.short(), p.text2);
                            } else {
                                ui::cell_pill(&mut row, l.leave.kind.short(), super::journal::kind_tone(l.leave.kind));
                            }
                            match l.status {
                                Some(s) => {
                                    ui::cell_pill(&mut row, s.label(), status_tone(s));
                                }
                                None => {
                                    ui::cell_text(&mut row, "", p.text2);
                                }
                            }
                            ui::cell_right(&mut row, engine::fmt_days(l.leave.length()), p.text);
                            let dates = match (l.leave.start, l.end) {
                                (Some(s), Some(e)) => format!("{} – {}", date::fmt(s), date::fmt(e)),
                                _ => String::new(),
                            };
                            ui::cell_text(&mut row, dates, p.text2);
                            ui::cell_text(&mut row, &l.leave.note, p.text3);
                            if row.response().clicked() {
                                open = l.emp_ix.map(|i| snap.emps[i].emp.id);
                            }
                        });
                    });
            });
        }

        // ---- by department ----
        ui.add_space(24.0);
        let (depts, total) = snap.dept_report(app.f.reports.filter.dept);
        ui.horizontal(|ui| {
            ui.label(ui::title("По подразделениям", 15.0, p.text));
            ui.label(ui::text("действующие сотрудники", 12.0, p.text3));
        });
        ui.add_space(8.0);
        if depts.is_empty() {
            ui::card(ui, |ui| {
                ui.label(ui::text("Подразделения ещё не добавлены — список ведётся в «Настройках».", 13.0, p.text2));
            });
            return;
        }
        ui::flush_card(ui, |ui| {
            ui::table(ui, "reports-depts")
                .vscroll(false)
                .sense(Sense::hover())
                .column(ui::rest(190.0))
                .column(ui::exact(110.0))
                .column(ui::exact(100.0))
                .column(ui::exact(80.0))
                .column(ui::exact(72.0))
                .column(ui::exact(60.0))
                .column(ui::exact(60.0))
                .column(ui::exact(84.0))
                .column(ui::exact(130.0))
                .header(ui::HEAD_H + 12.0, |mut h| {
                    ui::head_cell(&mut h, "Подразделение", false);
                    ui::head_cell(&mut h, "Внимание", true);
                    ui::head_cell(&mut h, "Дней к исп.", true);
                    ui::head_cell(&mut h, "Работает", true);
                    ui::head_cell(&mut h, "Просроч.", true);
                    ui::head_cell(&mut h, "Горит", true);
                    ui::head_cell(&mut h, "Скоро", true);
                    ui::head_cell(&mut h, "Приост.", true);
                    ui::head_cell(&mut h, "Состояние", false);
                })
                .body(|mut body| {
                    let all: Vec<&DeptReport> = std::iter::once(&total).chain(depts.iter()).collect();
                    for (i, d) in all.iter().enumerate() {
                        body.row(ui::ROW_H, |mut row| {
                            let strong = i == 0;
                            let color = if strong { p.text } else { p.text2 };
                            if strong {
                                ui::cell_strong(&mut row, &d.name, p.text);
                            } else {
                                ui::cell_text(&mut row, &d.name, p.text);
                            }
                            ui::cell_right(&mut row, d.attention.to_string(), if d.attention > 0 { p.warn } else { p.text3 });
                            ui::cell_right(&mut row, engine::fmt_days(d.days_left), color);
                            ui::cell_right(&mut row, d.working.to_string(), color);
                            ui::cell_right(&mut row, d.overdue.to_string(), if d.overdue > 0 { p.danger } else { p.text3 });
                            ui::cell_right(&mut row, d.burning.to_string(), if d.burning > 0 { p.orange } else { p.text3 });
                            ui::cell_right(&mut row, d.soon.to_string(), if d.soon > 0 { p.warn } else { p.text3 });
                            ui::cell_right(&mut row, d.suspended.to_string(), if d.suspended > 0 { p.violet } else { p.text3 });
                            ui::cell(&mut row, |ui| stacked_bar(ui, &p, d));
                        });
                    }
                });
        });
    });

    if print {
        // The same leaves as in the table, and the department table for the chosen department.
        let f = &app.f.reports.filter;
        let html = otpuska_core::print::reports(&snap, &shown_leaves, f.dept, &f.describe(&snap, LeaveShow::Active));
        app.print(html, "otchety");
    }
    if let Some(id) = open {
        app.open_employee(id);
    }
}

/// Red/orange/yellow share of the people who work in the department, the rest in green.
fn stacked_bar(ui: &mut Ui, p: &Pal, d: &DeptReport) {
    let total = (d.working + d.suspended).max(1) as f32;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width() - 14.0, 8.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(4), p.sunken);
    if d.working + d.suspended == 0 {
        return;
    }
    let ok = (d.working + d.suspended).saturating_sub(d.overdue + d.burning + d.soon + d.suspended);
    let segs = [(d.overdue, p.danger), (d.burning, p.orange), (d.soon, p.warn), (ok, p.ok), (d.suspended, p.violet)];
    let painter = ui.painter_at(rect);
    let mut x = rect.left();
    for (n, color) in segs {
        if n == 0 {
            continue;
        }
        let w = rect.width() * n as f32 / total;
        painter.rect_filled(egui::Rect::from_min_size(egui::pos2(x, rect.top()), vec2(w, rect.height())), CornerRadius::ZERO, color);
        x += w;
    }
}
