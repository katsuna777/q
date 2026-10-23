//! «Периоды»: working years of every active employee side by side — what is left in each.

use super::filters::{self, EmpBar};
use crate::app::{fixed_page, top_bar, App};
use crate::theme::{icons, medium, Pal};
use crate::ui::{self, Kind, Tone};
use egui::{vec2, Align, Align2, CornerRadius, FontId, Layout, Sense, Ui};
use egui_phosphor::regular as icon;
use otpuska_core::date;
use otpuska_core::engine::{self, PeriodRow, PeriodTone};
use otpuska_core::filter::{EmpFilter, RestFilter};
use otpuska_core::model::Status;

pub struct State {
    pub first_year: i32,
    pub span: usize,
    pub filter: EmpFilter,
    pub rest: RestFilter,
}

impl Default for State {
    fn default() -> Self {
        State { first_year: 2022, span: 8, filter: EmpFilter::default(), rest: RestFilter::Any }
    }
}

const NAME_W: f32 = 210.0;
const STATE_W: f32 = 132.0;
const TOTAL_W: f32 = 70.0;
const YEAR_W: f32 = 108.0;
const ROW_H: f32 = 48.0;

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.pal;
    let snap = app.snap.clone();
    let mut print = false;
    top_bar(
        ui,
        |ui| {
            ui.label(ui::title("Периоды", 17.0, p.text));
        },
        |ui| {
            if !snap.emps.is_empty() {
                print = ui::button(ui, Kind::Secondary, Some(icon::PRINTER), "Печать", true).clicked();
            }
        },
    );

    let mut open = None;
    let mut shown_rows: Vec<usize> = Vec::new();
    fixed_page(ui, 1800.0, |ui| {
        if snap.emps.iter().all(|r| r.emp.status == Status::Dismissed) {
            ui::card(ui, |ui| ui::empty_state(ui, icon::TABLE, "Пока нечего показывать", "Рабочие годы появятся, когда на вкладке «Сотрудники» будут действующие сотрудники с датой приема"));
            return;
        }
        let st = &mut app.f.periods;
        let span = st.span;
        ui.horizontal(|ui| {
            if ui::icon_button(ui, icon::CARET_LEFT, "Раньше", true).clicked() {
                st.first_year = (st.first_year - span as i32).max(1990);
            }
            ui.label(ui::strong(format!("{} – {}", st.first_year, st.first_year + span as i32 - 1), 13.5, p.text));
            if ui::icon_button(ui, icon::CARET_RIGHT, "Позже", true).clicked() {
                st.first_year = (st.first_year + span as i32).min(2090);
            }
            if ui::link(ui, None, "к текущему").clicked() {
                st.first_year = snap.default_first_year();
            }
            ui.add_space(8.0);
            ui::segmented(ui, &mut st.span, &[(4, "4 года"), (6, "6 лет"), (8, "8 лет")]);
            ui.add_space(8.0);
            ui.label(ui::text("Отпуск", 12.5, p.text3));
            let tip = "«Не отгулял» — в показанных годах есть неиспользованные дни; «Отгулял» — в этих годах всё использовано";
            ui::segmented(
                ui,
                &mut st.rest,
                &[(RestFilter::Any, RestFilter::Any.label()), (RestFilter::HasLeft, RestFilter::HasLeft.label()), (RestFilter::NoLeft, RestFilter::NoLeft.label())],
            );
            ui.label(ui::icon_text(icon::INFO, 14.0, p.text3)).on_hover_text(tip);
        });
        ui.add_space(8.0);
        filters::emp_bar(ui, "periods", &snap, &mut st.filter, EmpBar { work: false, states: true, took: false, dismissed: false });
        let (first, span) = (st.first_year, st.span);
        let rows = snap.period_rows(&st.filter, st.rest, first, span);
        let total = snap.emps.iter().filter(|r| r.emp.status != Status::Dismissed).count();
        let active = st.filter.is_active() || st.rest != RestFilter::Any;
        let reset = filters::footer(ui, active, active, rows.len(), total, "");
        if reset {
            st.filter.reset();
            st.rest = RestFilter::Any;
        }
        ui.horizontal(|ui| {
            ui.label(ui::text("Остаток дней за рабочий год, считая от даты приема. Жёлтые даты — год продлён отпуском без оплаты или по уходу за ребёнком. Наведите курсор на ячейку — подробности.", 12.0, p.text3));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| legend(ui, &p));
        });
        ui.add_space(6.0);
        shown_rows = rows.clone();
        if rows.is_empty() {
            ui::card(ui, |ui| ui::empty_state(ui, icon::FUNNEL, "Никого не найдено", "Измените фильтры или строку поиска"));
            return;
        }
        ui::flush_card(ui, |ui| {
            egui::ScrollArea::horizontal().auto_shrink([false, false]).show(ui, |ui| {
                let mut t = ui::table(ui, "periods").column(ui::exact(NAME_W)).column(ui::exact(STATE_W)).column(ui::exact(TOTAL_W));
                for _ in 0..span {
                    t = t.column(ui::exact(YEAR_W));
                }
                t.header(ui::HEAD_H, |mut h| {
                    ui::head_cell(&mut h, "ФИО", false);
                    ui::head_cell(&mut h, "Состояние", false);
                    ui::head_cell(&mut h, "Всего", true);
                    for y in 0..span {
                        ui::head_cell(&mut h, &(first + y as i32).to_string(), false);
                    }
                })
                .body(|body| {
                    body.rows(ROW_H, rows.len(), |mut row| {
                        let r = &snap.emps[rows[row.index()]];
                        ui::cell_strong(&mut row, &r.emp.fio, p.text);
                        ui::cell_state(&mut row, r.calc.state);
                        ui::cell_right(&mut row, engine::fmt_days_opt(r.calc.left), ui::days_color(&p, r.calc.left));
                        for y in 0..span {
                            let pr = engine::period_for_year(&r.emp, &r.calc, &snap.params, first + y as i32);
                            period_cell(&mut row, &p, pr.as_ref());
                        }
                        if row.response().clicked() {
                            open = Some(r.emp.id);
                        }
                    });
                });
            });
        });
    });

    if print {
        let (first, span) = (app.f.periods.first_year, app.f.periods.span);
        let mut lines = app.f.periods.filter.describe(&snap, false);
        if app.f.periods.rest != RestFilter::Any {
            lines.insert(0, format!("Отпуск: {}", app.f.periods.rest.label().to_lowercase()));
        }
        // Exactly the rows of the table, for the years shown on the screen.
        let html = otpuska_core::print::periods(&snap, &shown_rows, first, span, &lines);
        app.print(html, "periody");
    }
    if let Some(id) = open {
        app.open_employee(id);
    }
}

fn legend(ui: &mut Ui, p: &Pal) {
    // Right-to-left layout: items are added from the right.
    for (tone, label) in [
        (Tone::Info, "есть остаток"),
        (Tone::Warn, "скоро"),
        (Tone::Orange, "горит"),
        (Tone::Danger, "просрочено"),
        (Tone::Ok, "использован"),
    ] {
        ui.label(ui::text(label, 11.5, p.text3));
        let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
        ui.painter().rect_filled(r, CornerRadius::same(3), tone.color(p));
        ui.add_space(6.0);
    }
}

fn period_cell(row: &mut egui_extras::TableRow, p: &Pal, pr: Option<&PeriodRow>) {
    let (_, resp) = row.col(|ui| {
        let rect = ui.max_rect();
        ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, egui::Stroke::new(1.0, p.border));
        let Some(pr) = pr else { return };
        let tone = ui::period_tone(pr.tone);
        let color = tone.color(p);
        let box_rect = rect.shrink2(vec2(4.0, 5.0));
        let fill = match pr.tone {
            PeriodTone::Future => p.sunken,
            _ => tone.bg(p),
        };
        ui.painter().rect_filled(box_rect, CornerRadius::same(7), fill);
        let top = box_rect.top() + 14.0;
        match (pr.tone, pr.left) {
            (PeriodTone::Used, _) => {
                ui.painter().text(egui::pos2(box_rect.left() + 10.0, top), Align2::LEFT_CENTER, icon::CHECK, FontId::new(14.0, icons()), color);
            }
            (PeriodTone::Future, _) => {
                ui.painter().text(egui::pos2(box_rect.left() + 9.0, top), Align2::LEFT_CENTER, "—", FontId::new(13.0, medium()), p.text3);
            }
            (_, Some(l)) => {
                ui.painter().text(egui::pos2(box_rect.left() + 9.0, top), Align2::LEFT_CENTER, engine::fmt_days(l), FontId::new(14.5, crate::theme::semibold()), color);
            }
            _ => {}
        }
        if let Some(e) = pr.left_extra.filter(|e| *e > 0.0) {
            ui.painter().text(
                egui::pos2(box_rect.right() - 7.0, top),
                Align2::RIGHT_CENTER,
                format!("+{} доп.", engine::fmt_days(e)),
                FontId::new(10.5, egui::FontFamily::Proportional),
                p.text2,
            );
        }
        ui.painter().text(
            egui::pos2(box_rect.center().x, box_rect.bottom() - 8.5),
            Align2::CENTER_CENTER,
            format!("{}–{}", date::fmt_short(pr.start), date::fmt_short(pr.end)),
            FontId::new(9.5, egui::FontFamily::Proportional),
            // A year that lasts longer than twelve months (unpaid / childcare leave) is marked.
            if pr.shift > 0 { p.warn } else { p.text3 },
        );
    });
    if let Some(pr) = pr {
        resp.on_hover_ui(|ui| {
            let line = format!(
                "Рабочий год {} – {}\nПоложено {} · отгулено {} · остаток {}{}\nКрайний срок {}{}{}",
                date::fmt(pr.start),
                date::fmt(pr.end),
                engine::fmt_days(pr.entitled),
                engine::fmt_days_opt(pr.taken),
                engine::fmt_days_opt(pr.left),
                pr.left_extra.map(|e| format!(" (в т.ч. доп. {})", engine::fmt_days(e))).unwrap_or_default(),
                date::fmt(pr.deadline),
                if !pr.available {
                    "\nПраво на этот период ещё не возникло".to_string()
                } else if pr.left.is_some_and(|l| l > 0.0) {
                    if pr.days_to_deadline < 0 {
                        format!(" · просрочено на {} дн.", -pr.days_to_deadline)
                    } else {
                        format!(" · осталось {} дн.", pr.days_to_deadline)
                    }
                } else {
                    String::new()
                },
                super::employee::shift_text(pr),
            );
            ui.label(line);
        });
    }
}
