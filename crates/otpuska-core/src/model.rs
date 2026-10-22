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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Leave {
    pub id: i64,
    pub employee_id: i64,
    pub start: Option<Date>,
    pub base_days: f64,
    pub extra_days: f64,
    pub note: String,
}

impl Leave {
    pub fn total_days(&self) -> f64 {
        self.base_days + self.extra_days
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
}
