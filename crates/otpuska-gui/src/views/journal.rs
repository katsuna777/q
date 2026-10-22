//! «Журнал отпусков»: one row per leave (or part of it), with a form on top.

use super::filters;
use super::{fit, leave_form, FormEvent, LeaveDraft, Sort};
use crate::app::{fixed_page, top_bar, App};
use crate::ui::{self, Kind, Tone};
use egui::Ui;
use egui_phosphor::regular as icon;
use otpuska_core::date;
use otpuska_core::engine::{self, LeaveRow, LeaveStatus};
use otpuska_core::filter::{LeaveFilter, LeaveShow};
use std::cmp::Ordering;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Col {
    Order,
    Fio,
    Start,
    Status,
}

pub struct State {
    pub draft: LeaveDraft,
    pub filter: LeaveFilter,
    pub sort: Sort<Col>,
}

impl Default for State {
    fn default() -> Self {
        State { draft: LeaveDraft::default(), filter: LeaveFilter::default(), sort: Sort::new(Col::Order, false) }
    }
}

fn compare(a: &LeaveRow, b: &LeaveRow, col: Col) -> Ordering {
    match col {
        Col::Order => a.leave.id.cmp(&b.leave.id),
        Col::Fio => a.fio.to_lowercase().cmp(&b.fio.to_lowercase()),
        Col::Start => match (a.leave.start, b.leave.start) {
            (Some(x), Some(y)) => x.cmp(&y),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        },
        Col::Status => {
            let rank = |s: Option<LeaveStatus>| match s {
                Some(LeaveStatus::InLeave) => 0,
                Some(LeaveStatus::Planned) => 1,
                Some(LeaveStatus::Done) => 2,
                Some(LeaveStatus::Undated) => 3,
                None => 4,
            };
            rank(a.status).cmp(&rank(b.status))
        }
    }
}

pub fn status_tone(s: LeaveStatus) -> Tone {
    match s {
        LeaveStatus::InLeave => Tone::Ok,
        LeaveStatus::Planned => Tone::Info,
        LeaveStatus::Done => Tone::Neutral,
        LeaveStatus::Undated => Tone::Muted,
    }
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.pal;
    let snap = app.snap.clone();
    let mut print = false;
    top_bar(
        ui,
        |ui| {
            ui.label(ui::title("Журнал отпусков", 17.0, p.text));
        },
        |ui| {
            if !snap.leaves.is_empty() {
                print = ui::button(ui, Kind::Secondary, Some(icon::PRINTER), "Печать", true).clicked();
            }
        },
    );

    let (mut submit, mut cancel) = (false, false);
    let mut edit: Option<i64> = None;
    let mut delete: Option<i64> = None;
    let mut open_emp: Option<i64> = None;
    let mut sort_click = None;
    let mut shown_rows: Vec<usize> = Vec::new();

    fixed_page(ui, 1400.0, |ui| {
        if snap.emps.is_empty() {
            ui::card(ui, |ui| ui::empty_state(ui, icon::BOOK_OPEN_TEXT, "Сначала добавьте сотрудников", "Отпуска вносятся на сотрудников — список создаётся на вкладке «Сотрудники»"));
            return;
        }
        ui::card(ui, |ui| {
            match leave_form(ui, &snap, &mut app.f.journal.draft, true, "journal") {
                FormEvent::Submit => submit = true,
                FormEvent::Cancel => cancel = true,
                FormEvent::None => {}
            }
            // After a save the employee stays selected for quick consecutive entry.
        });
        ui.add_space(10.0);

        let st = &mut app.f.journal;
        filters::leave_bar(
            ui,
            "journal",
            &snap,
            &mut st.filter,
            &[LeaveShow::All, LeaveShow::InLeave, LeaveShow::Planned, LeaveShow::Done, LeaveShow::Undated],
            true,
            true,
        );
        let mut rows = snap.leave_rows(&st.filter);
        let sort = st.sort;
        rows.sort_by(|&a, &b| {
            let o = compare(&snap.leaves[a], &snap.leaves[b], sort.key).then(snap.leaves[a].leave.id.cmp(&snap.leaves[b].leave.id));
            if sort.asc {
                o
            } else {
                o.reverse()
            }
        });
        if {
            let active = st.filter.is_active(LeaveShow::All, snap.params.as_of);
            filters::footer(ui, active, active, rows.len(), snap.leaves.len(), "")
        } {
            st.filter = LeaveFilter::default();
        }
        ui.add_space(6.0);
        shown_rows = rows.clone();

        if rows.is_empty() {
            ui::card(ui, |ui| {
                if snap.leaves.is_empty() {
                    ui::empty_state(ui, icon::BOOK_OPEN_TEXT, "Журнал пуст", "Внесите первый отпуск в форме выше. «14+14» — это две записи.");
                } else {
                    ui::empty_state(ui, icon::FUNNEL, "Ничего не найдено", "Измените фильтры или строку поиска");
                }
            });
            return;
        }

        let avail = ui.available_width() - 4.0;
        let show = fit(avail, 56.0 + 220.0 + 100.0 + 120.0 + 70.0 + 76.0, &[(1, 100.0), (2, 80.0), (3, 210.0), (4, 100.0), (5, 190.0)]);
        let editing = app.f.journal.draft.id;
        ui::flush_card(ui, |ui| {
            let mut t = ui::table(ui, "journal").column(ui::exact(56.0)).column(ui::rest(220.0));
            if show[3] {
                t = t.column(ui::exact(210.0));
            }
            t = t.column(ui::exact(100.0));
            if show[0] {
                t = t.column(ui::exact(100.0));
            }
            t = t.column(ui::exact(80.0));
            if show[1] {
                t = t.column(ui::exact(80.0));
            }
            t = t.column(ui::exact(120.0));
            if show[2] {
                t = t.column(ui::exact(100.0));
            }
            if show[4] {
                t = t.column(ui::exact(190.0));
            }
            t.column(ui::exact(76.0))
                .header(ui::HEAD_H, |mut h| {
                    if ui::head_sort(&mut h, "№", false, sort.arrow(Col::Order)) {
                        sort_click = Some(Col::Order);
                    }
                    if ui::head_sort(&mut h, "Сотрудник", false, sort.arrow(Col::Fio)) {
                        sort_click = Some(Col::Fio);
                    }
                    if show[3] {
                        ui::head_cell(&mut h, "Подразделение", false);
                    }
                    if ui::head_sort(&mut h, "Начало", false, sort.arrow(Col::Start)) {
                        sort_click = Some(Col::Start);
                    }
                    if show[0] {
                        ui::head_cell(&mut h, "Окончание", false);
                    }
                    ui::head_cell(&mut h, "Основной", true);
                    if show[1] {
                        ui::head_cell(&mut h, "Доп.", true);
                    }
                    if ui::head_sort(&mut h, "Статус", false, sort.arrow(Col::Status)) {
                        sort_click = Some(Col::Status);
                    }
                    if show[2] {
                        ui::head_cell(&mut h, "Остаток", true);
                    }
                    if show[4] {
                        ui::head_cell(&mut h, "Примечание", false);
                    }
                    ui::head_cell(&mut h, "", true);
                })
                .body(|body| {
                    body.rows(ui::ROW_H, rows.len(), |mut row| {
                        let l = &snap.leaves[rows[row.index()]];
                        let selected = l.leave.id == editing;
                        row.set_selected(selected);
                        ui::cell_text(&mut row, l.leave.id.to_string(), p.text3);
                        let name = ui::cell_strong(&mut row, if l.fio.is_empty() { "— сотрудник удалён —" } else { &l.fio }, p.text);
                        if name.clicked() {
                            if let Some(i) = l.emp_ix {
                                open_emp = Some(snap.emps[i].emp.id);
                            }
                        }
                        if show[3] {
                            ui::cell_text(&mut row, &l.dept, p.text2);
                        }
                        ui::cell_text(&mut row, date::fmt_opt(l.leave.start), p.text2);
                        if show[0] {
                            ui::cell_text(&mut row, date::fmt_opt(l.end), p.text2);
                        }
                        ui::cell_right(&mut row, if l.leave.base_days != 0.0 { engine::fmt_days(l.leave.base_days) } else { String::new() }, p.text);
                        if show[1] {
                            ui::cell_right(&mut row, if l.leave.extra_days != 0.0 { engine::fmt_days(l.leave.extra_days) } else { String::new() }, p.text);
                        }
                        match l.status {
                            Some(s) => {
                                ui::cell_pill(&mut row, s.label(), status_tone(s));
                            }
                            None => {
                                ui::cell_pill(&mut row, "нет дней", Tone::Danger);
                            }
                        }
                        if show[2] {
                            ui::cell_right(&mut row, engine::fmt_days_opt(l.emp_left), ui::days_color(&p, l.emp_left));
                        }
                        if show[4] {
                            ui::cell_text(&mut row, &l.leave.note, p.text3);
                        }
                        ui::cell(&mut row, |ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            if ui::icon_button(ui, icon::PENCIL_SIMPLE, "Изменить", true).clicked() {
                                edit = Some(l.leave.id);
                            }
                            if ui::icon_button(ui, icon::TRASH, "Удалить запись", true).clicked() {
                                delete = Some(l.leave.id);
                            }
                        });
                    });
                });
        });
    });
    if let Some(col) = sort_click {
        app.f.journal.sort.click(col);
    }

    if let Some(id) = edit {
        if let Some(l) = snap.leaves.iter().find(|l| l.leave.id == id) {
            app.f.journal.draft = LeaveDraft::from_leave(&l.leave);
        }
    }
    if let Some(id) = delete {
        let key = format!("delete-leave-{id}");
        let armed = app.armed(ui, &key);
        app.confirm(ui, &key);
        if armed {
            app.delete_leave(id);
            if app.f.journal.draft.id == id {
                app.f.journal.draft = LeaveDraft::for_employee(app.f.journal.draft.employee);
            }
        } else {
            app.toast(true, "Нажмите на корзину ещё раз, чтобы удалить запись");
        }
    }
    if cancel {
        let emp = app.f.journal.draft.employee;
        app.f.journal.draft = LeaveDraft::for_employee(emp);
    }
    if submit {
        if let Some(l) = app.f.journal.draft.to_leave() {
            let editing = app.f.journal.draft.is_edit();
            let emp = app.f.journal.draft.employee;
            if app.save_leave(&l) {
                let left = emp.and_then(|id| app.snap.emp_by_id(id)).and_then(|r| r.calc.left);
                let tail = left.map(|v| format!(" · остаток {} дн.", engine::fmt_days(v))).unwrap_or_default();
                app.toast(true, format!("{}{tail}", if editing { "Запись изменена" } else { "Отпуск добавлен" }));
                app.f.journal.draft = LeaveDraft::for_employee(emp);
            }
        }
    }
    if let Some(id) = open_emp {
        app.open_employee(id);
    }
    if print {
        // Exactly the rows (and the order) of the table.
        let html = otpuska_core::print::journal(&snap, &shown_rows, &app.f.journal.filter.describe(&snap, LeaveShow::All));
        app.print(html, "zhurnal");
    }
}
