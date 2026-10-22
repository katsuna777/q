pub mod employee;
pub mod employees;
pub mod filters;
pub mod journal;
pub mod periods;
pub mod reports;
pub mod settings;
pub mod summary;

use crate::app::Route;
use crate::ui::{self, Kind};
use egui::Ui;
use egui_phosphor::regular as icon;
use otpuska_core::date::Date;
use otpuska_core::model::Leave;
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
    pub settings: settings::State,
}

impl Forms {
    pub fn on_leave(&mut self, route: &Route) {
        if let Route::Settings = route {
            self.settings.sync = true;
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
    pub start: Option<Date>,
    pub base: f64,
    pub extra: f64,
    pub note: String,
}

impl LeaveDraft {
    pub fn for_employee(id: Option<i64>) -> Self {
        LeaveDraft { employee: id, ..Default::default() }
    }

    pub fn from_leave(l: &Leave) -> Self {
        LeaveDraft { id: l.id, employee: Some(l.employee_id), start: l.start, base: l.base_days, extra: l.extra_days, note: l.note.clone() }
    }

    pub fn to_leave(&self) -> Option<Leave> {
        Some(Leave {
            id: self.id,
            employee_id: self.employee?,
            start: self.start,
            base_days: self.base,
            extra_days: self.extra,
            note: self.note.clone(),
        })
    }

    pub fn is_edit(&self) -> bool {
        self.id != 0
    }

    pub fn valid(&self) -> bool {
        self.employee.is_some() && self.base + self.extra > 0.0
    }
}

#[derive(PartialEq, Eq)]
pub enum FormEvent {
    None,
    Submit,
    Cancel,
}

/// One-line form for a journal record. `pick_employee` adds the employee selector.
pub fn leave_form(ui: &mut Ui, snap: &Snapshot, d: &mut LeaveDraft, pick_employee: bool, id: &str) -> FormEvent {
    let p = ui::pal(ui);
    let mut event = FormEvent::None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
        if pick_employee {
            ui::field(ui, "Сотрудник", "", |ui| {
                ui::emp_picker(ui, (id, "emp"), snap, &mut d.employee, "Выберите сотрудника…", 270.0, true);
            });
        }
        ui::field(ui, "Начало отпуска", "", |ui| {
            ui::date_field(ui, (id, "start"), &mut d.start, "дд.мм.гггг", 150.0);
        });
        ui::field(ui, "Основной, дн.", "", |ui| {
            ui::num_field_req(ui, (id, "base"), &mut d.base, "0", 90.0);
        });
        ui::field(ui, "Доп. за вредн., дн.", "", |ui| {
            ui::num_field_req(ui, (id, "extra"), &mut d.extra, "0", 90.0);
        });
        ui::field(ui, "Примечание", "приказ, период и т.п.", |ui| {
            ui::input(ui, &mut d.note, "", if pick_employee { 230.0 } else { 260.0 });
        });
        ui.vertical(|ui| {
            ui.add_space(19.0);
            ui.horizontal(|ui| {
                let (glyph, label) = if d.is_edit() { (icon::CHECK, "Сохранить") } else { (icon::PLUS, "Добавить") };
                let hint = if d.valid() {
                    String::new()
                } else if d.employee.is_none() {
                    "Выберите сотрудника".to_string()
                } else {
                    "Укажите число дней".to_string()
                };
                let r = ui::button(ui, Kind::Primary, Some(glyph), label, d.valid());
                if !hint.is_empty() {
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
    if d.is_edit() {
        ui.add_space(2.0);
        ui.label(ui::text("Изменение существующей записи", 11.5, p.warn));
    }
    event
}
