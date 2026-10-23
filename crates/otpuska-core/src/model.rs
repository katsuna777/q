//! Plain data types shared by the database, the engine and the GUI.

use crate::date::Date;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Status {
    #[default]
    Working,
    Dismissed,
    Suspended,
}

impl Status {
    pub const ALL: [Status; 3] = [Status::Working, Status::Dismissed, Status::Suspended];

    pub fn code(self) -> &'static str {
        match self {
            Status::Working => "work",
            Status::Dismissed => "dismissed",
            Status::Suspended => "suspended",
        }
    }

    pub fn from_code(code: &str) -> Status {
        match code {
            "dismissed" => Status::Dismissed,
            "suspended" => Status::Suspended,
            _ => Status::Working,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Status::Working => "Работает",
            Status::Dismissed => "Уволен",
            Status::Suspended => "Приостановка ТД",
        }
    }

    /// Parses the labels used in the spreadsheet (and a few spellings of them).
    pub fn from_label(label: &str) -> Status {
        let l = label.trim().to_lowercase();
        if l.starts_with("уволен") {
            Status::Dismissed
        } else if l.starts_with("приостановка") {
            Status::Suspended
        } else {
            Status::Working
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Department {
    pub id: i64,
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Employee {
    pub id: i64,
    pub fio: String,
    pub position: String,
    pub department_id: Option<i64>,
    pub hire_date: Option<Date>,
    pub status: Status,
    /// Additional leave for harmful conditions, days per year (`None` = 0).
    pub extra_days: Option<f64>,
    pub suspend_from: Option<Date>,
    pub suspend_to: Option<Date>,
    pub dismiss_date: Option<Date>,
    /// Own base leave, days per year (`None` = the default from the settings).
    pub base_days: Option<f64>,
    /// Count the service from this date instead of the hire date.
    pub accrual_from: Option<Date>,
    pub note: String,
}

/// What kind of leave a journal record is. Only the annual paid leave is written off from the
/// balance; the others are dated periods that matter for the length of the working year
/// (ст. 121 ТК РФ) and for reminders.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum LeaveType {
    /// Ежегодный оплачиваемый отпуск (основной и дополнительный).
    #[default]
    Paid,
    /// Отпуск без сохранения заработной платы (ст. 128): свыше 14 дней в рабочем году не идёт в стаж.
    Unpaid,
    /// Отпуск по беременности и родам (ст. 255): идёт в стаж.
    Maternity,
    /// Отпуск по уходу за ребёнком (ст. 256): в стаж не включается (ст. 121).
    Childcare,
}

impl LeaveType {
    pub const ALL: [LeaveType; 4] = [LeaveType::Paid, LeaveType::Unpaid, LeaveType::Maternity, LeaveType::Childcare];

    pub fn code(self) -> &'static str {
        match self {
            LeaveType::Paid => "paid",
            LeaveType::Unpaid => "unpaid",
            LeaveType::Maternity => "maternity",
            LeaveType::Childcare => "childcare",
        }
    }

    pub fn from_code(code: &str) -> LeaveType {
        match code {
            "unpaid" => LeaveType::Unpaid,
            "maternity" => LeaveType::Maternity,
            "childcare" => LeaveType::Childcare,
            _ => LeaveType::Paid,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LeaveType::Paid => "Ежегодный оплачиваемый",
            LeaveType::Unpaid => "Без сохранения оплаты",
            LeaveType::Maternity => "По беременности и родам",
            LeaveType::Childcare => "По уходу за ребёнком",
        }
    }

    /// For narrow columns.
    pub fn short(self) -> &'static str {
        match self {
            LeaveType::Paid => "Ежегодный",
            LeaveType::Unpaid => "Без оплаты",
            LeaveType::Maternity => "Декретный",
            LeaveType::Childcare => "Уход за ребёнком",
        }
    }

    pub fn is_paid(self) -> bool {
        self == LeaveType::Paid
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Leave {
    pub id: i64,
    pub employee_id: i64,
    pub kind: LeaveType,
    pub start: Option<Date>,
    /// Last day of a leave that is not the annual paid one (those are given by start and dates).
    pub end: Option<Date>,
    pub base_days: f64,
    pub extra_days: f64,
    pub note: String,
}

impl Leave {
    /// Days written off from the annual balance (zero for the other kinds of leave).
    pub fn total_days(&self) -> f64 {
        self.base_days + self.extra_days
    }

    /// Calendar days the record lasts: the entered days for annual leave, `start..=end` for the rest.
    pub fn length(&self) -> f64 {
        match self.kind {
            LeaveType::Paid => self.total_days(),
            _ => match (self.start, self.end) {
                (Some(s), Some(e)) if e >= s => ((e - s).num_days() + 1) as f64,
                _ => 0.0,
            },
        }
    }

    /// What must hold before a record is saved: the other kinds of leave need both dates.
    pub fn check(&self) -> Result<(), &'static str> {
        if self.kind.is_paid() {
            return Ok(());
        }
        match (self.start, self.end) {
            (Some(s), Some(e)) if e >= s => Ok(()),
            (Some(_), Some(_)) => Err("Окончание отпуска раньше его начала"),
            _ => Err("Укажите даты начала и окончания отпуска"),
        }
    }
}

/// A planned annual leave in the vacation schedule (график отпусков, ст. 123 ТК РФ).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    pub id: i64,
    pub employee_id: i64,
    pub start: Option<Date>,
    pub base_days: f64,
    pub extra_days: f64,
    pub note: String,
}

impl Plan {
    pub fn total_days(&self) -> f64 {
        self.base_days + self.extra_days
    }

    /// Last day of the planned leave.
    pub fn end(&self) -> Option<Date> {
        let total = self.total_days();
        match self.start {
            Some(s) if total > 0.0 => Some(s + chrono::Duration::days(total.floor() as i64 - 1)),
            _ => None,
        }
    }
}

/// Persisted calculation settings (the workbook's «Настройки» sheet).
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Fixed "calculate as of" date; `None` means today.
    pub as_of_manual: Option<Date>,
    pub base_days: f64,
    pub first_leave_months: i32,
    pub deadline_months: i32,
    pub burning_days: i32,
    pub soon_days: i32,
    /// Whether a suspension of the employment contract counts towards the service.
    pub suspension_counts: bool,
    /// Reminder before a leave starts: the near stage, in days (the employee has to be told
    /// under signature at least two weeks ahead, ст. 123 ТК РФ).
    pub remind_near_days: i32,
    /// Reminder before a leave starts: the early stage, in months.
    pub remind_far_months: i32,
    /// Reminder before maternity / childcare leave ends, in days.
    pub remind_end_days: i32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            as_of_manual: None,
            base_days: 28.0,
            first_leave_months: 6,
            deadline_months: 12,
            burning_days: 90,
            soon_days: 180,
            suspension_counts: false,
            remind_near_days: 14,
            remind_far_months: 1,
            remind_end_days: 14,
        }
    }
}

/// Settings with the calculation date resolved — what the engine works with.
#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    pub as_of: Date,
    pub base_days: f64,
    pub first_leave_months: i32,
    pub deadline_months: i32,
    pub burning_days: i32,
    pub soon_days: i32,
    pub suspension_counts: bool,
    pub remind_near_days: i32,
    pub remind_far_months: i32,
    pub remind_end_days: i32,
}

impl Settings {
    pub fn params(&self, today: Date) -> Params {
        Params {
            as_of: self.as_of_manual.unwrap_or(today),
            base_days: self.base_days,
            first_leave_months: self.first_leave_months,
            deadline_months: self.deadline_months,
            burning_days: self.burning_days,
            soon_days: self.soon_days,
            suspension_counts: self.suspension_counts,
            remind_near_days: self.remind_near_days,
            remind_far_months: self.remind_far_months,
            remind_end_days: self.remind_end_days,
        }
    }
}

/// Everything stored in the database.
#[derive(Clone, Debug, Default)]
pub struct Data {
    pub settings: Settings,
    pub departments: Vec<Department>,
    pub employees: Vec<Employee>,
    pub leaves: Vec<Leave>,
    /// The vacation schedule.
    pub plans: Vec<Plan>,
}
