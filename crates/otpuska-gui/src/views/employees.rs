//! «Сотрудники»: the directory with totals for all working years. A click opens the card.

use super::filters::{self, EmpBar};
use super::{fit, Sort};
use crate::app::{fixed_page, top_bar, App};
use crate::ui::{self, Kind};
use egui::Ui;
use egui_phosphor::regular as icon;
use otpuska_core::date;
use otpuska_core::engine::{self, EmpRow};
use otpuska_core::filter::{EmpFilter, Scope};
use otpuska_core::model::Status;
use std::cmp::Ordering;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Col {
    Order,
    Fio,
    Dept,
    Hire,
    Left,
    State,
    Deadline,
}

pub struct State {
    pub filter: EmpFilter,
    pub sort: Sort<Col>,
}

impl Default for State {
    fn default() -> Self {
        State { filter: EmpFilter::default(), sort: Sort::new(Col::Order, true) }
    }
}

fn cmp_opt<T: PartialOrd>(a: Option<T>, b: Option<T>) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn compare(a: &EmpRow, b: &EmpRow, col: Col) -> Ordering {
    match col {
        Col::Order => a.emp.id.cmp(&b.emp.id),
        Col::Fio => a.emp.fio.to_lowercase().cmp(&b.emp.fio.to_lowercase()),
        Col::Dept => a.dept.to_lowercase().cmp(&b.dept.to_lowercase()),
        Col::Hire => cmp_opt(a.emp.hire_date, b.emp.hire_date),
        Col::Left => cmp_opt(a.calc.left, b.calc.left),
        Col::State => a.calc.state.rank().cmp(&b.calc.state.rank()).then(cmp_opt(a.calc.days_left(), b.calc.days_left())),
        Col::Deadline => cmp_opt(a.calc.deadline(), b.calc.deadline()),
    }
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.pal;
    let snap = app.snap.clone();
    let (mut add, mut print) = (false, false);
    top_bar(
        ui,
        |ui| {
            ui.label(ui::title("Сотрудники", 17.0, p.text));
        },
        |ui| {
            add = ui::button(ui, Kind::Primary, Some(icon::USER_PLUS), "Новый сотрудник", true).clicked();
            if !snap.emps.is_empty() {
                ui.add_space(6.0);
                print = ui::button(ui, Kind::Secondary, Some(icon::PRINTER), "Печать", true).clicked();
            }
        },
    );
    if add {
        app.new_employee();
        return;
    }

    let mut open = None;
    let mut shown_rows: Vec<usize> = Vec::new();
    fixed_page(ui, 1400.0, |ui| {
        if snap.emps.is_empty() {
            ui::card(ui, |ui| {
                ui::empty_state(ui, icon::USERS, "Сотрудников пока нет", "Добавьте первого сотрудника или загрузите список из Excel в «Настройках»");
            });
            return;
        }
        let st = &mut app.f.employees;
        filters::emp_bar(ui, "employees", &snap, &mut st.filter, EmpBar { work: true, states: true, took: true, dismissed: true });
        let mut rows = snap.emp_rows(&st.filter, Scope::All, true);
        let sort = st.sort;
        rows.sort_by(|&a, &b| {
            let o = compare(&snap.emps[a], &snap.emps[b], sort.key).then(snap.emps[a].emp.id.cmp(&snap.emps[b].emp.id));
            if sort.asc {
                o
            } else {
                o.reverse()
            }
        });
        if filters::footer(ui, st.filter.is_active(), st.filter.is_active(), rows.len(), snap.emps.len(), "") {
            st.filter.reset();
        }
        ui.add_space(6.0);
        shown_rows = rows.clone();

        if rows.is_empty() {
            ui::card(ui, |ui| ui::empty_state(ui, icon::FUNNEL, "Никого не найдено", "Измените фильтры или строку поиска"));
            return;
        }

        // Required: №, ФИО, остаток, состояние, срок. Optional columns drop out on narrow windows.
        let avail = ui.available_width() - 4.0;
        let show = fit(avail, 52.0 + 220.0 + 90.0 + 150.0 + 104.0, &[(1, 100.0), (2, 200.0), (3, 170.0), (4, 130.0), (5, 80.0), (6, 80.0)]);
        let mut sort_click = None;
        ui::flush_card(ui, |ui| {
            let mut t = ui::table(ui, "employees").column(ui::exact(52.0)).column(ui::rest(220.0));
            if show[1] {
                t = t.column(ui::exact(200.0));
            }
            if show[2] {
                t = t.column(ui::exact(170.0));
            }
            if show[0] {
                t = t.column(ui::exact(100.0));
            }
            if show[3] {
                t = t.column(ui::exact(130.0));
            }
            if show[4] {
                t = t.column(ui::exact(80.0)).column(ui::exact(80.0));
            }
            t = t.column(ui::exact(90.0)).column(ui::exact(150.0)).column(ui::exact(104.0));
            t.header(ui::HEAD_H, |mut h| {
                if ui::head_sort(&mut h, "№", false, sort.arrow(Col::Order)) {
                    sort_click = Some(Col::Order);
                }
                if ui::head_sort(&mut h, "ФИО", false, sort.arrow(Col::Fio)) {
                    sort_click = Some(Col::Fio);
                }
                if show[1] && ui::head_sort(&mut h, "Подразделение", false, sort.arrow(Col::Dept)) {
                    sort_click = Some(Col::Dept);
                }
                if show[2] {
                    ui::head_cell(&mut h, "Должность", false);
                }
                if show[0] && ui::head_sort(&mut h, "Принят", false, sort.arrow(Col::Hire)) {
                    sort_click = Some(Col::Hire);
                }
                if show[3] {
                    ui::head_cell(&mut h, "Статус", false);
                }
                if show[4] {
                    ui::head_tip(&mut h, "Положено", true, "Всего дней отпуска за все прошедшие рабочие годы");
                    ui::head_tip(&mut h, "Отгуляно", true, "Сколько дней уже использовано (по журналу отпусков)");
                }
                if ui::head_sort(&mut h, "Остаток", true, sort.arrow(Col::Left)) {
                    sort_click = Some(Col::Left);
                }
                if ui::head_sort(&mut h, "Состояние", false, sort.arrow(Col::State)) {
                    sort_click = Some(Col::State);
                }
                if ui::head_sort(&mut h, "Срок до", false, sort.arrow(Col::Deadline)) {
                    sort_click = Some(Col::Deadline);
                }
            })
            .body(|body| {
                body.rows(ui::ROW_H, rows.len(), |mut row| {
                    let r = &snap.emps[rows[row.index()]];
                    let c = &r.calc;
                    let dim = r.emp.status == Status::Dismissed;
                    let main = if dim { p.text3 } else { p.text };
                    let second = if dim { p.text3 } else { p.text2 };
                    ui::cell_text(&mut row, r.emp.id.to_string(), p.text3);
                    let name_color = if r.emp.fio.starts_with(engine::PLACEHOLDER_PREFIX) { p.warn } else { main };
                    ui::cell_strong(&mut row, &r.emp.fio, name_color);
                    if show[1] {
                        ui::cell_text(&mut row, &r.dept, second);
                    }
                    if show[2] {
                        ui::cell_text(&mut row, &r.emp.position, second);
                    }
                    if show[0] {
                        ui::cell_text(&mut row, date::fmt_opt(r.emp.hire_date), second);
                    }
                    if show[3] {
                        let tone = match r.emp.status {
                            Status::Working => ui::Tone::Neutral,
                            Status::Suspended => ui::Tone::Violet,
                            Status::Dismissed => ui::Tone::Muted,
                        };
                        ui::cell_pill(&mut row, r.emp.status.label(), tone);
                    }
                    if show[4] {
                        ui::cell_right(&mut row, engine::fmt_days_opt(c.entitled), second);
                        ui::cell_right(&mut row, if r.leave_count > 0 || c.taken > 0.0 { engine::fmt_days(c.taken) } else { String::new() }, second);
                    }
                    ui::cell_right(&mut row, engine::fmt_days_opt(c.left), ui::days_color(&p, c.left));
                    ui::cell_state(&mut row, c.state);
                    ui::cell_text(&mut row, date::fmt_opt(c.deadline()), second);
                    if row.response().clicked() {
                        open = Some(r.emp.id);
                    }
                });
            });
        });
        if let Some(col) = sort_click {
            app.f.employees.sort.click(col);
        }
    });

    if print {
        // Exactly the rows (and the order) of the table.
        let html = otpuska_core::print::employees(&snap, &shown_rows, &app.f.employees.filter.describe(&snap, true));
        app.print(html, "sotrudniki");
    }
    if let Some(id) = open {
        app.open_employee(id);
    }
}
