//! The calculation rules of the workbook, one function per sheet formula chain.
//!
//! Terms: a *working year* (период) starts on the hire date (or on the accrual date) and lasts
//! twelve months. Each year gives `base_days + extra_days` of leave. The leave taken is written
//! off from the earliest year first, base and extra separately. The deadline for a year is its
//! end plus `deadline_months`.
//!
//! Some time does not count towards the service (ст. 121 ТК РФ): unpaid leave over 14 calendar
//! days in a working year and childcare leave. The working year then ends later by that many days
//! and the next one starts the day after. Without such leave every date below is exactly the one
//! the workbook gave (`EDATE` of the start), which is what the tests of this module check.

use crate::date::{edate, excel_date, Date};
use crate::model::*;
use chrono::{Datelike, Duration};
use std::collections::HashMap;

// ---- states ------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum State {
    Overdue,
    Burning,
    Soon,
    Normal,
    AllUsed,
    Advance,
    CheckBooks,
    Suspended,
    Dismissed,
    NoRight,
    NoHireDate,
}

impl State {
    pub const ALL: [State; 11] = [
        State::Overdue,
        State::Burning,
        State::Soon,
        State::Normal,
        State::AllUsed,
        State::Advance,
        State::CheckBooks,
        State::Suspended,
        State::Dismissed,
        State::NoRight,
        State::NoHireDate,
    ];

    pub fn label(self) -> &'static str {
        match self {
            State::Overdue => "Просрочено",
            State::Burning => "Горит",
            State::Soon => "Скоро",
            State::Normal => "В норме",
            State::AllUsed => "Всё отгулено",
            State::Advance => "Взято авансом",
            State::CheckBooks => "Проверьте учёт",
            State::Suspended => "Приостановка ТД",
            State::Dismissed => "Уволен",
            State::NoRight => "Право не возникло",
            State::NoHireDate => "Нет даты приема",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            State::Overdue => "Крайний срок за самый старый неотгулянный период уже прошёл",
            State::Burning => "До крайнего срока осталось не больше «горящего» порога",
            State::Soon => "До крайнего срока осталось не больше порога «скоро»",
            State::Normal => "Есть неиспользованные дни, срок далеко",
            State::AllUsed => "Всё положенное за прошедшие периоды отгуляно",
            State::Advance => "Отгулено больше, чем положено",
            State::CheckBooks => "Основной и дополнительный отпуск внесены неверно (например, дни за вредность записаны как основной)",
            State::Suspended => "Трудовой договор приостановлен, отпуск не планируется",
            State::Dismissed => "Не входит в сводку и отчёты, остаётся в справочнике",
            State::NoRight => "Право на первый отпуск ещё не возникло",
            State::NoHireDate => "Укажите дату приема",
        }
    }

    /// Order on the summary: the most urgent first.
    pub fn rank(self) -> u8 {
        match self {
            State::NoHireDate => 0,
            State::Overdue => 1,
            State::Burning => 2,
            State::Soon => 3,
            State::Normal => 4,
            State::Advance | State::CheckBooks => 5,
            State::Suspended => 6,
            State::NoRight => 7,
            State::AllUsed | State::Dismissed => 8,
        }
    }

    /// States that carry a deadline.
    pub fn has_deadline(self) -> bool {
        matches!(self, State::Overdue | State::Burning | State::Soon | State::Normal)
    }

    pub fn needs_attention(self) -> bool {
        matches!(self, State::Overdue | State::Burning | State::Soon)
    }
}

// ---- one employee ------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Calc {
    /// Start of the first working year (hire or accrual date, shifted by a suspension).
    pub accrual_base: Option<Date>,
    pub base_per_year: f64,
    pub extra_per_year: f64,
    /// Full years of service as of the calculation date.
    pub years: Option<i32>,
    /// Working years that have started.
    pub started: i32,
    /// Working years the employee is already entitled to.
    pub available: i32,
    pub taken_base: f64,
    pub taken_extra: f64,
    /// Entitled minus taken, base and extra separately.
    pub left_base: Option<f64>,
    pub left_extra: Option<f64>,
    pub entitled: Option<f64>,
    pub taken: f64,
    pub left: Option<f64>,
    pub state: State,
    /// Number of the earliest working year that still has unused days.
    pub early_period: Option<i32>,
    /// The working years that have started, with their real dates (see `WorkYear`).
    pub work_years: Vec<WorkYear>,
    deadline_raw: Option<Date>,
    days_raw: Option<i64>,
}

impl Calc {
    /// Deadline for the earliest unused year (only for states that have one).
    pub fn deadline(&self) -> Option<Date> {
        self.state.has_deadline().then_some(self.deadline_raw).flatten()
    }

    pub fn days_left(&self) -> Option<i64> {
        self.state.has_deadline().then_some(self.days_raw).flatten()
    }

    /// Remaining extra days, shown only when the employee has any.
    pub fn left_extra_shown(&self) -> Option<f64> {
        if self.entitled.is_none() || (self.extra_per_year == 0.0 && self.taken_extra == 0.0) {
            None
        } else {
            self.left_extra
        }
    }

    /// Dates of the earliest unused working year.
    pub fn early_period_dates(&self) -> Option<(Date, Date)> {
        let k = self.early_period?;
        let y = self.work_years.get(usize::try_from(k.checked_sub(1)?).ok()?)?;
        self.state.has_deadline().then_some((y.start, y.end))
    }

    /// Working year number `k` (1-based), if it has started.
    pub fn work_year(&self, k: i32) -> Option<&WorkYear> {
        usize::try_from(k.checked_sub(1)?).ok().and_then(|i| self.work_years.get(i))
    }
}

// ---- working years with leave that does not count ----------------------------------------------

/// Calendar days of unpaid leave per working year that still count towards the service.
pub const UNPAID_FREE_DAYS: i64 = 14;

/// A dated leave that can change the length of the working years: unpaid and childcare leave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: Date,
    pub end: Date,
    pub kind: LeaveType,
}

/// One working year as it really runs. `start` and `end` are the nominal dates (the anniversary
/// of the start of the accrual) moved by the leave that did not count in the years before and in
/// this one.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkYear {
    pub start: Date,
    pub end: Date,
    /// Calendar days of unpaid leave inside the year (the first 14 of them count, the rest do not).
    pub unpaid: i64,
    /// Calendar days of childcare leave inside the year (none of them counts).
    pub childcare: i64,
    /// Days by which the end of this year moved: unpaid days over 14 plus the childcare days.
    pub shift: i64,
    /// The same for this and all earlier years together: how far the dates are from the nominal ones.
    pub cum: i64,
}

/// The kind of leave that covers a day and does not (fully) count; childcare wins over unpaid.
fn day_kind(d: Date, spans: &[Span]) -> Option<LeaveType> {
    let mut found = None;
    for s in spans {
        if s.start <= d && d <= s.end {
            match s.kind {
                LeaveType::Childcare => return Some(LeaveType::Childcare),
                LeaveType::Unpaid => found = Some(LeaveType::Unpaid),
                _ => {}
            }
        }
    }
    found
}

struct Walk {
    last: Date,
    unpaid: i64,
    childcare: i64,
}

/// Goes forward from `start` until `len` days that count towards the service have passed, so the
/// last day lies `len - 1` days ahead plus the days that did not count on the way. A day is
/// counted once even when records of leave overlap.
fn walk(start: Date, len: i64, spans: &[Span]) -> Walk {
    if spans.is_empty() {
        return Walk { last: start + Duration::days(len - 1), unpaid: 0, childcare: 0 };
    }
    let (mut counted, mut unpaid, mut childcare) = (0, 0, 0);
    let mut d = start;
    // Ends: days outside the leave count, and a leave is finite (the guard is for absurd input).
    for _ in 0..200_000 {
        match day_kind(d, spans) {
            Some(LeaveType::Childcare) => childcare += 1,
            Some(_) => {
                unpaid += 1;
                if unpaid <= UNPAID_FREE_DAYS {
                    counted += 1;
                }
            }
            None => counted += 1,
        }
        if counted >= len {
            break;
        }
        d += Duration::days(1);
    }
    Walk { last: d, unpaid, childcare }
}

const MAX_YEARS: i32 = 200;

/// The working years that have started by `as_of`, counted from `v`.
fn build_years(v: Date, as_of: Date, spans: &[Span]) -> Vec<WorkYear> {
    let mut years = Vec::new();
    let mut cum = 0i64;
    for n in 0..MAX_YEARS {
        // A year has started when its anniversary (29 February rolls to 1 March, as `DATE` does in
        // the workbook) plus the days moved so far has come.
        let anniversary = excel_date(v.year() + n, v.month(), v.day()).unwrap_or(v) + Duration::days(cum);
        if anniversary > as_of {
            break;
        }
        let nominal_start = edate(v, 12 * n);
        let nominal_next = edate(v, 12 * (n + 1));
        let len = (nominal_next - nominal_start).num_days();
        let start = nominal_start + Duration::days(cum);
        let w = walk(start, len, spans);
        let shift = (w.last - (start + Duration::days(len - 1))).num_days();
        cum += shift;
        years.push(WorkYear { start, end: w.last, unpaid: w.unpaid, childcare: w.childcare, shift, cum });
    }
    years
}

/// The day the right to the first leave arises: `months` of service from `v`, later by the time
/// that did not count.
fn first_available(v: Date, months: i32, spans: &[Span]) -> Date {
    let nominal = edate(v, months);
    let len = (nominal - v).num_days();
    if spans.is_empty() || len <= 0 {
        return nominal;
    }
    walk(v, len, spans).last + Duration::days(1)
}

pub fn compute(e: &Employee, taken_base: f64, taken_extra: f64, p: &Params) -> Calc {
    compute_with(e, taken_base, taken_extra, &[], p)
}

/// `spans`: the employee's unpaid and childcare leave (see `Span`).
pub fn compute_with(e: &Employee, taken_base: f64, taken_extra: f64, spans: &[Span], p: &Params) -> Calc {
    let w = e.base_days.unwrap_or(p.base_days);
    let x = e.extra_days.unwrap_or(0.0);

    // Start of the first working year; a suspension that does not count shifts it forward.
    let base = e.hire_date.map(|hire| {
        let start = e.accrual_from.unwrap_or(hire);
        let shift = match e.suspend_from {
            Some(from) if !p.suspension_counts => {
                let to = e.suspend_to.unwrap_or(if e.status == Status::Suspended { p.as_of } else { from });
                (to - from).num_days().max(0)
            }
            _ => 0,
        };
        start + Duration::days(shift)
    });

    let work_years = base.map(|v| build_years(v, p.as_of, spans)).unwrap_or_default();
    let started = work_years.len() as i32;
    // Full working years behind (the current one is not counted).
    let years = base.map(|_| (started - 1).max(0));
    let available = match base {
        Some(v) if started == 1 => i32::from(p.as_of >= first_available(v, p.first_leave_months, spans)),
        Some(_) => started,
        None => 0,
    };
    let av = f64::from(available);

    let (left_base, left_extra) = match base {
        Some(_) => (Some(w * av - taken_base), Some(x * av - taken_extra)),
        None => (None, None),
    };
    let entitled = (base.is_some() && e.status != Status::Dismissed).then(|| (w + x) * av);
    let taken = taken_base + taken_extra;
    let left = entitled.map(|h| h - taken);

    // Earliest working year that still has unused days (base and extra tracked separately).
    let period_of = |per_year: f64, taken: f64, left: Option<f64>| -> Option<i32> {
        match (entitled, left) {
            (Some(_), Some(l)) if available > 0 && per_year > 0.0 && l > 0.0 => Some((taken / per_year).floor() as i32 + 1),
            _ => None,
        }
    };
    let early_period = match (period_of(w, taken_base, left_base), period_of(x, taken_extra, left_extra)) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    let deadline_raw = match (base, early_period) {
        (Some(v), Some(k)) => Some(year_deadline(v, k, work_years.get((k - 1).max(0) as usize).map_or(0, |y| y.cum), p)),
        _ => None,
    };
    let days_raw = deadline_raw.map(|d| (d - p.as_of).num_days());

    let state = if e.status == Status::Dismissed {
        State::Dismissed
    } else if e.hire_date.is_none() {
        State::NoHireDate
    } else if e.status == Status::Suspended {
        State::Suspended
    } else if available == 0 {
        State::NoRight
    } else if left_base.is_some_and(|v| v < 0.0) || left_extra.is_some_and(|v| v < 0.0) {
        if left.is_some_and(|j| j < 0.0) {
            State::Advance
        } else {
            State::CheckBooks
        }
    } else if left == Some(0.0) {
        State::AllUsed
    } else {
        match days_raw {
            Some(d) if d < 0 => State::Overdue,
            Some(d) if d <= i64::from(p.burning_days) => State::Burning,
            Some(d) if d <= i64::from(p.soon_days) => State::Soon,
            _ => State::Normal,
        }
    };

    Calc {
        accrual_base: base,
        base_per_year: w,
        extra_per_year: x,
        years,
        started,
        available,
        taken_base,
        taken_extra,
        left_base,
        left_extra,
        entitled,
        taken,
        left,
        state,
        early_period,
        work_years,
        deadline_raw,
        days_raw,
    }
}

/// The working years a dated leave overlaps, as they would be with that leave added — to show
/// what an unpaid or childcare leave changes before it is saved. `skip` is the record being edited
/// (it is left out of the calculation).
pub fn leave_effect(snap: &Snapshot, emp_id: i64, skip: i64, kind: LeaveType, start: Date, end: Date) -> Vec<WorkYear> {
    let Some(r) = snap.emp_by_id(emp_id) else { return Vec::new() };
    let mut spans: Vec<Span> = snap
        .leaves
        .iter()
        .filter(|l| l.leave.employee_id == emp_id && l.leave.id != skip && matches!(l.leave.kind, LeaveType::Unpaid | LeaveType::Childcare))
        .filter_map(|l| Some(Span { start: l.leave.start?, end: l.end?, kind: l.leave.kind }))
        .collect();
    if matches!(kind, LeaveType::Unpaid | LeaveType::Childcare) {
        spans.push(Span { start, end, kind });
    }
    // Look as far as the end of the leave, also when it lies in the future.
    let params = Params { as_of: snap.params.as_of.max(end), ..snap.params.clone() };
    let c = compute_with(&r.emp, r.calc.taken_base, r.calc.taken_extra, &spans, &params);
    c.work_years.into_iter().filter(|y| y.start <= end && y.end >= start).collect()
}

/// Last day to use the leave of working year `k`: its end plus `deadline_months` (`cum` — how far
/// the dates of that year were moved by leave that did not count).
fn year_deadline(v: Date, k: i32, cum: i64, p: &Params) -> Date {
    edate(v, 12 * k + p.deadline_months) - Duration::days(1) + Duration::days(cum)
}

// ---- working years -----------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeriodTone {
    /// Not yet entitled (the first year before the waiting period passed).
    Future,
    Suspended,
    Used,
    Overdue,
    Burning,
    Soon,
    Open,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PeriodRow {
    pub k: i32,
    pub start: Date,
    pub end: Date,
    pub available: bool,
    pub entitled: f64,
    pub taken: Option<f64>,
    pub left: Option<f64>,
    pub left_extra: Option<f64>,
    pub deadline: Date,
    pub days_to_deadline: i64,
    pub tone: PeriodTone,
    /// Days by which the end of this year was moved by leave that does not count (0 — as usual).
    pub shift: i64,
    /// Calendar days of unpaid leave inside the year.
    pub unpaid: i64,
    /// Calendar days of childcare leave inside the year.
    pub childcare: i64,
}

/// Working year number `k` (1-based) of an employee, if it has started.
pub fn period(e: &Employee, c: &Calc, p: &Params, k: i32) -> Option<PeriodRow> {
    if e.status == Status::Dismissed {
        return None;
    }
    let v = c.accrual_base?;
    if k < 1 || k > c.started {
        return None;
    }
    let year = c.work_year(k)?;
    let (start, end) = (year.start, year.end);
    let (w, x) = (c.base_per_year, c.extra_per_year);
    let available = k <= c.available;
    let left_of = |per_year: f64, taken: f64| per_year - per_year.min((taken - per_year * f64::from(k - 1)).max(0.0));
    let (lb, le) = (left_of(w, c.taken_base), left_of(x, c.taken_extra));
    let left = available.then_some(lb + le);
    let deadline = year_deadline(v, k, year.cum, p);
    let days = (deadline - p.as_of).num_days();
    let tone = if e.status == Status::Suspended {
        PeriodTone::Suspended
    } else if !available {
        PeriodTone::Future
    } else {
        match left {
            Some(l) if l == 0.0 => PeriodTone::Used,
            Some(_) if days < 0 => PeriodTone::Overdue,
            Some(_) if days <= i64::from(p.burning_days) => PeriodTone::Burning,
            Some(_) if days <= i64::from(p.soon_days) => PeriodTone::Soon,
            _ => PeriodTone::Open,
        }
    };
    Some(PeriodRow {
        k,
        start,
        end,
        available,
        entitled: w + x,
        taken: left.map(|l| w + x - l),
        left,
        left_extra: (available && x > 0.0).then_some(le),
        deadline,
        days_to_deadline: days,
        tone,
        shift: year.shift,
        unpaid: year.unpaid,
        childcare: year.childcare,
    })
}

/// The working year that started in calendar year `year`.
pub fn period_for_year(e: &Employee, c: &Calc, p: &Params, year: i32) -> Option<PeriodRow> {
    let k = c.work_years.iter().position(|y| y.start.year() == year)? as i32 + 1;
    period(e, c, p, k)
}

pub fn periods(e: &Employee, c: &Calc, p: &Params) -> Vec<PeriodRow> {
    (1..=c.started).filter_map(|k| period(e, c, p, k)).collect()
}

// ---- journal rows --------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaveStatus {
    Undated,
    Planned,
    InLeave,
    Done,
}

impl LeaveStatus {
    pub fn label(self) -> &'static str {
        match self {
            LeaveStatus::Undated => "Без даты",
            LeaveStatus::Planned => "Запланирован",
            LeaveStatus::InLeave => "В отпуске",
            LeaveStatus::Done => "Завершён",
        }
    }
}

pub fn leave_end(l: &Leave) -> Option<Date> {
    if !l.kind.is_paid() {
        return match (l.start, l.end) {
            (Some(s), Some(e)) if e >= s => Some(e),
            _ => None,
        };
    }
    let total = l.total_days();
    match l.start {
        Some(s) if total > 0.0 => Some(s + Duration::days(total.floor() as i64 - 1)),
        _ => None,
    }
}

/// `None` for an empty record (no days).
pub fn leave_status(l: &Leave, p: &Params) -> Option<LeaveStatus> {
    if l.length() == 0.0 {
        return None;
    }
    Some(match (l.start, leave_end(l)) {
        (Some(s), _) if s > p.as_of => LeaveStatus::Planned,
        (Some(_), Some(end)) if end >= p.as_of => LeaveStatus::InLeave,
        (Some(_), _) => LeaveStatus::Done,
        (None, _) => LeaveStatus::Undated,
    })
}

/// `14` → "14", `14.5` → "14,5".
pub fn fmt_days(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.1}").replace('.', ",")
    }
}

pub fn fmt_days_opt(v: Option<f64>) -> String {
    v.map(fmt_days).unwrap_or_default()
}

// ---- the whole picture -----------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct EmpRow {
    pub emp: Employee,
    pub calc: Calc,
    pub dept: String,
    pub leave_count: usize,
}

#[derive(Clone, Debug)]
pub struct LeaveRow {
    pub leave: Leave,
    pub emp_ix: Option<usize>,
    pub fio: String,
    pub dept: String,
    pub end: Option<Date>,
    pub status: Option<LeaveStatus>,
    pub emp_left: Option<f64>,
    pub emp_dismissed: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Counts {
    pub overdue: usize,
    pub burning: usize,
    pub soon: usize,
    pub ok: usize,
    pub suspended: usize,
    pub on_leave: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeptReport {
    pub name: String,
    pub attention: usize,
    pub days_left: f64,
    pub working: usize,
    pub overdue: usize,
    pub burning: usize,
    pub soon: usize,
    pub suspended: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Warn,
    Info,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Warning {
    pub severity: Severity,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub params: Params,
    pub settings: Settings,
    pub departments: Vec<Department>,
    pub emps: Vec<EmpRow>,
    pub leaves: Vec<LeaveRow>,
    /// The vacation schedule with its state against the journal (see `schedule.rs`).
    pub plans: Vec<crate::schedule::PlanRow>,
    /// What to remind about today (see `schedule.rs`), most urgent first.
    pub reminders: Vec<crate::schedule::Reminder>,
    index: HashMap<i64, usize>,
}

pub const PLACEHOLDER_PREFIX: &str = "Без ФИО";

/// "сотрудника" after 1, 21, 31… and "сотрудников" otherwise (genitive after «у N»).
pub fn staff_gen(n: usize) -> &'static str {
    if n % 10 == 1 && n % 100 != 11 {
        "сотрудника"
    } else {
        "сотрудников"
    }
}

impl Snapshot {
    pub fn build(data: &Data, today: Date) -> Snapshot {
        let params = data.settings.params(today);
        let dept_names: HashMap<i64, &str> = data.departments.iter().map(|d| (d.id, d.name.as_str())).collect();
        let dept_of = |id: Option<i64>| id.and_then(|i| dept_names.get(&i).copied()).unwrap_or("").to_string();

        // Annual leave is written off from the balance; unpaid and childcare leave change the
        // length of the working years instead.
        let mut taken: HashMap<i64, (f64, f64, usize)> = HashMap::new();
        let mut spans: HashMap<i64, Vec<Span>> = HashMap::new();
        for l in &data.leaves {
            let t = taken.entry(l.employee_id).or_default();
            t.2 += 1;
            match l.kind {
                LeaveType::Paid => {
                    t.0 += l.base_days;
                    t.1 += l.extra_days;
                }
                LeaveType::Unpaid | LeaveType::Childcare => {
                    if let (Some(start), Some(end)) = (l.start, leave_end(l)) {
                        spans.entry(l.employee_id).or_default().push(Span { start, end, kind: l.kind });
                    }
                }
                LeaveType::Maternity => {}
            }
        }

        let mut index = HashMap::new();
        let emps: Vec<EmpRow> = data
            .employees
            .iter()
            .enumerate()
            .map(|(i, e)| {
                index.insert(e.id, i);
                let (tb, te, n) = taken.get(&e.id).copied().unwrap_or_default();
                let own = spans.get(&e.id).map(Vec::as_slice).unwrap_or(&[]);
                EmpRow { calc: compute_with(e, tb, te, own, &params), dept: dept_of(e.department_id), leave_count: n, emp: e.clone() }
            })
            .collect();

        let leaves: Vec<LeaveRow> = data
            .leaves
            .iter()
            .map(|l| {
                let ix = index.get(&l.employee_id).copied();
                let row = ix.map(|i| &emps[i]);
                LeaveRow {
                    leave: l.clone(),
                    emp_ix: ix,
                    fio: row.map(|r| r.emp.fio.clone()).unwrap_or_default(),
                    dept: row.map(|r| r.dept.clone()).unwrap_or_default(),
                    end: leave_end(l),
                    status: leave_status(l, &params),
                    emp_left: row.and_then(|r| r.calc.left),
                    emp_dismissed: row.is_some_and(|r| r.emp.status == Status::Dismissed),
                }
            })
            .collect();

        let plans = crate::schedule::match_plans(&data.plans, &emps, &leaves, &index, params.as_of);
        let mut snap = Snapshot { params, settings: data.settings.clone(), departments: data.departments.clone(), emps, leaves, plans, reminders: Vec::new(), index };
        snap.reminders = snap.compute_reminders();
        snap
    }

    pub fn emp_by_id(&self, id: i64) -> Option<&EmpRow> {
        self.index.get(&id).map(|&i| &self.emps[i])
    }

    pub fn emp_ix(&self, id: i64) -> Option<usize> {
        self.index.get(&id).copied()
    }

    pub fn dept_name(&self, id: Option<i64>) -> &str {
        id.and_then(|i| self.departments.iter().find(|d| d.id == i)).map(|d| d.name.as_str()).unwrap_or("")
    }

    pub fn active_count(&self) -> usize {
        self.emps.iter().filter(|r| r.emp.status != Status::Dismissed).count()
    }

    fn in_dept(r: &EmpRow, filter: Option<i64>) -> bool {
        filter.map_or(true, |id| r.emp.department_id == Some(id))
    }

    /// Counters on top of the summary.
    pub fn counts(&self, filter: Option<i64>) -> Counts {
        let mut c = Counts::default();
        for r in self.emps.iter().filter(|r| Self::in_dept(r, filter)) {
            match r.calc.state {
                State::Overdue => c.overdue += 1,
                State::Burning => c.burning += 1,
                State::Soon => c.soon += 1,
                State::Normal | State::AllUsed => c.ok += 1,
                State::Suspended => c.suspended += 1,
                _ => {}
            }
        }
        c.on_leave = self
            .leaves
            .iter()
            .filter(|l| l.status == Some(LeaveStatus::InLeave) && !l.emp_dismissed)
            .filter(|l| filter.map_or(true, |id| l.emp_ix.is_some_and(|i| self.emps[i].emp.department_id == Some(id))))
            .count();
        c
    }

    /// Indexes into `emps`: active employees, most urgent first.
    pub fn summary_order(&self, filter: Option<i64>) -> Vec<usize> {
        let mut rows: Vec<(u8, i64, usize)> = self
            .emps
            .iter()
            .enumerate()
            .filter(|(_, r)| r.emp.status != Status::Dismissed && Self::in_dept(r, filter))
            .map(|(i, r)| (r.calc.state.rank(), r.calc.days_left().unwrap_or(0), i))
            .collect();
        rows.sort();
        rows.into_iter().map(|(_, _, i)| i).collect()
    }

    /// Departments (in list order, then employees without one) plus the total row.
    pub fn dept_report(&self, only: Option<i64>) -> (Vec<DeptReport>, DeptReport) {
        let mut out: Vec<DeptReport> = Vec::new();
        let mut groups: Vec<(String, Option<i64>)> =
            self.departments.iter().filter(|d| only.map_or(true, |id| d.id == id)).map(|d| (d.name.clone(), Some(d.id))).collect();
        if only.is_none() && self.emps.iter().any(|r| r.emp.department_id.is_none() && r.emp.status != Status::Dismissed) {
            groups.push(("Без подразделения".to_string(), None));
        }
        for (name, id) in groups {
            let mut d = DeptReport { name, ..Default::default() };
            for r in self.emps.iter().filter(|r| r.emp.department_id == id) {
                if r.emp.status != Status::Dismissed {
                    d.days_left += r.calc.left.filter(|l| *l > 0.0).unwrap_or(0.0);
                    if r.emp.status != Status::Suspended {
                        d.working += 1;
                    }
                }
                match r.calc.state {
                    State::Overdue => d.overdue += 1,
                    State::Burning => d.burning += 1,
                    State::Soon => d.soon += 1,
                    _ => {}
                }
                if r.emp.status == Status::Suspended {
                    d.suspended += 1;
                }
            }
            d.attention = d.overdue + d.burning + d.soon;
            out.push(d);
        }
        let mut total = DeptReport { name: "ВСЕГО".into(), ..Default::default() };
        for d in &out {
            total.attention += d.attention;
            total.days_left += d.days_left;
            total.working += d.working;
            total.overdue += d.overdue;
            total.burning += d.burning;
            total.soon += d.soon;
            total.suspended += d.suspended;
        }
        (out, total)
    }

    /// Indexes into `leaves`: ongoing and planned leaves, nearest first.
    pub fn upcoming(&self) -> Vec<usize> {
        let mut rows: Vec<(Date, i64, usize)> = self
            .leaves
            .iter()
            .enumerate()
            .filter(|(_, l)| matches!(l.status, Some(LeaveStatus::InLeave | LeaveStatus::Planned)) && !l.emp_dismissed)
            .filter_map(|(i, l)| l.leave.start.map(|s| (s, l.leave.id, i)))
            .collect();
        rows.sort();
        rows.into_iter().map(|(_, _, i)| i).collect()
    }

    pub fn warnings(&self) -> Vec<Warning> {
        let mut out = Vec::new();
        if self.emps.is_empty() {
            return out;
        }
        if self.leaves.is_empty() {
            out.push(Warning {
                severity: Severity::Warn,
                text: "Журнал отпусков пуст: отпуска ещё не внесены, поэтому остатки завышены, а «Просрочено» и «Горит» показаны без учёта фактических отпусков.".into(),
            });
        }
        let placeholders = self.emps.iter().filter(|r| r.emp.fio.starts_with(PLACEHOLDER_PREFIX)).count();
        if placeholders > 0 {
            out.push(Warning {
                severity: Severity::Info,
                text: format!("У {placeholders} {} ФИО ещё не вписаны (в списке они помечены «{PLACEHOLDER_PREFIX} …»).", staff_gen(placeholders)),
            });
        }
        let check = self.emps.iter().filter(|r| r.calc.state == State::CheckBooks).count();
        if check > 0 {
            out.push(Warning {
                severity: Severity::Warn,
                text: format!("Проверьте учёт у {check} {}: основной и дополнительный отпуск внесены неверно.", staff_gen(check)),
            });
        }
        let no_date = self.emps.iter().filter(|r| r.calc.state == State::NoHireDate).count();
        if no_date > 0 {
            out.push(Warning { severity: Severity::Warn, text: format!("У {no_date} {} не указана дата приема — расчёт невозможен.", staff_gen(no_date)) });
        }
        out
    }

    /// First year of the period window shown by default: the last five years up to today.
    pub fn default_first_year(&self) -> i32 {
        self.params.as_of.year() - 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> Date {
        Date::from_ymd_opt(y, m, day).unwrap()
    }

    fn params(as_of: Date) -> Params {
        Settings::default().params(as_of)
    }

    fn emp(hire: Date) -> Employee {
        Employee { id: 1, fio: "Иванов И. И.".into(), hire_date: Some(hire), ..Default::default() }
    }

    #[test]
    fn long_service_without_leaves_is_overdue() {
        let p = params(d(2026, 10, 22));
        let c = compute(&emp(d(2022, 9, 28)), 0.0, 0.0, &p);
        assert_eq!(c.years, Some(4));
        assert_eq!((c.started, c.available), (5, 5));
        assert_eq!(c.entitled, Some(140.0));
        assert_eq!(c.left, Some(140.0));
        assert_eq!(c.early_period, Some(1));
        assert_eq!(c.deadline(), Some(d(2024, 9, 27)));
        assert_eq!(c.days_left(), Some(-755));
        assert_eq!(c.state, State::Overdue);
    }

    #[test]
    fn taken_days_move_the_early_period() {
        let p = params(d(2026, 10, 22));
        let c = compute(&emp(d(2022, 9, 28)), 28.0, 0.0, &p);
        assert_eq!(c.early_period, Some(2));
        assert_eq!(c.deadline(), Some(d(2025, 9, 27)));
        assert_eq!(c.state, State::Overdue);
        assert_eq!(c.early_period_dates(), Some((d(2023, 9, 28), d(2024, 9, 27))));
    }

    #[test]
    fn first_year_waits_for_the_period() {
        let p = params(d(2026, 10, 22));
        let c = compute(&emp(d(2026, 7, 1)), 0.0, 0.0, &p);
        assert_eq!((c.started, c.available), (1, 0));
        assert_eq!(c.state, State::NoRight);
        // After six months the first year becomes available.
        let c = compute(&emp(d(2026, 7, 1)), 0.0, 0.0, &params(d(2027, 1, 1)));
        assert_eq!((c.started, c.available), (1, 1));
        assert_eq!(c.left, Some(28.0));
        assert_eq!(c.state, State::Normal);
    }

    #[test]
    fn all_used_and_advance() {
        let p = params(d(2024, 12, 1));
        let e = emp(d(2024, 6, 1));
        assert_eq!(compute(&e, 28.0, 0.0, &p).state, State::AllUsed);
        assert_eq!(compute(&e, 35.0, 0.0, &p).state, State::Advance);
    }

    #[test]
    fn extra_days_booked_as_base_need_a_check() {
        // 28 base + 7 extra per year, one year available; 35 booked as base days.
        let p = params(d(2024, 12, 1));
        let mut e = emp(d(2024, 6, 1));
        e.extra_days = Some(7.0);
        let c = compute(&e, 35.0, 0.0, &p);
        assert_eq!(c.left, Some(0.0));
        assert_eq!(c.left_base, Some(-7.0));
        assert_eq!(c.state, State::CheckBooks);
        assert_eq!(compute(&e, 28.0, 7.0, &p).state, State::AllUsed);
    }

    #[test]
    fn thresholds_burning_and_soon() {
        // Hired 2024-01-10 -> first year ends 2025-01-09, deadline 2026-01-09.
        let e = emp(d(2024, 1, 10));
        let state_at = |as_of| compute(&e, 0.0, 0.0, &params(as_of)).state;
        assert_eq!(state_at(d(2026, 1, 10)), State::Overdue);
        assert_eq!(state_at(d(2026, 1, 9)), State::Burning); // 0 days left
        assert_eq!(state_at(d(2025, 10, 11)), State::Burning); // 90 days
        assert_eq!(state_at(d(2025, 10, 10)), State::Soon); // 91 days
        assert_eq!(state_at(d(2025, 7, 13)), State::Soon); // 180 days
        assert_eq!(state_at(d(2025, 7, 12)), State::Normal); // 181 days
    }

    #[test]
    fn dismissed_and_missing_dates() {
        let p = params(d(2026, 10, 22));
        let mut e = emp(d(2022, 9, 28));
        e.status = Status::Dismissed;
        let c = compute(&e, 0.0, 0.0, &p);
        assert_eq!(c.state, State::Dismissed);
        assert_eq!(c.entitled, None);
        assert_eq!(c.left, None);
        assert!(period(&e, &c, &p, 1).is_none());
        let nd = Employee { id: 2, fio: "Без даты".into(), ..Default::default() };
        assert_eq!(compute(&nd, 0.0, 0.0, &p).state, State::NoHireDate);
    }

    #[test]
    fn suspension_freezes_service_unless_it_counts() {
        let mut e = emp(d(2022, 10, 19));
        e.status = Status::Suspended;
        e.suspend_from = Some(d(2025, 3, 1));
        let p = params(d(2026, 10, 22));
        let c = compute(&e, 0.0, 0.0, &p);
        assert_eq!(c.state, State::Suspended);
        // Shift = days from the start of the suspension until the calculation date.
        let shift = (d(2026, 10, 22) - d(2025, 3, 1)).num_days();
        assert_eq!(c.accrual_base, Some(d(2022, 10, 19) + Duration::days(shift)));

        // With a return date the shift is fixed.
        let mut back = e.clone();
        back.status = Status::Working;
        back.suspend_to = Some(d(2025, 4, 1));
        let c = compute(&back, 0.0, 0.0, &p);
        assert_eq!(c.accrual_base, Some(d(2022, 10, 19) + Duration::days(31)));

        // "Counts towards the service" switches the shift off.
        let mut s = Settings::default();
        s.suspension_counts = true;
        let c = compute(&back, 0.0, 0.0, &s.params(d(2026, 10, 22)));
        assert_eq!(c.accrual_base, Some(d(2022, 10, 19)));
    }

    #[test]
    fn own_base_days_and_accrual_date() {
        let mut e = emp(d(2022, 9, 28));
        e.base_days = Some(31.0);
        e.accrual_from = Some(d(2023, 1, 1));
        let c = compute(&e, 0.0, 0.0, &params(d(2026, 10, 22)));
        assert_eq!(c.accrual_base, Some(d(2023, 1, 1)));
        assert_eq!(c.base_per_year, 31.0);
        assert_eq!(c.started, 4);
        assert_eq!(c.entitled, Some(124.0));
    }

    #[test]
    fn leap_day_anniversary_rolls_to_march() {
        // Hired 29 Feb 2024; on 28 Feb 2025 the anniversary (1 Mar in Excel) has not come yet.
        let e = emp(d(2024, 2, 29));
        assert_eq!(compute(&e, 0.0, 0.0, &params(d(2025, 2, 28))).years, Some(0));
        assert_eq!(compute(&e, 0.0, 0.0, &params(d(2025, 3, 1))).years, Some(1));
    }

    #[test]
    fn period_rows_write_off_oldest_first() {
        let p = params(d(2026, 10, 22));
        let mut e = emp(d(2022, 9, 28));
        e.extra_days = Some(7.0);
        let c = compute(&e, 40.0, 7.0, &p);
        let rows = periods(&e, &c, &p);
        assert_eq!(rows.len(), 5);
        // Year 1: base 28 fully used, extra 7 used -> nothing left.
        assert_eq!(rows[0].left, Some(0.0));
        assert_eq!(rows[0].tone, PeriodTone::Used);
        // Year 2: base 12 of 28 used -> 16 base left + 7 extra left.
        assert_eq!(rows[1].left, Some(23.0));
        assert_eq!(rows[1].left_extra, Some(7.0));
        assert_eq!(rows[1].taken, Some(12.0));
        assert_eq!(rows[1].tone, PeriodTone::Overdue);
        assert_eq!(rows[4].k, 5);
        assert_eq!(rows[4].tone, PeriodTone::Open);
        assert_eq!(period_for_year(&e, &c, &p, 2023).map(|r| r.k), Some(2));
        assert!(period_for_year(&e, &c, &p, 2021).is_none());
    }

    #[test]
    fn leave_statuses() {
        let p = params(d(2026, 10, 22));
        let mk = |start: Option<Date>, days: f64| Leave { id: 1, employee_id: 1, start, base_days: days, ..Default::default() };
        assert_eq!(leave_status(&mk(Some(d(2026, 11, 1)), 14.0), &p), Some(LeaveStatus::Planned));
        assert_eq!(leave_status(&mk(Some(d(2026, 10, 20)), 14.0), &p), Some(LeaveStatus::InLeave));
        assert_eq!(leave_status(&mk(Some(d(2026, 10, 9)), 14.0), &p), Some(LeaveStatus::InLeave)); // ends 22 Oct
        assert_eq!(leave_status(&mk(Some(d(2026, 10, 8)), 14.0), &p), Some(LeaveStatus::Done)); // ends 21 Oct
        assert_eq!(leave_status(&mk(None, 14.0), &p), Some(LeaveStatus::Undated));
        assert_eq!(leave_status(&mk(Some(d(2026, 10, 20)), 0.0), &p), None);
        assert_eq!(leave_end(&mk(Some(d(2026, 6, 1)), 14.0)), Some(d(2026, 6, 14)));
    }

    #[test]
    fn snapshot_summary_and_reports() {
        let data = Data {
            settings: Settings { as_of_manual: Some(d(2026, 10, 22)), ..Default::default() },
            departments: vec![Department { id: 1, name: "Цех".into() }, Department { id: 2, name: "Офис".into() }],
            employees: vec![
                Employee { id: 1, fio: "А".into(), department_id: Some(1), hire_date: Some(d(2022, 9, 28)), ..Default::default() },
                Employee { id: 2, fio: "Б".into(), department_id: Some(2), hire_date: Some(d(2026, 7, 1)), ..Default::default() },
                Employee { id: 3, fio: "В".into(), department_id: Some(2), hire_date: Some(d(2020, 1, 1)), status: Status::Dismissed, ..Default::default() },
                Employee { id: 4, fio: "Г".into(), department_id: Some(1), hire_date: Some(d(2025, 3, 3)), ..Default::default() },
            ],
            leaves: vec![
                Leave { id: 1, employee_id: 4, start: Some(d(2026, 10, 20)), base_days: 14.0, ..Default::default() },
                Leave { id: 2, employee_id: 4, start: Some(d(2026, 12, 1)), base_days: 14.0, ..Default::default() },
                Leave { id: 3, employee_id: 3, start: Some(d(2026, 10, 20)), base_days: 14.0, ..Default::default() },
            ],
            ..Default::default()
        };
        let s = Snapshot::build(&data, d(2030, 1, 1)); // manual date wins over "today"
        assert_eq!(s.params.as_of, d(2026, 10, 22));
        assert_eq!(s.active_count(), 3);
        let order = s.summary_order(None);
        assert_eq!(order.len(), 3);
        assert_eq!(s.emps[order[0]].emp.fio, "А"); // overdue first
        assert_eq!(s.emps[order[2]].emp.fio, "Б"); // no right yet last
        assert_eq!(s.summary_order(Some(2)).len(), 1);
        let c = s.counts(None);
        assert_eq!((c.overdue, c.on_leave), (1, 1)); // dismissed employee's leave is not counted
        assert_eq!(s.upcoming().len(), 2);
        let (rows, total) = s.dept_report(None);
        assert_eq!(s.dept_report(Some(2)).0.len(), 1);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].working, 2);
        assert_eq!(total.working, 3);
        assert_eq!(total.attention, rows[0].attention + rows[1].attention);
        assert_eq!(crate::date::fmt(d(2026, 10, 22)), "22.10.2026");
    }

    // ---- leave that does not count towards the service ------------------------------------------------

    fn span(kind: LeaveType, start: Date, end: Date) -> Span {
        Span { start, end, kind }
    }

    /// The formulas of the first version, written out once more: with no unpaid or childcare leave
    /// the new code has to give exactly the same dates, also on 29 February and at month ends.
    #[test]
    fn without_such_leave_every_date_is_what_the_old_formulas_gave() {
        let old = |v: Date, as_of: Date, first_months: i32, deadline_months: i32| {
            let years = if v > as_of {
                0
            } else {
                let anniversary = excel_date(as_of.year(), v.month(), v.day()).unwrap_or(as_of);
                as_of.year() - v.year() - i32::from(anniversary > as_of)
            };
            let started = if v > as_of { 0 } else { years + 1 };
            let available = if started == 0 {
                0
            } else if years == 0 {
                i32::from(as_of >= edate(v, first_months))
            } else {
                started
            };
            let bounds: Vec<(Date, Date, Date)> = (1..=started)
                .map(|k| (edate(v, 12 * (k - 1)), edate(v, 12 * k) - Duration::days(1), edate(v, 12 * k + deadline_months) - Duration::days(1)))
                .collect();
            (years, started, available, bounds)
        };
        let mut hires = vec![d(2016, 2, 29), d(2020, 2, 29), d(2024, 2, 29), d(2023, 1, 31), d(2022, 8, 31), d(2025, 12, 31), d(2024, 12, 31)];
        let mut day = d(2018, 1, 1);
        while day < d(2026, 12, 1) {
            hires.push(day);
            day += Duration::days(37);
        }
        let as_ofs = [d(2024, 2, 28), d(2025, 2, 28), d(2025, 3, 1), d(2026, 10, 23), d(2027, 3, 15), d(2029, 2, 28), d(2029, 3, 1), d(2017, 6, 1)];
        for v in hires {
            for as_of in as_ofs {
                for (first, deadline) in [(6, 12), (3, 18)] {
                    let p = Params { first_leave_months: first, deadline_months: deadline, ..params(as_of) };
                    let c = compute(&emp(v), 0.0, 0.0, &p);
                    let (years, started, available, bounds) = old(v, as_of, first, deadline);
                    assert_eq!((c.years, c.started, c.available), (Some(years), started, available), "hired {v}, as of {as_of}");
                    let got: Vec<(Date, Date)> = c.work_years.iter().map(|y| (y.start, y.end)).collect();
                    assert_eq!(got, bounds.iter().map(|b| (b.0, b.1)).collect::<Vec<_>>(), "hired {v}, as of {as_of}");
                    for (k, b) in bounds.iter().enumerate() {
                        assert_eq!(year_deadline(v, k as i32 + 1, 0, &p), b.2);
                    }
                    assert!(c.work_years.iter().all(|y| y.shift == 0 && y.cum == 0));
                }
            }
        }
    }

    #[test]
    fn unpaid_leave_up_to_14_days_a_year_changes_nothing() {
        let p = params(d(2026, 10, 22));
        let e = emp(d(2022, 9, 28));
        let plain = compute(&e, 0.0, 0.0, &p);
        let c = compute_with(&e, 0.0, 0.0, &[span(LeaveType::Unpaid, d(2023, 1, 10), d(2023, 1, 23))], &p); // exactly 14 days
        assert_eq!(c.work_years, plain.work_years.iter().cloned().map(|mut y| { y.unpaid = if y.start == d(2022, 9, 28) { 14 } else { 0 }; y }).collect::<Vec<_>>());
        assert_eq!(c.deadline(), plain.deadline());
    }

    #[test]
    fn unpaid_leave_over_14_days_moves_the_end_of_the_year_and_the_next_year_starts_after_it() {
        // 20 days of unpaid leave inside the first year: 6 days do not count.
        let p = params(d(2026, 10, 22));
        let e = emp(d(2022, 9, 28));
        let unpaid = [span(LeaveType::Unpaid, d(2023, 1, 10), d(2023, 1, 29))];
        let c = compute_with(&e, 0.0, 0.0, &unpaid, &p);
        let y1 = &c.work_years[0];
        assert_eq!((y1.start, y1.end, y1.unpaid, y1.shift), (d(2022, 9, 28), d(2023, 10, 3), 20, 6));
        let y2 = &c.work_years[1];
        assert_eq!((y2.start, y2.end, y2.shift, y2.cum), (d(2023, 10, 4), d(2024, 10, 3), 0, 6), "the next year starts the day after, and the later years follow");
        assert_eq!(c.work_years[4].start, d(2026, 10, 4));
        // On the old anniversary the second year has not started yet.
        let before = compute_with(&e, 0.0, 0.0, &unpaid, &params(d(2023, 10, 3)));
        assert_eq!((before.started, before.years), (1, Some(0)));
        let after = compute_with(&e, 0.0, 0.0, &unpaid, &params(d(2023, 10, 4)));
        assert_eq!((after.started, after.years), (2, Some(1)));
        // The deadline of the first year moves with it.
        assert_eq!(c.early_period, Some(1));
        assert_eq!(c.deadline(), Some(d(2024, 9, 27) + Duration::days(6)));
        assert_eq!(c.early_period_dates(), Some((d(2022, 9, 28), d(2023, 10, 3))));
        // Today is 22 Oct 2026, the fifth year started on 4 Oct 2026.
        assert_eq!(c.started, 5);
        let rows = periods(&e, &c, &p);
        assert_eq!((rows[0].shift, rows[0].unpaid, rows[1].shift), (6, 20, 0));
    }

    #[test]
    fn the_fifteenth_day_is_the_first_that_does_not_count() {
        let p = params(d(2026, 10, 22));
        let e = emp(d(2022, 9, 28));
        let shift_for = |days: i64| {
            let u = [span(LeaveType::Unpaid, d(2023, 2, 1), d(2023, 2, 1) + Duration::days(days - 1))];
            compute_with(&e, 0.0, 0.0, &u, &p).work_years[0].shift
        };
        assert_eq!((shift_for(14), shift_for(15), shift_for(30)), (0, 1, 16));
    }

    #[test]
    fn separate_and_overlapping_records_count_by_days() {
        let p = params(d(2026, 10, 22));
        let e = emp(d(2022, 9, 28));
        // Two records of 10 days each in one year: 20 days together.
        let two = [span(LeaveType::Unpaid, d(2023, 1, 10), d(2023, 1, 19)), span(LeaveType::Unpaid, d(2023, 5, 1), d(2023, 5, 10))];
        assert_eq!(compute_with(&e, 0.0, 0.0, &two, &p).work_years[0].shift, 6);
        // The same days entered twice are still the same days.
        let twice = [span(LeaveType::Unpaid, d(2023, 1, 10), d(2023, 1, 29)), span(LeaveType::Unpaid, d(2023, 1, 20), d(2023, 1, 29))];
        assert_eq!(compute_with(&e, 0.0, 0.0, &twice, &p).work_years[0].shift, 6);
        // A leave across the border of two years is counted in each year separately: 8 + 13 days.
        let across = [span(LeaveType::Unpaid, d(2023, 9, 20), d(2023, 10, 10))];
        let c = compute_with(&e, 0.0, 0.0, &across, &p);
        assert_eq!((c.work_years[0].unpaid, c.work_years[1].unpaid), (8, 13));
        assert!(c.work_years.iter().all(|y| y.shift == 0));
    }

    #[test]
    fn unpaid_days_inside_the_moved_part_of_the_year_do_not_count_either() {
        // 30 days of unpaid leave begin 5 days before the nominal end of the first year (27 Sep 2023):
        // 5 of them are in this year, the next 25 in the following one.
        let p = params(d(2026, 10, 22));
        let e = emp(d(2022, 9, 28));
        let u = [span(LeaveType::Unpaid, d(2023, 9, 23), d(2023, 10, 22))];
        let c = compute_with(&e, 0.0, 0.0, &u, &p);
        assert_eq!((c.work_years[0].unpaid, c.work_years[0].shift), (5, 0));
        assert_eq!((c.work_years[1].unpaid, c.work_years[1].shift), (25, 11));
        // Together with a leave already above the limit in the same year the extension is longer.
        let first = [span(LeaveType::Unpaid, d(2022, 11, 1), d(2022, 11, 20)), span(LeaveType::Unpaid, d(2023, 9, 25), d(2023, 10, 10))];
        let c = compute_with(&e, 0.0, 0.0, &first, &p);
        // 20 days early in the year; 3 more before the nominal end (all over the limit); the year is
        // extended by 6 + 3 = 9 days and the days of the second record in the extension count as well.
        assert_eq!((c.work_years[0].unpaid, c.work_years[0].shift), (20 + 16, 22));
        assert_eq!(c.work_years[1].start, d(2023, 9, 28) + Duration::days(22));
    }

    #[test]
    fn childcare_leave_does_not_count_at_all() {
        // Hired 10 Jan 2020; childcare leave from 1 Jun 2022 to 31 May 2025 (1096 days).
        let p = params(d(2026, 10, 22));
        let e = emp(d(2020, 1, 10));
        let care = [span(LeaveType::Childcare, d(2022, 6, 1), d(2025, 5, 31))];
        let c = compute_with(&e, 0.0, 0.0, &care, &p);
        let y3 = &c.work_years[2];
        assert_eq!(y3.start, d(2022, 1, 10));
        assert_eq!((y3.childcare, y3.shift), (1096, 1096));
        assert_eq!(y3.end, d(2023, 1, 9) + Duration::days(1096));
        assert_eq!(c.work_years[3].start, y3.end + Duration::days(1));
        // The years before the leave are as usual; there is no working year that started in 2023 or 2024.
        assert_eq!((c.work_years[0].shift, c.work_years[1].shift), (0, 0));
        assert!(period_for_year(&e, &c, &p, 2023).is_none() && period_for_year(&e, &c, &p, 2024).is_none());
        assert_eq!(period_for_year(&e, &c, &p, 2022).map(|r| r.k), Some(3));
        // Without the leave the employee would be in the 7th working year by now; three years of
        // childcare leave do not count, so it is the 4th (started on 10 Jan 2026).
        assert_eq!(compute(&e, 0.0, 0.0, &p).started, 7);
        assert_eq!(c.started, 4);
        assert_eq!(c.work_years[3].start, d(2026, 1, 10));
        // The childcare leave that is still going on: the year stays open.
        let going = [span(LeaveType::Childcare, d(2025, 7, 1), d(2028, 6, 30))];
        let c = compute_with(&e, 0.0, 0.0, &going, &p);
        assert_eq!(c.started, 6, "the sixth year began on 10 Jan 2025 and has not ended: the leave goes on");
        assert!(c.work_years.last().is_some_and(|y| y.childcare > 0 && y.end > d(2028, 6, 30)));
    }

    #[test]
    fn maternity_leave_counts_as_service() {
        let p = params(d(2026, 10, 22));
        let e = emp(d(2022, 9, 28));
        let plain = compute(&e, 0.0, 0.0, &p);
        // Maternity leave never reaches the engine as a span; Snapshot::build leaves it out.
        let data = Data {
            settings: Settings { as_of_manual: Some(d(2026, 10, 22)), ..Default::default() },
            employees: vec![e.clone()],
            leaves: vec![Leave { id: 1, employee_id: 1, kind: LeaveType::Maternity, start: Some(d(2024, 1, 10)), end: Some(d(2024, 6, 1)), ..Default::default() }],
            ..Default::default()
        };
        let s = Snapshot::build(&data, d(2026, 10, 22));
        assert_eq!(s.emps[0].calc.work_years, plain.work_years);
        assert_eq!((s.emps[0].calc.taken, s.emps[0].calc.left), (0.0, plain.left), "no days are written off either");
        assert_eq!(s.leaves[0].status, Some(LeaveStatus::Done));
        assert_eq!(s.leaves[0].end, Some(d(2024, 6, 1)));
    }

    #[test]
    fn unpaid_leave_in_the_first_months_postpones_the_first_right() {
        // Hired 1 Jul 2026, 20 days of unpaid leave in July: the six months are complete 6 days later.
        let e = emp(d(2026, 7, 1));
        let u = [span(LeaveType::Unpaid, d(2026, 7, 5), d(2026, 7, 24))];
        let at = |as_of| compute_with(&e, 0.0, 0.0, &u, &params(as_of));
        assert_eq!(at(d(2027, 1, 6)).available, 0);
        assert_eq!(at(d(2027, 1, 7)).available, 1);
        assert_eq!(compute(&e, 0.0, 0.0, &params(d(2027, 1, 1))).available, 1, "without it the right arises on 1 Jan");
    }

    #[test]
    fn snapshot_applies_the_leave_of_each_employee_to_that_employee_only() {
        let data = Data {
            settings: Settings { as_of_manual: Some(d(2026, 10, 22)), ..Default::default() },
            employees: vec![emp(d(2022, 9, 28)), Employee { id: 2, ..emp(d(2022, 9, 28)) }],
            leaves: vec![
                Leave { id: 1, employee_id: 1, kind: LeaveType::Unpaid, start: Some(d(2023, 1, 10)), end: Some(d(2023, 1, 29)), ..Default::default() },
                Leave { id: 2, employee_id: 1, start: Some(d(2025, 6, 2)), base_days: 14.0, ..Default::default() },
            ],
            ..Default::default()
        };
        let s = Snapshot::build(&data, d(2026, 10, 22));
        assert_eq!(s.emps[0].calc.work_years[0].shift, 6);
        assert_eq!(s.emps[1].calc.work_years[0].shift, 0);
        assert_eq!(s.emps[0].calc.taken, 14.0, "the unpaid leave is not written off from the balance");
        assert_eq!(s.emps[0].leave_count, 2);
        assert_eq!(s.leaves[0].end, Some(d(2023, 1, 29)));
        assert_eq!(s.leaves[0].status, Some(LeaveStatus::Done));
    }

    #[test]
    fn staff_plural() {
        assert_eq!(staff_gen(1), "сотрудника");
        assert_eq!(staff_gen(2), "сотрудников");
        assert_eq!(staff_gen(11), "сотрудников");
        assert_eq!(staff_gen(21), "сотрудника");
    }

    #[test]
    fn days_formatting() {
        assert_eq!(fmt_days(14.0), "14");
        assert_eq!(fmt_days(14.5), "14,5");
        assert_eq!(fmt_days(-3.0), "-3");
    }
}
