//! The employee card: data form, totals, working years and this employee's leaves.

use super::{leave_form, FormEvent, LeaveDraft};
use crate::app::{page, top_bar, App, Route, Target};
use crate::ui::{self, Kind, Tone};
use egui::{vec2, Align, Layout, Ui};
use egui_phosphor::regular as icon;
use otpuska_core::date;
use otpuska_core::engine::{self, LeaveStatus, PeriodRow, State as St};
use otpuska_core::model::{Employee, Status};
use otpuska_core::Snapshot;

#[derive(Default)]
pub struct Form {
    pub draft: Employee,
    pub orig: Employee,
    pub is_new: bool,
    pub leave: LeaveDraft,
}

impl Form {
    pub fn load(&mut self, snap: &Snapshot, id: i64) {
        if let Some(r) = snap.emp_by_id(id) {
            self.draft = r.emp.clone();
            self.orig = r.emp.clone();
            self.is_new = false;
            self.leave = LeaveDraft::for_employee(Some(id));
        }
    }

    pub fn start_new(&mut self) {
        self.draft = Employee::default();
        self.orig = Employee::default();
        self.is_new = true;
        self.leave = LeaveDraft::default();
    }

    pub fn dirty(&self) -> bool {
        self.draft != self.orig
    }

    fn valid(&self) -> bool {
        !self.draft.fio.trim().is_empty()
    }
}

pub fn show(app: &mut App, ui: &mut Ui, target: &Target) {
    let p = app.pal;
    let snap = app.snap.clone();

    // Make sure the form matches the route (e.g. after a restore or a deletion elsewhere).
    match target {
        Target::Id(id) => {
            if app.f.employee.draft.id != *id || app.f.employee.is_new {
                if snap.emp_by_id(*id).is_some() {
                    app.f.employee.load(&snap, *id);
                } else {
                    let back = app.came_from.clone();
                    app.go(back);
                    return;
                }
            }
        }
        Target::New => {
            if !app.f.employee.is_new {
                app.f.employee.start_new();
            }
        }
    }

    let row = match target {
        Target::Id(id) => snap.emp_by_id(*id).cloned(),
        Target::New => None,
    };
    let back_label = match app.came_from {
        Route::Summary => "Сводка",
        Route::Periods => "Периоды",
        Route::Journal => "Журнал",
        Route::Reports => "Отчёты",
        _ => "Сотрудники",
    };
    let (mut back, mut save, mut cancel, mut delete) = (false, false, false, false);
    let dirty = app.f.employee.dirty();
    let valid = app.f.employee.valid();
    let is_new = app.f.employee.is_new;
    let title = if is_new {
        "Новый сотрудник".to_string()
    } else {
        app.f.employee.orig.fio.clone()
    };
    let armed = app.armed(ui, "delete-employee");
    top_bar(
        ui,
        |ui| {
            back = ui::link(ui, Some(ui::ICON_BACK), back_label).clicked();
            ui.add_space(12.0);
            ui.label(ui::title(&title, 17.0, p.text));
            if let Some(r) = &row {
                ui.add_space(8.0);
                ui::state_pill(ui, r.calc.state);
            }
        },
        |ui| {
            save = ui::button(ui, Kind::Primary, Some(icon::CHECK), if is_new { "Создать" } else { "Сохранить" }, dirty && valid).clicked();
            if dirty || is_new {
                ui.add_space(6.0);
                cancel = ui::button(ui, Kind::Secondary, None, "Отмена", true).clicked();
            }
            if !is_new {
                ui.add_space(6.0);
                let label = if armed { "Удалить сотрудника и его отпуска?" } else { "Удалить" };
                delete = ui::button(ui, if armed { Kind::DangerSolid } else { Kind::Danger }, Some(icon::TRASH), label, true).clicked();
            }
        },
    );

    page(ui, "employee", 1100.0, |ui| {
        ui.columns(2, |cols| {
            // Left: the form.
            ui::card(&mut cols[0], |ui| form(app, ui, &snap));
            // Right: totals and working years.
            if let Some(r) = &row {
                totals(app, &mut cols[1], &snap, r);
            } else {
                ui::card(&mut cols[1], |ui| {
                    ui.add_space(4.0);
                    ui.label(ui::strong("После создания", 14.0, p.text));
                    ui.add_space(4.0);
                    ui.label(ui::text("здесь появятся итоги, периоды и журнал отпусков сотрудника. Состояние считается автоматически по дате приема и отпускам.", 12.5, p.text2));
                });
            }
        });
        if let Some(r) = &row {
            ui.add_space(16.0);
            leaves(app, ui, &snap, r.emp.id);
        }
    });

    if back {
        let to = app.came_from.clone();
        app.go(to);
        return;
    }
    if cancel {
        if is_new {
            let to = app.came_from.clone();
            app.go(to);
        } else {
            app.f.employee.draft = app.f.employee.orig.clone();
        }
        return;
    }
    if save {
        let draft = app.f.employee.draft.clone();
        if let Some(id) = app.save_employee(&draft) {
            let snap = app.snap.clone();
            app.f.employee.load(&snap, id);
            app.toast(true, if is_new { "Сотрудник добавлен" } else { "Сохранено" });
            if is_new {
                app.go(Route::Employee(Target::Id(id)));
            }
        }
    }
    if delete {
        if app.confirm(ui, "delete-employee") {
            let id = app.f.employee.orig.id;
            if app.delete_employee(id) {
                let to = app.came_from.clone();
                app.go(to);
            }
        }
    }
}

fn form(app: &mut App, ui: &mut Ui, snap: &Snapshot) {
    let p = app.pal;
    let f = &mut app.f.employee;
    let d = &mut f.draft;
    ui.spacing_mut().item_spacing = vec2(10.0, 12.0);
    ui::section_title(ui, "Основное");
    let w = ui.available_width();
    ui::field(ui, "ФИО", "", |ui| {
        ui::input(ui, &mut d.fio, "Иванов Иван Иванович", w);
    });
    ui::field(ui, "Должность", "", |ui| {
        ui::input(ui, &mut d.position, "", w);
    });
    ui::field(ui, "Подразделение", "", |ui| {
        ui::dept_picker(ui, "emp-dept", snap, &mut d.department_id, w.min(420.0), "— не указано —");
    });
    ui.horizontal(|ui| {
        ui::field(ui, "Дата приема", "", |ui| {
            ui::date_field(ui, "emp-hire", &mut d.hire_date, "дд.мм.гггг", 150.0);
        });
        ui::field(ui, "Доп. отпуск за вредность", "дн./год", |ui| {
            let mut v = d.extra_days;
            if ui::num_field(ui, "emp-extra", &mut v, "нет", 110.0) {
                d.extra_days = v.filter(|x| *x > 0.0);
            }
        });
    });
    ui::field(ui, "Статус работы", "", |ui| {
        let mut s = d.status;
        if ui::segmented(ui, &mut s, &[(Status::Working, "Работает"), (Status::Suspended, "Приостановка ТД"), (Status::Dismissed, "Уволен")]) {
            d.status = s;
            if s == Status::Dismissed && d.dismiss_date.is_none() {
                d.dismiss_date = Some(date::today());
            }
        }
    });
    if d.status == Status::Dismissed || d.dismiss_date.is_some() {
        ui::field(ui, "Дата увольнения", "", |ui| {
            ui::date_field(ui, "emp-dismiss", &mut d.dismiss_date, "дд.мм.гггг", 150.0);
        });
    }

    ui.add_space(4.0);
    ui::divider(ui);
    // Rarely needed fields stay folded unless something is filled in.
    let any_extra = d.base_days.is_some() || d.accrual_from.is_some() || d.suspend_from.is_some() || d.suspend_to.is_some();
    ui::accordion(ui, ("emp-extra-block", d.id), icon::SLIDERS, "Дополнительно — только при необходимости", any_extra, |ui| {
        ui.spacing_mut().item_spacing = vec2(10.0, 12.0);
        ui.horizontal(|ui| {
            ui::field(ui, "Основной отпуск", "дн./год, если не по умолчанию", |ui| {
                let mut v = d.base_days;
                if ui::num_field(ui, "emp-base", &mut v, &format!("{}", engine::fmt_days(snap.params.base_days)), 110.0) {
                    d.base_days = v.filter(|x| *x > 0.0);
                }
            });
        });
        ui::field(ui, "Учёт стажа с", "если не с даты приема", |ui| {
            ui::date_field(ui, "emp-accrual", &mut d.accrual_from, "дд.мм.гггг", 150.0);
        });
        ui.horizontal(|ui| {
            ui::field(ui, "Приостановка ТД: с", "", |ui| {
                ui::date_field(ui, "emp-susp-from", &mut d.suspend_from, "дд.мм.гггг", 150.0);
            });
            ui::field(ui, "по (возврат)", "", |ui| {
                ui::date_field(ui, "emp-susp-to", &mut d.suspend_to, "дд.мм.гггг", 150.0);
            });
        });
        if d.suspend_from.is_some() {
            let text = if snap.params.suspension_counts {
                "Приостановка засчитывается в стаж (настройка в «Настройках»)."
            } else {
                "Приостановка не засчитывается: рабочие годы сдвигаются на её длительность."
            };
            ui.label(ui::text(text, 11.5, p.text3));
        }
    });
    ui::field(ui, "Примечание", "", |ui| {
        ui::multiline(ui, &mut d.note, "", w, 2);
    });
    if d.hire_date.is_none() {
        ui.label(ui::text("Без даты приема остатки не считаются.", 11.5, p.warn));
    }
}

fn dash(v: Option<f64>) -> String {
    v.map(engine::fmt_days).unwrap_or_else(|| "—".to_string())
}

fn stat(ui: &mut Ui, label: &str, value: String, color: egui::Color32) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui::section_title(ui, label);
        ui.label(ui::title(value, 22.0, color));
    });
}

fn totals(app: &mut App, ui: &mut Ui, snap: &Snapshot, r: &engine::EmpRow) {
    let p = app.pal;
    let c = &r.calc;
    ui::card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(ui::strong("Итоги за все рабочие годы", 14.0, p.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui::state_pill(ui, c.state);
            });
        });
        ui.add_space(8.0);
        ui.columns(3, |cols| {
            stat(&mut cols[0], "Положено", dash(c.entitled), p.text);
            stat(&mut cols[1], "Отгуляно", engine::fmt_days(c.taken), p.text);
            stat(&mut cols[2], "Остаток", dash(c.left), ui::days_color(&p, c.left));
        });
        ui.add_space(8.0);
        ui::divider(ui);
        ui.add_space(8.0);
        let line = |ui: &mut Ui, k: &str, v: String, color: egui::Color32| {
            ui.horizontal(|ui| {
                ui.label(ui::text(k, 12.5, p.text2));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(ui::strong(v, 12.5, color));
                });
            });
        };
        ui.spacing_mut().item_spacing.y = 5.0;
        if let Some(base) = c.accrual_base {
            line(ui, "Рабочие годы считаются с", date::fmt(base), p.text);
        }
        if c.entitled.is_some() {
            line(
                ui,
                "Рабочих лет начато / доступно",
                format!("{} / {}", c.started, c.available),
                p.text,
            );
            line(
                ui,
                "Положено в год",
                if c.extra_per_year > 0.0 {
                    format!("{} + {} (доп.)", engine::fmt_days(c.base_per_year), engine::fmt_days(c.extra_per_year))
                } else {
                    engine::fmt_days(c.base_per_year)
                },
                p.text,
            );
            if c.extra_per_year > 0.0 || c.taken_extra > 0.0 {
                line(ui, "Остаток: основной / дополнительный", format!("{} / {}", engine::fmt_days_opt(c.left_base), engine::fmt_days_opt(c.left_extra)), p.text);
            }
        }
        if let Some(dl) = c.deadline() {
            let days = c.days_left().unwrap_or(0);
            let (txt, color) = if days < 0 { (format!("{} · просрочено на {} дн.", date::fmt(dl), -days), p.danger) } else { (format!("{} · осталось {} дн.", date::fmt(dl), days), p.text) };
            line(ui, "Крайний срок (самый ранний период)", txt, color);
            if let Some((a, b)) = c.early_period_dates() {
                line(ui, "Этот период", format!("{} – {}", date::fmt(a), date::fmt(b)), p.text2);
            }
        }
        if c.state == St::NoRight {
            ui.add_space(4.0);
            ui.label(ui::text(
                format!("Право на первый отпуск возникает через {} мес. работы.", snap.params.first_leave_months),
                12.0,
                p.text3,
            ));
        }
    });

    let periods = engine::periods(&r.emp, c, &snap.params);
    if !periods.is_empty() {
        ui.add_space(12.0);
        ui::card(ui, |ui| {
            ui.label(ui::strong("Рабочие годы", 14.0, p.text));
            ui.add_space(6.0);
            for pr in periods.iter().rev() {
                period_line(ui, &p, pr);
            }
        });
    }
}

fn period_line(ui: &mut Ui, p: &crate::theme::Pal, pr: &PeriodRow) {
    let tone = ui::period_tone(pr.tone);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), egui::Sense::hover());
    let painter = ui.painter();
    let mid = rect.center().y;
    painter.text(rect.left_center() + vec2(0.0, 0.0), egui::Align2::LEFT_CENTER, format!("{}", pr.k), egui::FontId::new(11.5, egui::FontFamily::Proportional), p.text3);
    painter.text(
        rect.left_center() + vec2(22.0, 0.0),
        egui::Align2::LEFT_CENTER,
        format!("{} – {}", date::fmt(pr.start), date::fmt(pr.end)),
        egui::FontId::new(12.5, crate::theme::medium()),
        p.text,
    );
    // Right side: remaining + status pill drawn by hand to stay on one line.
    let (label, left_txt) = match (pr.available, pr.left) {
        (false, _) => ("ещё не положено".to_string(), String::new()),
        (true, Some(l)) if l == 0.0 => ("использован".to_string(), "0".to_string()),
        (true, Some(l)) => (
            if pr.days_to_deadline < 0 { format!("срок вышел {} дн. назад", -pr.days_to_deadline) } else { format!("срок через {} дн.", pr.days_to_deadline) },
            engine::fmt_days(l),
        ),
        _ => (String::new(), String::new()),
    };
    let color = tone.color(p);
    let font = egui::FontId::new(11.5, egui::FontFamily::Proportional);
    let gal = painter.layout_no_wrap(label, font, color);
    let right = rect.right();
    painter.galley(egui::pos2(right - gal.size().x, mid - gal.size().y / 2.0), gal.clone(), color);
    if !left_txt.is_empty() {
        painter.text(
            egui::pos2(right - gal.size().x - 12.0, mid),
            egui::Align2::RIGHT_CENTER,
            left_txt,
            egui::FontId::new(13.5, crate::theme::semibold()),
            if pr.tone == engine::PeriodTone::Used { p.text3 } else { p.text },
        );
    }
    painter.hline(rect.x_range(), rect.bottom(), egui::Stroke::new(1.0, p.border));
    resp.on_hover_text(format!(
        "Положено {} · отгуляно {} · остаток {}{}\nКрайний срок {}",
        engine::fmt_days(pr.entitled),
        engine::fmt_days_opt(pr.taken),
        engine::fmt_days_opt(pr.left),
        pr.left_extra.map(|e| format!(" (в т.ч. доп. {})", engine::fmt_days(e))).unwrap_or_default(),
        date::fmt(pr.deadline)
    ));
}

fn leaves(app: &mut App, ui: &mut Ui, snap: &Snapshot, id: i64) {
    let p = app.pal;
    let rows: Vec<usize> = {
        let mut v: Vec<usize> = (0..snap.leaves.len()).filter(|&i| snap.leaves[i].leave.employee_id == id).collect();
        v.sort_by_key(|&i| std::cmp::Reverse((snap.leaves[i].leave.start, snap.leaves[i].leave.id)));
        v
    };
    let mut submit = false;
    let mut cancel = false;
    let mut edit = None;
    let mut delete = None;
    ui::card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(ui::strong("Отпуска сотрудника", 14.0, p.text));
            ui.label(ui::text(format!("{} записей", rows.len()), 12.0, p.text3));
        });
        ui.add_space(8.0);
        match leave_form(ui, snap, &mut app.f.employee.leave, false, "emp-leave") {
            FormEvent::Submit => submit = true,
            FormEvent::Cancel => cancel = true,
            FormEvent::None => {}
        }
        if rows.is_empty() {
            ui.add_space(10.0);
            ui.label(ui::text("Отпусков пока не внесено. Старый отпуск без точных дат: оставьте дату пустой и впишите только дни.", 12.5, p.text3));
            return;
        }
        ui.add_space(10.0);
        ui::divider(ui);
        for &i in &rows {
            let l = &snap.leaves[i];
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), egui::Sense::hover());
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center)));
            child.spacing_mut().item_spacing.x = 10.0;
            let period = match (l.leave.start, l.end) {
                (Some(s), Some(e)) => format!("{} – {}", date::fmt(s), date::fmt(e)),
                _ => "без даты".to_string(),
            };
            child.add_sized([190.0, 20.0], egui::Label::new(ui::strong(period, 13.0, p.text)));
            let days = match (l.leave.base_days > 0.0, l.leave.extra_days > 0.0) {
                (true, true) => format!("{} + {} доп.", engine::fmt_days(l.leave.base_days), engine::fmt_days(l.leave.extra_days)),
                (false, true) => format!("{} доп.", engine::fmt_days(l.leave.extra_days)),
                _ => format!("{} дн.", engine::fmt_days(l.leave.base_days)),
            };
            child.add_sized([110.0, 20.0], egui::Label::new(ui::text(days, 13.0, p.text2)));
            if let Some(s) = l.status {
                ui::pill(&mut child, s.label(), match s {
                    LeaveStatus::InLeave => Tone::Ok,
                    LeaveStatus::Planned => Tone::Info,
                    LeaveStatus::Done => Tone::Neutral,
                    LeaveStatus::Undated => Tone::Muted,
                });
            }
            child.add(egui::Label::new(ui::text(&l.leave.note, 12.5, p.text3)).truncate());
            child.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui::icon_button(ui, icon::TRASH, "Удалить запись", true).clicked() {
                    delete = Some(l.leave.id);
                }
                if ui::icon_button(ui, icon::PENCIL_SIMPLE, "Изменить", true).clicked() {
                    edit = Some(l.leave.clone());
                }
            });
            ui.painter().hline(rect.x_range(), rect.bottom(), egui::Stroke::new(1.0, p.border));
        }
    });

    if let Some(l) = edit {
        app.f.employee.leave = super::LeaveDraft::from_leave(&l);
    }
    if let Some(lid) = delete {
        let key = format!("delete-leave-{lid}");
        if app.armed(ui, &key) {
            app.confirm(ui, &key);
            app.delete_leave(lid);
            app.f.employee.leave = LeaveDraft::for_employee(Some(id));
        } else {
            app.confirm(ui, &key);
            app.toast(true, "Нажмите на корзину ещё раз, чтобы удалить запись");
        }
    }
    if cancel {
        app.f.employee.leave = LeaveDraft::for_employee(Some(id));
    }
    if submit {
        if let Some(l) = app.f.employee.leave.to_leave() {
            let editing = app.f.employee.leave.is_edit();
            if app.save_leave(&l) {
                app.toast(true, if editing { "Запись изменена" } else { "Отпуск добавлен" });
                app.f.employee.leave = LeaveDraft::for_employee(Some(id));
            }
        }
    }
}
