//! «График отпусков»: who is planned to go on leave and when (ст. 123 ТК РФ), checked against
//! the journal — what is booked, going, taken, and what was planned but never put into the journal.

use super::filters;
use super::{plan_form, FormEvent, PlanDraft, Sort};
use crate::app::{fixed_page, top_bar, App};
use crate::ui::{self, Kind, Tone};
use chrono::Datelike;
use egui::Ui;
use egui_phosphor::regular as icon;
use otpuska_core::date;
use otpuska_core::engine;
use otpuska_core::filter::{PlanFilter, PlanShow};
use otpuska_core::model::{Leave, Status};
use otpuska_core::schedule::{PlanRow, PlanStatus};
use std::cmp::Ordering;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Col {
    Fio,
    Start,
    Status,
}

pub struct State {
    pub draft: PlanDraft,
    pub filter: PlanFilter,
    pub sort: Sort<Col>,
}

impl Default for State {
    fn default() -> Self {
        State { draft: PlanDraft::default(), filter: PlanFilter { year: date::today().year(), ..Default::default() }, sort: Sort::new(Col::Start, true) }
    }
}

fn compare(a: &PlanRow, b: &PlanRow, col: Col) -> Ordering {
    match col {
        Col::Fio => a.fio.to_lowercase().cmp(&b.fio.to_lowercase()),
        Col::Start => a.plan.start.cmp(&b.plan.start),
        Col::Status => PlanStatus::ALL.iter().position(|s| *s == a.status).cmp(&PlanStatus::ALL.iter().position(|s| *s == b.status)),
    }
}

pub fn plan_tone(s: PlanStatus) -> Tone {
    match s {
        PlanStatus::Ahead => Tone::Neutral,
        PlanStatus::Booked => Tone::Info,
        PlanStatus::InLeave => Tone::Ok,
        PlanStatus::Done => Tone::Muted,
        PlanStatus::Missed => Tone::Danger,
    }
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.pal;
    let snap = app.snap.clone();
    let mut print = false;
    top_bar(
        ui,
        |ui| {
            ui.label(ui::title("График отпусков", 17.0, p.text));
        },
        |ui| {
            if !snap.plans.is_empty() {
                print = ui::button(ui, Kind::Secondary, Some(icon::PRINTER), "Печать", true).clicked();
            }
        },
    );

    let (mut submit, mut cancel) = (false, false);
    let mut edit: Option<i64> = None;
    let mut delete: Option<i64> = None;
    let mut to_journal: Option<i64> = None;
    let mut open_emp: Option<i64> = None;
    let mut sort_click = None;
    let mut shown_rows: Vec<usize> = Vec::new();

    fixed_page(ui, 1400.0, |ui| {
        if snap.emps.is_empty() {
            ui::card(ui, |ui| ui::empty_state(ui, icon::CALENDAR_DOTS, "Сначала добавьте сотрудников", "График составляется на сотрудников — список создаётся на вкладке «Сотрудники»"));
            return;
        }
        let st = &mut app.f.schedule;

        // ---- year ----
        ui.horizontal(|ui| {
            if ui::icon_button(ui, icon::CARET_LEFT, "Предыдущий год", st.filter.year != 0).clicked() {
                st.filter.year -= 1;
            }
            let label = if st.filter.year == 0 { "Все годы".to_string() } else { format!("{} год", st.filter.year) };
            ui.label(ui::strong(label, 14.0, p.text));
            if ui::icon_button(ui, icon::CARET_RIGHT, "Следующий год", st.filter.year != 0).clicked() {
                st.filter.year += 1;
            }
            let this_year = snap.params.as_of.year();
            if st.filter.year != this_year && ui::link(ui, None, "текущий год").clicked() {
                st.filter.year = this_year;
            }
            if st.filter.year != 0 && ui::link(ui, None, "все годы").clicked() {
                st.filter.year = 0;
            }
            ui.add_space(10.0);
            ui.label(ui::text(
                "График обязателен и для работодателя, и для работника; утверждается не позднее чем за 2 недели до начала года (ст. 123 ТК РФ).",
                12.0,
                p.text3,
            ));
        });
        ui.add_space(8.0);

        // ---- tiles: one per status, plus the people not in the schedule ----
        let year_rows = snap.plan_rows(&PlanFilter { show: PlanShow::All, search: String::new(), dept: st.filter.dept, year: st.filter.year }, false);
        let count = |s: PlanStatus| year_rows.iter().filter(|&&i| snap.plans[i].status == s).count();
        let without: Vec<&str> = snap
            .emps
            .iter()
            .filter(|r| r.emp.status == Status::Working && r.emp.hire_date.is_some())
            .filter(|r| st.filter.dept.is_none() || r.emp.department_id == st.filter.dept)
            .filter(|r| !snap.plans.iter().any(|pl| pl.plan.employee_id == r.emp.id && (st.filter.year == 0 || pl.plan.start.is_some_and(|s| s.year() == st.filter.year))))
            .map(|r| r.emp.fio.as_str())
            .collect();
        ui.columns(6, |c| {
            for (i, s) in PlanStatus::ALL.iter().enumerate() {
                let active = st.filter.show == PlanShow::Only(*s);
                let sub = match s {
                    PlanStatus::Ahead => "ещё впереди",
                    PlanStatus::Booked => "запись в журнале",
                    PlanStatus::InLeave => "идёт сейчас",
                    PlanStatus::Done => "отпуск взят",
                    PlanStatus::Missed => "нет в журнале",
                };
                if ui::kpi(&mut c[i], &s.label().to_uppercase(), &count(*s).to_string(), sub, plan_tone(*s), active).on_hover_text(s.hint()).clicked() {
                    st.filter.show = if active { PlanShow::All } else { PlanShow::Only(*s) };
                }
            }
            let tip = if without.is_empty() { "У всех работающих сотрудников есть отпуск в графике".to_string() } else { format!("Нет в графике:\n{}", without.iter().take(25).copied().collect::<Vec<_>>().join("\n")) };
            ui::kpi(&mut c[5], "БЕЗ ГРАФИКА", &without.len().to_string(), "нет в графике", if without.is_empty() { Tone::Ok } else { Tone::Warn }, false).on_hover_text(tip);
        });
        ui.add_space(10.0);

        // ---- form ----
        ui::card(ui, |ui| {
            match plan_form(ui, &snap, &mut st.draft, "schedule") {
                FormEvent::Submit => submit = true,
                FormEvent::Cancel => cancel = true,
                FormEvent::None => {}
            }
        });
        ui.add_space(10.0);

        // ---- filter bar ----
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
        ui.horizontal(|ui| {
            ui::search_input(ui, &mut st.filter.search, "Поиск по ФИО, отделу, примечанию", 270.0);
            ui::dept_picker(ui, "schedule-dept", &snap, &mut st.filter.dept, 220.0, "Все подразделения");
            let mut options: Vec<(PlanShow, &str)> = vec![(PlanShow::All, PlanShow::All.label())];
            options.extend(PlanStatus::ALL.iter().map(|s| (PlanShow::Only(*s), s.label())));
            ui::segmented(ui, &mut st.filter.show, &options);
        });
        let mut rows = snap.plan_rows(&st.filter, true);
        let sort = st.sort;
        rows.sort_by(|&a, &b| {
            let o = compare(&snap.plans[a], &snap.plans[b], sort.key).then(snap.plans[a].plan.start.cmp(&snap.plans[b].plan.start)).then(snap.plans[a].plan.id.cmp(&snap.plans[b].plan.id));
            if sort.asc {
                o
            } else {
                o.reverse()
            }
        });
        let in_year = snap.plan_rows(&PlanFilter { year: st.filter.year, ..Default::default() }, false).len();
        let narrowed = st.filter.is_active();
        if filters::footer(ui, narrowed, narrowed, rows.len(), in_year, "") {
            st.filter.search.clear();
            st.filter.dept = None;
            st.filter.show = PlanShow::All;
        }
        ui.add_space(6.0);
        shown_rows = rows.clone();

        if rows.is_empty() {
            ui::card(ui, |ui| {
                if snap.plans.is_empty() {
                    ui::empty_state(ui, icon::CALENDAR_DOTS, "График пуст", "Выберите сотрудника, дату начала и число дней в форме выше. Программа сама сверит график с журналом и напомнит о ближайших отпусках.");
                } else {
                    ui::empty_state(ui, icon::FUNNEL, "Ничего не найдено", "Измените год, фильтры или строку поиска");
                }
            });
            return;
        }

        let avail = ui.available_width() - 24.0;
        let show = super::fit(avail, 56.0 + 220.0 + 100.0 + 100.0 + 70.0 + 130.0 + 112.0, &[(2, 200.0), (1, 190.0), (3, 180.0)]);
        let editing = app.f.schedule.draft.id;
        ui::flush_card(ui, |ui| {
            let mut t = ui::table(ui, "schedule").column(ui::exact(56.0)).column(ui::rest(220.0));
            if show[0] {
                t = t.column(ui::exact(200.0));
            }
            t = t.column(ui::exact(100.0)).column(ui::exact(100.0)).column(ui::exact(70.0)).column(ui::exact(130.0));
            if show[1] {
                t = t.column(ui::exact(190.0));
            }
            if show[2] {
                t = t.column(ui::exact(180.0));
            }
            t.column(ui::exact(112.0))
                .header(ui::HEAD_H, |mut h| {
                    ui::head_cell(&mut h, "№", false);
                    if ui::head_sort(&mut h, "Сотрудник", false, sort.arrow(Col::Fio)) {
                        sort_click = Some(Col::Fio);
                    }
                    if show[0] {
                        ui::head_cell(&mut h, "Подразделение", false);
                    }
                    if ui::head_sort(&mut h, "Начало", false, sort.arrow(Col::Start)) {
                        sort_click = Some(Col::Start);
                    }
                    ui::head_cell(&mut h, "Окончание", false);
                    ui::head_cell(&mut h, "Дней", true);
                    if ui::head_sort(&mut h, "Статус", false, sort.arrow(Col::Status)) {
                        sort_click = Some(Col::Status);
                    }
                    if show[1] {
                        ui::head_tip(&mut h, "По журналу", false, "Какая запись журнала выполняет эту строку графика");
                    }
                    if show[2] {
                        ui::head_cell(&mut h, "Примечание", false);
                    }
                    ui::head_cell(&mut h, "", true);
                })
                .body(|body| {
                    body.rows(ui::ROW_H, rows.len(), |mut row| {
                        let pl = &snap.plans[rows[row.index()]];
                        row.set_selected(pl.plan.id == editing);
                        let n = row.index() + 1;
                        ui::cell_text(&mut row, n.to_string(), p.text3);
                        let name = ui::cell_strong(&mut row, if pl.fio.is_empty() { "— сотрудник удалён —" } else { &pl.fio }, p.text);
                        if name.clicked() {
                            open_emp = pl.emp_ix.map(|i| snap.emps[i].emp.id);
                        }
                        if show[0] {
                            ui::cell_text(&mut row, &pl.dept, p.text2);
                        }
                        ui::cell_text(&mut row, date::fmt_opt(pl.plan.start), p.text2);
                        ui::cell_text(&mut row, date::fmt_opt(pl.end), p.text2);
                        ui::cell_right(&mut row, engine::fmt_days(pl.plan.total_days()), p.text);
                        let status = ui::cell_pill(&mut row, pl.status.label(), plan_tone(pl.status));
                        status.on_hover_text(pl.status.hint());
                        if show[1] {
                            let fact = pl.fact.map(|li| &snap.leaves[li]);
                            let text = match fact.and_then(|l| Some((l.leave.start?, l.end?))) {
                                Some((s, e)) => format!("{} – {}", date::fmt(s), date::fmt(e)),
                                None => String::new(),
                            };
                            // A different start than planned is worth noticing.
                            let moved = fact.is_some_and(|l| l.leave.start != pl.plan.start);
                            ui::cell_text(&mut row, text, if moved { p.warn } else { p.text2 });
                        }
                        if show[2] {
                            ui::cell_text(&mut row, &pl.plan.note, p.text3);
                        }
                        ui::cell(&mut row, |ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            let can_book = pl.fact.is_none() && pl.emp_ix.is_some();
                            if ui::icon_button(ui, icon::BOOK_OPEN_TEXT, if can_book { "Внести в журнал" } else { "Уже есть в журнале" }, can_book).clicked() {
                                to_journal = Some(pl.plan.id);
                            }
                            if ui::icon_button(ui, icon::PENCIL_SIMPLE, "Изменить", true).clicked() {
                                edit = Some(pl.plan.id);
                            }
                            if ui::icon_button(ui, icon::TRASH, "Удалить из графика", true).clicked() {
                                delete = Some(pl.plan.id);
                            }
                        });
                    });
                });
        });
    });
    if let Some(col) = sort_click {
        app.f.schedule.sort.click(col);
    }

    if let Some(id) = edit {
        if let Some(pl) = snap.plans.iter().find(|p| p.plan.id == id) {
            app.f.schedule.draft = PlanDraft::from_plan(&pl.plan);
        }
    }
    if let Some(id) = delete {
        let key = format!("delete-plan-{id}");
        let armed = app.armed(ui, &key);
        app.confirm(ui, &key);
        if armed {
            app.delete_plan(id);
            if app.f.schedule.draft.id == id {
                app.f.schedule.draft = PlanDraft::default();
            }
        } else {
            app.toast(true, "Нажмите на корзину ещё раз, чтобы убрать запись из графика");
        }
    }
    if cancel {
        app.f.schedule.draft = PlanDraft { employee: app.f.schedule.draft.employee, ..Default::default() };
    }
    if submit {
        if let Some(pl) = app.f.schedule.draft.to_plan() {
            let editing = app.f.schedule.draft.is_edit();
            let emp = app.f.schedule.draft.employee;
            let year = pl.start.map(|s| s.year());
            if app.save_plan(&pl) {
                app.toast(true, if editing { "Запись графика изменена" } else { "Отпуск добавлен в график" });
                // Show the year of the entry that was just made, and stay on the employee for quick entry.
                if let (Some(y), true) = (year, app.f.schedule.filter.year != 0) {
                    app.f.schedule.filter.year = y;
                }
                app.f.schedule.draft = PlanDraft { employee: emp, ..Default::default() };
            }
        }
    }
    if let Some(id) = to_journal {
        if let Some(pl) = snap.plans.iter().find(|p| p.plan.id == id) {
            let leave = Leave { employee_id: pl.plan.employee_id, start: pl.plan.start, base_days: pl.plan.base_days, extra_days: pl.plan.extra_days, note: pl.plan.note.clone(), ..Default::default() };
            if app.save_leave(&leave) {
                app.toast(true, "Отпуск внесён в журнал по графику");
            }
        }
    }
    if let Some(id) = open_emp {
        app.open_employee(id);
    }
    if print {
        let f = &app.f.schedule.filter;
        let html = otpuska_core::print::schedule(&snap, &shown_rows, f.year, &f.describe(&snap));
        app.print(html, "grafik");
    }
}
