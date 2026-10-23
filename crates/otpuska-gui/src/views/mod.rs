pub mod employee;
pub mod employees;
pub mod filters;
pub mod journal;
pub mod periods;
pub mod reminders;
pub mod reports;
pub mod schedule;
pub mod settings;
pub mod summary;

use crate::app::Route;
use crate::ui::{self, Kind};
use egui::Ui;
use egui_phosphor::regular as icon;
use otpuska_core::date::{self, Date};
use otpuska_core::engine;
use otpuska_core::model::{Leave, LeaveType, Plan};
use otpuska_core::Snapshot;

/// Per-screen UI state (filters, drafts) that survives switching tabs.
#[derive(Default)]
pub struct Forms {
    pub summary: summary::State,
    pub employees: employees::State,
    pub employee: employee::Form,
    pub periods: periods::State,
    pub reports: reports::State,
    pub journal: journal::State,
    pub schedule: schedule::State,
    pub reminders: reminders::State,
    pub settings: settings::State,
}

impl Forms {
    pub fn on_leave(&mut self, route: &Route) {
        if let Route::Settings = route {
            self.settings.sync = true;
            self.settings.db_info = None;
        }
    }
}

/// Sort column and direction for a table.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Sort<K: Copy + PartialEq> {
    pub key: K,
    pub asc: bool,
}

impl<K: Copy + PartialEq> Sort<K> {
    pub fn new(key: K, asc: bool) -> Self {
        Sort { key, asc }
    }

    /// Clicking the active column flips the direction, another column starts ascending.
    pub fn click(&mut self, key: K) {
        if self.key == key {
            self.asc = !self.asc;
        } else {
            *self = Sort { key, asc: true };
        }
    }

    pub fn arrow(&self, key: K) -> Option<bool> {
        (self.key == key).then_some(self.asc)
    }
}

/// Which optional columns fit into `avail` pixels next to `reserve` pixels of required ones.
/// `cols` are `(priority, width)`; a lower priority number is kept first.
pub fn fit(avail: f32, reserve: f32, cols: &[(u8, f32)]) -> Vec<bool> {
    let mut order: Vec<usize> = (0..cols.len()).collect();
    order.sort_by_key(|&i| (cols[i].0, i));
    let mut used = reserve;
    let mut show = vec![false; cols.len()];
    for i in order {
        if used + cols[i].1 <= avail {
            used += cols[i].1;
            show[i] = true;
        }
    }
    show
}

// ---- the leave form (journal and employee card) -----------------------------------------

#[derive(Clone, Default, PartialEq, Debug)]
pub struct LeaveDraft {
    pub id: i64,
    pub employee: Option<i64>,
    pub kind: LeaveType,
    pub start: Option<Date>,
    /// Last day; for the kinds of leave other than the annual one.
    pub end: Option<Date>,
    pub base: f64,
    pub extra: f64,
    pub note: String,
    /// Childcare: the child's birth date, only to fill in the end date (it is not stored).
    pub child_birth: Option<Date>,
}

impl LeaveDraft {
    pub fn for_employee(id: Option<i64>) -> Self {
        LeaveDraft { employee: id, ..Default::default() }
    }

    pub fn from_leave(l: &Leave) -> Self {
        LeaveDraft {
            id: l.id,
            employee: Some(l.employee_id),
            kind: l.kind,
            start: l.start,
            end: l.end,
            base: l.base_days,
            extra: l.extra_days,
            note: l.note.clone(),
            child_birth: None,
        }
    }

    pub fn to_leave(&self) -> Option<Leave> {
        let paid = self.kind.is_paid();
        Some(Leave {
            id: self.id,
            employee_id: self.employee?,
            kind: self.kind,
            start: self.start,
            end: if paid { None } else { self.end },
            base_days: if paid { self.base } else { 0.0 },
            extra_days: if paid { self.extra } else { 0.0 },
            note: self.note.clone(),
        })
    }

    pub fn is_edit(&self) -> bool {
        self.id != 0
    }

    /// Why the record cannot be saved yet.
    pub fn problem(&self) -> Option<&'static str> {
        if self.employee.is_none() {
            return Some("Выберите сотрудника");
        }
        if self.kind.is_paid() {
            return (self.base + self.extra <= 0.0).then_some("Укажите число дней");
        }
        match (self.start, self.end) {
            (Some(s), Some(e)) if e >= s => None,
            (Some(_), Some(_)) => Some("Окончание раньше начала"),
            _ => Some("Укажите даты начала и окончания"),
        }
    }

    /// Calendar days of a leave given by two dates.
    pub fn span_days(&self) -> Option<i64> {
        match (self.start, self.end) {
            (Some(s), Some(e)) if e >= s => Some((e - s).num_days() + 1),
            _ => None,
        }
    }
}

#[derive(PartialEq, Eq)]
pub enum FormEvent {
    None,
    Submit,
    Cancel,
}

/// «1 день», «2 дня», «5 дней».
fn days_word(n: i64) -> &'static str {
    let (a, b) = (n % 10, n % 100);
    if a == 1 && b != 11 {
        "день"
    } else if (2..=4).contains(&a) && !(12..=14).contains(&b) {
        "дня"
    } else {
        "дней"
    }
}

/// A small helper button with a caption: «140 дн.».
fn quick(ui: &mut Ui, label: &str) -> egui::Response {
    ui::button(ui, Kind::Secondary, None, label, true)
}

/// What an unpaid or childcare leave being entered does to the working years of the employee.
fn effect_note(ui: &mut Ui, snap: &Snapshot, d: &LeaveDraft) {
    let p = ui::pal(ui);
    let (Some(emp), Some(start), Some(end)) = (d.employee, d.start, d.end) else { return };
    if end < start || !matches!(d.kind, LeaveType::Unpaid | LeaveType::Childcare) {
        return;
    }
    let years = engine::leave_effect(snap, emp, d.id, d.kind, start, end);
    let Some(y) = years.iter().find(|y| y.shift > 0).or(years.first()) else {
        ui.add_space(2.0);
        ui.label(ui::text("Отпуск приходится на время до даты приема — рабочие годы не меняются.", 12.0, p.text3));
        return;
    };
    let touched = years.iter().filter(|y| y.shift > 0).count();
    let (text, warn) = match (d.kind, y.shift) {
        (LeaveType::Unpaid, 0) => (
            format!("В рабочем году {} – {} без оплаты {} {} — в пределах 14 дней, стаж не меняется.", date::fmt(y.start), date::fmt(y.end), y.unpaid, days_word(y.unpaid)),
            false,
        ),
        (LeaveType::Unpaid, n) => (
            format!(
                "В рабочем году {} – {} без оплаты {} {}: первые 14 дней идут в стаж, остальные {} {} — нет. Рабочий год заканчивается {}, следующий начнётся {}.",
                date::fmt(y.start),
                date::fmt(y.end),
                y.unpaid,
                days_word(y.unpaid),
                n,
                days_word(n),
                date::fmt(y.end),
                date::fmt(y.end + chrono::Duration::days(1)),
            ),
            true,
        ),
        (_, 0) => ("Отпуск по уходу за ребёнком не идёт в стаж.".to_string(), false),
        (_, n) => (
            format!(
                "Рабочий год {} – {} продлится на {} {}{}; следующий начнётся {}.",
                date::fmt(y.start),
                date::fmt(y.end),
                n,
                days_word(n),
                if touched > 1 { format!(" (затронуто рабочих лет: {touched})") } else { String::new() },
                date::fmt(y.end + chrono::Duration::days(1)),
            ),
            true,
        ),
    };
    ui.add_space(2.0);
    ui.add(egui::Label::new(ui::text(text, 12.0, if warn { p.warn } else { p.text3 })).wrap());
}

/// " · рабочий год продлён на 6 дн." for a record of unpaid or childcare leave that moves the end
/// of a working year; empty otherwise. Asked before the record is saved.
pub fn shift_note(snap: &Snapshot, l: &Leave) -> String {
    let (Some(start), Some(end)) = (l.start, l.end) else { return String::new() };
    if end < start || !matches!(l.kind, LeaveType::Unpaid | LeaveType::Childcare) {
        return String::new();
    }
    let shift: i64 = engine::leave_effect(snap, l.employee_id, l.id, l.kind, start, end).iter().map(|y| y.shift).sum();
    let before: i64 = engine::leave_effect(snap, l.employee_id, l.id, LeaveType::Paid, start, end).iter().map(|y| y.shift).sum();
    match shift - before {
        0 => String::new(),
        n => format!(" · рабочий год продлён на {n} {}", days_word(n)),
    }
}

/// Form for a journal record, in two rows: who and when, then the extras, the note and the button.
/// (Two explicit rows instead of an automatic wrap: nested field groups do not wrap by themselves.)
/// `pick_employee` adds the employee selector.
pub fn leave_form(ui: &mut Ui, snap: &Snapshot, d: &mut LeaveDraft, pick_employee: bool, id: &str) -> FormEvent {
    let p = ui::pal(ui);
    let mut event = FormEvent::None;
    ui.horizontal(|ui| {
        ui.label(ui::text("Вид отпуска", 12.5, p.text3));
        let options: Vec<(LeaveType, &str)> = LeaveType::ALL.iter().map(|k| (*k, k.label())).collect();
        ui::segmented(ui, &mut d.kind, &options);
    });
    ui.add_space(6.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
        if pick_employee {
            ui::field(ui, "Сотрудник", "", |ui| {
                ui::emp_picker(ui, (id, "emp"), snap, &mut d.employee, "Выберите сотрудника…", 270.0, true);
            });
        }
        ui::field(ui, if d.kind.is_paid() { "Начало отпуска" } else { "Первый день" }, "", |ui| {
            ui::date_field(ui, (id, "start"), &mut d.start, "дд.мм.гггг", 150.0);
        });
        if d.kind.is_paid() {
            ui::field(ui, "Основной, дн.", "", |ui| {
                ui::num_field_req(ui, (id, "base"), &mut d.base, "0", 90.0);
            });
            ui::field(ui, "Доп. за вредн., дн.", "", |ui| {
                ui::num_field_req(ui, (id, "extra"), &mut d.extra, "0", 90.0);
            });
        } else {
            ui::field(ui, "Последний день", d.span_days().map(|n| format!("{n} {}", days_word(n))).unwrap_or_default().as_str(), |ui| {
                ui::date_field(ui, (id, "end"), &mut d.end, "дд.мм.гггг", 150.0);
            });
        }
    });
    ui.add_space(8.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
        match d.kind {
            LeaveType::Maternity => {
                ui::field(ui, "Продолжительность", "ст. 255 ТК РФ", |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        for (days, hint) in [
                            (140, "Обычно: 70 дней до родов и 70 после"),
                            (156, "70 дней до родов и 86 после осложнённых родов"),
                            (194, "Многоплодная беременность: 84 дня до и 110 после"),
                        ] {
                            if quick(ui, &format!("{days} дн.")).on_hover_text(hint).clicked() {
                                if let Some(s) = d.start {
                                    d.end = Some(s + chrono::Duration::days(days - 1));
                                }
                            }
                        }
                    });
                });
            }
            LeaveType::Childcare => {
                ui::field(ui, "Ребёнок родился", "", |ui| {
                    ui::date_field(ui, (id, "birth"), &mut d.child_birth, "дд.мм.гггг", 150.0);
                });
                ui::field(ui, "Отпуск до", "возраста", |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        for (months, label) in [(18, "1,5 лет"), (36, "3 лет")] {
                            let r = ui::button(ui, Kind::Secondary, None, label, d.child_birth.is_some());
                            if d.child_birth.is_none() {
                                r.on_disabled_hover_text("Сначала укажите дату рождения ребёнка");
                            } else if r.clicked() {
                                if let Some(b) = d.child_birth {
                                    d.end = Some(date::edate(b, months) - chrono::Duration::days(1));
                                }
                            }
                        }
                    });
                });
            }
            _ => {}
        }
        ui::field(ui, "Примечание", "приказ, период и т.п.", |ui| {
            ui::input(ui, &mut d.note, "", 260.0);
        });
        ui.vertical(|ui| {
            ui.add_space(19.0);
            ui.horizontal(|ui| {
                let (glyph, label) = if d.is_edit() { (icon::CHECK, "Сохранить") } else { (icon::PLUS, "Добавить") };
                let problem = d.problem();
                let r = ui::button(ui, Kind::Primary, Some(glyph), label, problem.is_none());
                if let Some(hint) = problem {
                    r.on_disabled_hover_text(hint);
                } else if r.clicked() {
                    event = FormEvent::Submit;
                }
                if d.is_edit() && ui::button(ui, Kind::Ghost, None, "Отмена", true).clicked() {
                    event = FormEvent::Cancel;
                }
            });
        });
    });
    match d.kind {
        LeaveType::Paid => {}
        LeaveType::Unpaid => {
            ui.add_space(2.0);
            ui.add(egui::Label::new(ui::text("Свыше 14 календарных дней в рабочем году такой отпуск в стаж не засчитывается: окончание рабочего года переносится на лишние дни, следующий год считается со дня после него (ст. 121, 128 ТК РФ).", 11.5, p.text3)).wrap());
        }
        LeaveType::Maternity => {
            ui.add_space(2.0);
            ui.add(egui::Label::new(ui::text("Идёт в стаж — рабочие годы не сдвигаются. За 2 недели до окончания программа напомнит о выходе на работу.", 11.5, p.text3)).wrap());
        }
        LeaveType::Childcare => {
            ui.add_space(2.0);
            ui.add(egui::Label::new(ui::text("В стаж не включается (ст. 121 ТК РФ): рабочий год продлевается на весь срок отпуска. За 2 недели до окончания программа напомнит о выходе на работу.", 11.5, p.text3)).wrap());
        }
    }
    effect_note(ui, snap, d);
    if d.is_edit() {
        ui.add_space(2.0);
        ui.label(ui::text("Изменение существующей записи", 11.5, p.warn));
    }
    event
}

// ---- the schedule form ---------------------------------------------------------------------------

#[derive(Clone, Default, PartialEq, Debug)]
pub struct PlanDraft {
    pub id: i64,
    pub employee: Option<i64>,
    pub start: Option<Date>,
    pub base: f64,
    pub extra: f64,
    pub note: String,
}

impl PlanDraft {
    pub fn from_plan(p: &Plan) -> Self {
        PlanDraft { id: p.id, employee: Some(p.employee_id), start: p.start, base: p.base_days, extra: p.extra_days, note: p.note.clone() }
    }

    pub fn to_plan(&self) -> Option<Plan> {
        Some(Plan { id: self.id, employee_id: self.employee?, start: self.start, base_days: self.base, extra_days: self.extra, note: self.note.clone() })
    }

    pub fn is_edit(&self) -> bool {
        self.id != 0
    }

    pub fn problem(&self) -> Option<&'static str> {
        if self.employee.is_none() {
            Some("Выберите сотрудника")
        } else if self.start.is_none() {
            Some("Укажите дату начала")
        } else if self.base + self.extra <= 0.0 {
            Some("Укажите число дней")
        } else {
            None
        }
    }
}

/// Form for an entry of the vacation schedule, in two rows.
pub fn plan_form(ui: &mut Ui, snap: &Snapshot, d: &mut PlanDraft, id: &str) -> FormEvent {
    let p = ui::pal(ui);
    let mut event = FormEvent::None;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
        ui::field(ui, "Сотрудник", "", |ui| {
            ui::emp_picker(ui, (id, "emp"), snap, &mut d.employee, "Выберите сотрудника…", 270.0, true);
        });
        ui::field(ui, "Начало отпуска", "", |ui| {
            ui::date_field(ui, (id, "start"), &mut d.start, "дд.мм.гггг", 150.0);
        });
        ui::field(ui, "Основной, дн.", "", |ui| {
            ui::num_field_req(ui, (id, "base"), &mut d.base, "0", 90.0);
        });
        ui::field(ui, "Доп. за вредн., дн.", "", |ui| {
            ui::num_field_req(ui, (id, "extra"), &mut d.extra, "0", 90.0);
        });
    });
    ui.add_space(8.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
        ui::field(ui, "Примечание", "", |ui| {
            ui::input(ui, &mut d.note, "", 400.0);
        });
        ui.vertical(|ui| {
            ui.add_space(19.0);
            ui.horizontal(|ui| {
                let (glyph, label) = if d.is_edit() { (icon::CHECK, "Сохранить") } else { (icon::PLUS, "В график") };
                let problem = d.problem();
                let r = ui::button(ui, Kind::Primary, Some(glyph), label, problem.is_none());
                if let Some(hint) = problem {
                    r.on_disabled_hover_text(hint);
                } else if r.clicked() {
                    event = FormEvent::Submit;
                }
                if d.is_edit() && ui::button(ui, Kind::Ghost, None, "Отмена", true).clicked() {
                    event = FormEvent::Cancel;
                }
            });
        });
    });
    if let (Some(s), true) = (d.start, d.base + d.extra > 0.0) {
        let end = s + chrono::Duration::days((d.base + d.extra).floor() as i64 - 1);
        ui.add_space(2.0);
        ui.label(ui::text(format!("Отпуск по графику: {} – {}", date::fmt(s), date::fmt(end)), 11.5, p.text3));
    }
    if d.is_edit() {
        ui.add_space(2.0);
        ui.label(ui::text("Изменение записи графика", 11.5, p.warn));
    }
    event
}
