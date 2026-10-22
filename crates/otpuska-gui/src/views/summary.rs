//! «Сводка»: every active employee, the most urgent first.

use super::filters::{self, EmpBar};
use super::fit;
use crate::app::{fixed_page, top_bar, App, Route};
use crate::ui::{self, Tone};
use egui::Ui;
use egui_phosphor::regular as icon;
use otpuska_core::date;
use otpuska_core::engine::{self, State as St};
use otpuska_core::filter::{EmpFilter, Scope};
use otpuska_core::model::Status;

/// The summary tiles; a click adds or removes the states of the tile in the filter.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    Overdue,
    Burning,
    Soon,
    Ok,
    Suspended,
}

impl Group {
    fn states(self) -> &'static [St] {
        match self {
            Group::Overdue => &[St::Overdue],
            Group::Burning => &[St::Burning],
            Group::Soon => &[St::Soon],
            Group::Ok => &[St::Normal, St::AllUsed],
            Group::Suspended => &[St::Suspended],
        }
    }

    fn label(self) -> &'static str {
        match self {
            Group::Overdue => "Просрочено",
            Group::Burning => "Горит",
            Group::Soon => "Скоро",
            Group::Ok => "Всё в порядке",
            Group::Suspended => "Приостановка ТД",
        }
    }

    fn active(self, f: &EmpFilter) -> bool {
        self.states().iter().all(|s| f.states.contains(s))
    }

    fn toggle(self, f: &mut EmpFilter) {
        if self.active(f) {
            f.states.retain(|s| !self.states().contains(s));
        } else {
            for s in self.states() {
                if !f.states.contains(s) {
                    f.states.push(*s);
                }
            }
        }
    }
}

#[derive(Default)]
pub struct State {
    pub filter: EmpFilter,
}

/// "Как начать": three numbered steps for an empty database.
fn steps(ui: &mut Ui, p: &crate::theme::Pal) {
    let items = [
        ("1", "Подразделения", "«Настройки» → «Подразделения»"),
        ("2", "Сотрудники", "ФИО, подразделение, дата приема"),
        ("3", "Журнал отпусков", "кто, с какого числа, сколько дней"),
    ];
    ui.horizontal(|ui| {
        let w = 3.0 * 218.0 + 2.0 * 10.0;
        ui.add_space((ui.available_width() - w).max(0.0) / 2.0);
        ui.spacing_mut().item_spacing.x = 10.0;
        for (n, title, sub) in items {
            ui::sunken_card(ui, 218.0, |ui| {
                ui.set_min_height(44.0);
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
                    ui.painter().circle_filled(r.center(), 12.0, p.accent.gamma_multiply(0.2));
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, n, egui::FontId::new(12.5, crate::theme::semibold()), p.text);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.label(ui::strong(title, 13.0, p.text));
                        ui.label(ui::text(sub, 11.5, p.text3));
                    });
                });
            });
        }
    });
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.pal;
    let snap = app.snap.clone();
    let (mut print, mut add, mut import) = (false, false, false);
    top_bar(
        ui,
        |ui| {
            ui.label(ui::title("Сводка", 17.0, p.text));
        },
        |ui| {
            if !snap.emps.is_empty() {
                print = ui::button(ui, ui::Kind::Secondary, Some(icon::PRINTER), "Печать", true).clicked();
            }
        },
    );

    if snap.emps.is_empty() {
        fixed_page(ui, 1280.0, |ui| {
            ui::card(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(34.0);
                    ui.label(ui::icon_text(icon::SUN_HORIZON, 46.0, p.text3));
                    ui.add_space(10.0);
                    ui.label(ui::title("Здесь появится сводка по отпускам", 19.0, p.text));
                    ui.add_space(4.0);
                    ui.label(ui::text(
                        "Добавьте сотрудников и вносите отпуска в журнал — программа сама считает остатки,\nкрайние сроки и подсвечивает «горящие» отпуска. Данные хранятся в локальной базе.",
                        13.0,
                        p.text2,
                    ));
                    ui.add_space(16.0);
                    steps(ui, &p);
                    ui.add_space(18.0);
                    ui.horizontal(|ui| {
                        let w = 400.0;
                        ui.add_space((ui.available_width() - w).max(0.0) / 2.0);
                        import = ui::button(ui, ui::Kind::Secondary, Some(icon::FILE_XLS), "Загрузить из Excel", true).clicked();
                        add = ui::button(ui, ui::Kind::Primary, Some(icon::USER_PLUS), "Добавить сотрудника", true).clicked();
                    });
                    ui.add_space(34.0);
                });
            });
        });
        if add {
            app.new_employee();
        }
        if import {
            app.pick_import();
        }
        return;
    }

    let mut open = None;
    let mut to_reports = false;
    let mut rows: Vec<usize> = Vec::new();
    fixed_page(ui, 1280.0, |ui| {
        for w in snap.warnings() {
            let (tone, glyph) = match w.severity {
                engine::Severity::Warn => (Tone::Warn, icon::WARNING),
                engine::Severity::Info => (Tone::Info, icon::INFO),
            };
            ui::banner(ui, tone, glyph, &w.text);
            ui.add_space(6.0);
        }

        // Tiles count the people each state would show, so they ignore the state selection.
        let f = &mut app.f.summary.filter;
        let counts = snap.counts_for(&snap.emp_rows(f, Scope::Active, false));
        let toggle = |ui: &mut Ui, f: &mut EmpFilter, group: Group, value: usize, sub: String, tone: Tone| {
            let active = group.active(f);
            if ui::kpi(ui, &group.label().to_uppercase(), &value.to_string(), &sub, tone, active).clicked() {
                group.toggle(f);
            }
        };
        ui.columns(6, |c| {
            toggle(&mut c[0], f, Group::Overdue, counts.overdue, "срок прошёл".into(), Tone::Danger);
            toggle(&mut c[1], f, Group::Burning, counts.burning, format!("до срока ≤ {} дн.", snap.params.burning_days), Tone::Orange);
            toggle(&mut c[2], f, Group::Soon, counts.soon, format!("до срока ≤ {} дн.", snap.params.soon_days), Tone::Warn);
            toggle(&mut c[3], f, Group::Ok, counts.ok, "срок далеко / отгулено".into(), Tone::Ok);
            toggle(&mut c[4], f, Group::Suspended, counts.suspended, "мобилизация, контракт".into(), Tone::Violet);
            if ui::kpi(&mut c[5], "В ОТПУСКЕ", &counts.on_leave.to_string(), "сейчас · открыть отчёт", Tone::Info, false).clicked() {
                to_reports = true;
            }
        });
        ui.add_space(10.0);

        filters::emp_bar(ui, "summary", &snap, f, EmpBar { work: false, states: true, took: true, dismissed: false });
        rows = snap.summary_rows(f);
        let active = snap.active_count();
        let working = snap.emps.iter().filter(|r| r.emp.status == Status::Working).count();
        let note = format!("На {} · действующих: {} · работает: {} · приостановка ТД: {}", date::fmt(snap.params.as_of), active, working, active - working);
        if filters::footer(ui, f.is_active(), f.is_active(), rows.len(), active, &note) {
            f.reset();
        }
        ui.add_space(6.0);

        if rows.is_empty() {
            ui::card(ui, |ui| ui::empty_state(ui, icon::FUNNEL, "Никого не найдено", "Измените фильтры или строку поиска"));
            return;
        }

        let avail = ui.available_width() - 4.0;
        let show = fit(avail, 44.0 + 200.0 + 150.0 + 84.0 + 104.0 + 84.0, &[(1, 210.0), (2, 170.0), (3, 76.0)]);
        ui::flush_card(ui, |ui| {
            let mut t = ui::table(ui, "summary").column(ui::exact(44.0)).column(ui::rest(200.0));
            if show[0] {
                t = t.column(ui::exact(210.0));
            }
            if show[1] {
                t = t.column(ui::exact(170.0));
            }
            t = t.column(ui::exact(150.0)).column(ui::exact(84.0));
            if show[2] {
                t = t.column(ui::exact(76.0));
            }
            t.column(ui::exact(104.0))
                .column(ui::exact(84.0))
                .header(ui::HEAD_H, |mut h| {
                    ui::head_cell(&mut h, "№", false);
                    ui::head_cell(&mut h, "ФИО", false);
                    if show[0] {
                        ui::head_cell(&mut h, "Подразделение", false);
                    }
                    if show[1] {
                        ui::head_cell(&mut h, "Должность", false);
                    }
                    ui::head_cell(&mut h, "Состояние", false);
                    ui::head_tip(&mut h, "Остаток", true, "Сколько дней отпуска ещё не использовано (основной + дополнительный)");
                    if show[2] {
                        ui::head_cell(&mut h, "В т.ч. доп.", true);
                    }
                    ui::head_tip(&mut h, "Срок до", false, "Крайняя дата, до которой надо использовать самый ранний неотгулянный период");
                    ui::head_tip(&mut h, "Дней", true, "Сколько дней осталось до крайнего срока. Минус — срок уже прошёл");
                })
                .body(|body| {
                    body.rows(ui::ROW_H, rows.len(), |mut row| {
                        let n = row.index();
                        let r = &snap.emps[rows[n]];
                        let c = &r.calc;
                        ui::cell_text(&mut row, (n + 1).to_string(), p.text3);
                        let name_color = if r.emp.fio.starts_with(engine::PLACEHOLDER_PREFIX) { p.warn } else { p.text };
                        ui::cell_strong(&mut row, &r.emp.fio, name_color);
                        if show[0] {
                            ui::cell_text(&mut row, &r.dept, p.text2);
                        }
                        if show[1] {
                            ui::cell_text(&mut row, &r.emp.position, p.text2);
                        }
                        ui::cell_state(&mut row, c.state);
                        ui::cell_right(&mut row, engine::fmt_days_opt(c.left), ui::days_color(&p, c.left));
                        if show[2] {
                            ui::cell_right(&mut row, engine::fmt_days_opt(c.left_extra_shown()), p.text2);
                        }
                        ui::cell_text(&mut row, date::fmt_opt(c.deadline()), p.text2);
                        let days = c.days_left();
                        ui::cell_right(
                            &mut row,
                            days.map(|d| d.to_string()).unwrap_or_default(),
                            if days.is_some_and(|d| d < 0) { p.danger } else { p.text2 },
                        );
                        if row.response().clicked() {
                            open = Some(r.emp.id);
                        }
                    });
                });
        });
    });

    if print {
        // Exactly the rows of the table above.
        let html = otpuska_core::print::summary(&snap, &rows, &app.f.summary.filter.describe(&snap, false));
        app.print(html, "svodka");
    }
    if to_reports {
        app.go(Route::Reports);
    }
    if let Some(id) = open {
        app.open_employee(id);
    }
}
