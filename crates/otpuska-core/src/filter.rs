//! Filters shared by the screens and the printed reports: both ask this module for the rows,
//! so what is printed is exactly what the screen shows.

use crate::date::{self, Date};
use crate::engine::{self, LeaveRow, LeaveStatus, Snapshot, State};
use crate::model::Status;
use chrono::Datelike;

// ---- period -----------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PeriodPreset {
    #[default]
    All,
    ThisYear,
    LastYear,
    NextYear,
    Custom,
}

impl PeriodPreset {
    pub const ALL: [PeriodPreset; 5] = [PeriodPreset::All, PeriodPreset::ThisYear, PeriodPreset::LastYear, PeriodPreset::NextYear, PeriodPreset::Custom];

    pub fn label(self) -> &'static str {
        match self {
            PeriodPreset::All => "Всё время",
            PeriodPreset::ThisYear => "Этот год",
            PeriodPreset::LastYear => "Прошлый год",
            PeriodPreset::NextYear => "Следующий год",
            PeriodPreset::Custom => "Свои даты",
        }
    }
}

type Bounds = Option<(Option<Date>, Option<Date>)>;

/// A calendar period; years are counted from the calculation date.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PeriodFilter {
    pub preset: PeriodPreset,
    pub from: Option<Date>,
    pub to: Option<Date>,
}

impl PeriodFilter {
    /// Inclusive bounds (an open end is `None`); `None` when nothing is restricted.
    pub fn bounds(&self, as_of: Date) -> Bounds {
        let year = |y: i32| Some((Date::from_ymd_opt(y, 1, 1), Date::from_ymd_opt(y, 12, 31)));
        match self.preset {
            PeriodPreset::All => None,
            PeriodPreset::ThisYear => year(as_of.year()),
            PeriodPreset::LastYear => year(as_of.year() - 1),
            PeriodPreset::NextYear => year(as_of.year() + 1),
            PeriodPreset::Custom if self.from.is_none() && self.to.is_none() => None,
            PeriodPreset::Custom => Some((self.from, self.to)),
        }
    }

    pub fn describe(&self, as_of: Date) -> String {
        match self.bounds(as_of) {
            None => "за всё время".to_string(),
            Some((Some(a), Some(b))) => format!("{} – {}", date::fmt(a), date::fmt(b)),
            Some((Some(a), None)) => format!("с {}", date::fmt(a)),
            Some((None, Some(b))) => format!("по {}", date::fmt(b)),
            Some((None, None)) => "за всё время".to_string(),
        }
    }

    pub fn is_active(&self, as_of: Date) -> bool {
        self.bounds(as_of).is_some()
    }
}

/// Does the leave overlap the period? Undated leaves only match when the period is open.
fn in_range(l: &LeaveRow, bounds: Bounds) -> bool {
    let Some((from, to)) = bounds else { return true };
    let Some(start) = l.leave.start else { return false };
    let end = l.end.unwrap_or(start);
    from.map_or(true, |f| end >= f) && to.map_or(true, |t| start <= t)
}

// ---- employees ----------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WorkFilter {
    #[default]
    All,
    Working,
    Suspended,
    Dismissed,
}

impl WorkFilter {
    fn matches(self, s: Status) -> bool {
        match self {
            WorkFilter::All => true,
            WorkFilter::Working => s == Status::Working,
            WorkFilter::Suspended => s == Status::Suspended,
            WorkFilter::Dismissed => s == Status::Dismissed,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            WorkFilter::All => "Все",
            WorkFilter::Working => "Работают",
            WorkFilter::Suspended => "Приостановка ТД",
            WorkFilter::Dismissed => "Уволены",
        }
    }
}

/// Did the employee take leave in the period?
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Took {
    #[default]
    Any,
    Taken,
    NotTaken,
    Planned,
}

impl Took {
    pub const ALL: [Took; 4] = [Took::Any, Took::Taken, Took::NotTaken, Took::Planned];

    pub fn label(self) -> &'static str {
        match self {
            Took::Any => "Любой",
            Took::Taken => "Был в отпуске",
            Took::NotTaken => "Не был в отпуске",
            Took::Planned => "Запланирован",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EmpFilter {
    pub search: String,
    pub dept: Option<i64>,
    pub work: WorkFilter,
    /// Several states can be selected; empty means any.
    pub states: Vec<State>,
    pub took: Took,
    pub period: PeriodFilter,
}

impl EmpFilter {
    pub fn is_active(&self) -> bool {
        !self.search.trim().is_empty() || self.dept.is_some() || self.work != WorkFilter::All || !self.states.is_empty() || self.took != Took::Any
    }

    pub fn reset(&mut self) {
        *self = EmpFilter::default();
    }

    /// Human-readable lines for the printed header, one per active filter.
    /// `with_work` — whether the "work status" filter exists on that screen.
    pub fn describe(&self, snap: &Snapshot, with_work: bool) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(id) = self.dept {
            out.push(format!("Подразделение: {}", snap.dept_name(Some(id))));
        }
        if with_work && self.work != WorkFilter::All {
            out.push(format!("Статус работы: {}", self.work.label()));
        }
        if !self.states.is_empty() {
            out.push(format!("Состояние: {}", self.states.iter().map(|s| s.label()).collect::<Vec<_>>().join(", ")));
        }
        if self.took != Took::Any {
            out.push(format!("Отпуск: {} ({})", self.took.label().to_lowercase(), self.period.describe(snap.params.as_of)));
        }
        if !self.search.trim().is_empty() {
            out.push(format!("Поиск: «{}»", self.search.trim()));
        }
        out
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Everybody, narrowed by the work-status filter.
    All,
    /// Dismissed employees never appear (summary, periods).
    Active,
}

fn contains_ci(hay: &str, needle_lower: &str) -> bool {
    needle_lower.is_empty() || hay.to_lowercase().contains(needle_lower)
}

impl Snapshot {
    /// Indexes into `emps` in directory order. `with_states = false` leaves the state selection
    /// out (the summary tiles count the people each state would show).
    pub fn emp_rows(&self, f: &EmpFilter, scope: Scope, with_states: bool) -> Vec<usize> {
        let needle = f.search.trim().to_lowercase();
        let bounds = f.period.bounds(self.params.as_of);
        // Per employee: leave taken / planned in the period.
        let (mut taken, mut planned) = (vec![false; self.emps.len()], vec![false; self.emps.len()]);
        if f.took != Took::Any {
            for l in &self.leaves {
                let Some(i) = l.emp_ix else { continue };
                if !in_range(l, bounds) {
                    continue;
                }
                match l.status {
                    Some(LeaveStatus::Done | LeaveStatus::InLeave) => taken[i] = true,
                    Some(LeaveStatus::Undated) if bounds.is_none() => taken[i] = true,
                    Some(LeaveStatus::Planned) => planned[i] = true,
                    _ => {}
                }
            }
        }
        (0..self.emps.len())
            .filter(|&i| {
                let r = &self.emps[i];
                let status_ok = match scope {
                    Scope::Active => r.emp.status != Status::Dismissed,
                    Scope::All => f.work.matches(r.emp.status),
                };
                status_ok
                    && (f.dept.is_none() || r.emp.department_id == f.dept)
                    && (!with_states || f.states.is_empty() || f.states.contains(&r.calc.state))
                    && match f.took {
                        Took::Any => true,
                        Took::Taken => taken[i],
                        Took::NotTaken => !taken[i],
                        Took::Planned => planned[i],
                    }
                    && (contains_ci(&r.emp.fio, &needle) || contains_ci(&r.emp.position, &needle) || contains_ci(&r.dept, &needle))
            })
            .collect()
    }

    /// The summary order: active employees, the most urgent first.
    pub fn summary_rows(&self, f: &EmpFilter) -> Vec<usize> {
        let mut rows: Vec<(u8, i64, usize)> = self
            .emp_rows(f, Scope::Active, true)
            .into_iter()
            .map(|i| (self.emps[i].calc.state.rank(), self.emps[i].calc.days_left().unwrap_or(0), i))
            .collect();
        rows.sort();
        rows.into_iter().map(|(_, _, i)| i).collect()
    }

    /// Counters for exactly these employees.
    pub fn counts_for(&self, rows: &[usize]) -> engine::Counts {
        let mut c = engine::Counts::default();
        let mut in_rows = vec![false; self.emps.len()];
        for &i in rows {
            in_rows[i] = true;
            match self.emps[i].calc.state {
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
            .filter(|l| l.status == Some(LeaveStatus::InLeave) && !l.emp_dismissed && l.emp_ix.is_some_and(|i| in_rows[i]))
            .count();
        c
    }
}

// ---- working years ---------------------------------------------------------------------------

/// By what is left in the years shown on the "Периоды" screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RestFilter {
    #[default]
    Any,
    /// At least one of the shown years still has unused days.
    HasLeft,
    /// None of the shown years has unused days.
    NoLeft,
}

impl RestFilter {
    pub const ALL: [RestFilter; 3] = [RestFilter::Any, RestFilter::HasLeft, RestFilter::NoLeft];

    pub fn label(self) -> &'static str {
        match self {
            RestFilter::Any => "Все",
            RestFilter::HasLeft => "Не отгулял",
            RestFilter::NoLeft => "Отгулял",
        }
    }
}

impl Snapshot {
    /// Active employees (directory order) for the years `first .. first + span`.
    pub fn period_rows(&self, f: &EmpFilter, rest: RestFilter, first: i32, span: usize) -> Vec<usize> {
        self.emp_rows(f, Scope::Active, true)
            .into_iter()
            .filter(|&i| {
                if rest == RestFilter::Any {
                    return true;
                }
                let r = &self.emps[i];
                let has_left = (0..span as i32).any(|y| {
                    engine::period_for_year(&r.emp, &r.calc, &self.params, first + y).and_then(|p| p.left).is_some_and(|l| l > 0.0)
                });
                has_left == (rest == RestFilter::HasLeft)
            })
            .collect()
    }
}

// ---- journal --------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LeaveShow {
    #[default]
    All,
    /// In leave now or planned.
    Active,
    InLeave,
    Planned,
    Done,
    Undated,
}

impl LeaveShow {
    pub fn label(self) -> &'static str {
        match self {
            LeaveShow::All => "Все",
            LeaveShow::Active => "Идут и запланированы",
            LeaveShow::InLeave => "В отпуске",
            LeaveShow::Planned => "Запланированы",
            LeaveShow::Done => "Завершены",
            LeaveShow::Undated => "Без даты",
        }
    }

    fn matches(self, s: Option<LeaveStatus>) -> bool {
        match self {
            LeaveShow::All => true,
            LeaveShow::Active => matches!(s, Some(LeaveStatus::InLeave | LeaveStatus::Planned)),
            LeaveShow::InLeave => s == Some(LeaveStatus::InLeave),
            LeaveShow::Planned => s == Some(LeaveStatus::Planned),
            LeaveShow::Done => s == Some(LeaveStatus::Done),
            LeaveShow::Undated => s == Some(LeaveStatus::Undated),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LeaveKind {
    #[default]
    Any,
    Base,
    Extra,
}

impl LeaveKind {
    pub const ALL: [LeaveKind; 3] = [LeaveKind::Any, LeaveKind::Base, LeaveKind::Extra];

    pub fn label(self) -> &'static str {
        match self {
            LeaveKind::Any => "Любой",
            LeaveKind::Base => "Основной",
            LeaveKind::Extra => "Дополнительный",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LeaveFilter {
    pub search: String,
    pub dept: Option<i64>,
    pub show: LeaveShow,
    pub kind: LeaveKind,
    pub period: PeriodFilter,
}

impl LeaveFilter {
    /// The report «Сейчас в отпуске и ближайшие» starts with only ongoing and planned leaves.
    pub fn upcoming() -> Self {
        LeaveFilter { show: LeaveShow::Active, ..Default::default() }
    }

    pub fn is_active(&self, default_show: LeaveShow, as_of: Date) -> bool {
        !self.search.trim().is_empty() || self.dept.is_some() || self.show != default_show || self.kind != LeaveKind::Any || self.period.is_active(as_of)
    }

    pub fn describe(&self, snap: &Snapshot, default_show: LeaveShow) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(id) = self.dept {
            out.push(format!("Подразделение: {}", snap.dept_name(Some(id))));
        }
        if self.show != default_show {
            out.push(format!("Статус: {}", self.show.label()));
        }
        if self.kind != LeaveKind::Any {
            out.push(format!("Вид отпуска: {}", self.kind.label().to_lowercase()));
        }
        if self.period.is_active(snap.params.as_of) {
            out.push(format!("Период: {}", self.period.describe(snap.params.as_of)));
        }
        if !self.search.trim().is_empty() {
            out.push(format!("Поиск: «{}»", self.search.trim()));
        }
        out
    }
}

impl Snapshot {
    /// Indexes into `leaves` in journal order (by id).
    pub fn leave_rows(&self, f: &LeaveFilter) -> Vec<usize> {
        let needle = f.search.trim().to_lowercase();
        let bounds = f.period.bounds(self.params.as_of);
        (0..self.leaves.len())
            .filter(|&i| {
                let l = &self.leaves[i];
                f.show.matches(l.status)
                    && (f.dept.is_none() || l.emp_ix.is_some_and(|e| self.emps[e].emp.department_id == f.dept))
                    && match f.kind {
                        LeaveKind::Any => true,
                        LeaveKind::Base => l.leave.base_days > 0.0,
                        LeaveKind::Extra => l.leave.extra_days > 0.0,
                    }
                    && in_range(l, bounds)
                    && (contains_ci(&l.fio, &needle) || contains_ci(&l.dept, &needle) || contains_ci(&l.leave.note, &needle))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn d(y: i32, m: u32, day: u32) -> Date {
        Date::from_ymd_opt(y, m, day).unwrap()
    }

    fn snap() -> Snapshot {
        let emp = |id, fio: &str, dept, hire, status| Employee { id, fio: fio.into(), department_id: Some(dept), hire_date: Some(hire), status, ..Default::default() };
        let leave = |id, e, start: Option<Date>, days| Leave { id, employee_id: e, start, base_days: days, ..Default::default() };
        let data = Data {
            settings: Settings { as_of_manual: Some(d(2026, 10, 22)), ..Default::default() },
            departments: vec![Department { id: 1, name: "Цех".into() }, Department { id: 2, name: "Офис".into() }],
            employees: vec![
                emp(1, "Иванов", 1, d(2022, 9, 28), Status::Working),   // overdue; leave in 2025
                emp(2, "Петров", 2, d(2025, 1, 8), Status::Working),    // no leaves
                emp(3, "Сидоров", 1, d(2024, 6, 1), Status::Working),   // leave in 2026 + planned 2027
                emp(4, "Уволенный", 2, d(2020, 1, 1), Status::Dismissed),
            ],
            leaves: vec![
                leave(1, 1, Some(d(2025, 7, 7)), 14.0),
                leave(2, 3, Some(d(2026, 3, 2)), 28.0),
                leave(3, 3, Some(d(2027, 2, 1)), 14.0),
                leave(4, 1, None, 14.0),
            ],
        };
        Snapshot::build(&data, date::today())
    }

    fn names(s: &Snapshot, rows: &[usize]) -> Vec<String> {
        rows.iter().map(|&i| s.emps[i].emp.fio.clone()).collect()
    }

    #[test]
    fn no_filter_shows_everybody() {
        let s = snap();
        let f = EmpFilter::default();
        assert!(!f.is_active());
        assert_eq!(s.emp_rows(&f, Scope::All, true).len(), 4);
        assert_eq!(s.emp_rows(&f, Scope::Active, true).len(), 3, "dismissed never in the summary");
        assert!(f.describe(&s, true).is_empty());
    }

    #[test]
    fn department_state_search_and_work_filters() {
        let s = snap();
        let mut f = EmpFilter { dept: Some(1), ..Default::default() };
        assert_eq!(names(&s, &s.emp_rows(&f, Scope::All, true)), ["Иванов", "Сидоров"]);
        f.states = vec![State::Overdue];
        assert_eq!(names(&s, &s.emp_rows(&f, Scope::All, true)), ["Иванов"]);
        assert_eq!(s.emp_rows(&f, Scope::All, false).len(), 2, "tiles ignore the state selection");
        f.states.clear();
        f.search = "сид".into();
        assert_eq!(names(&s, &s.emp_rows(&f, Scope::All, true)), ["Сидоров"]);
        let f = EmpFilter { work: WorkFilter::Dismissed, ..Default::default() };
        assert_eq!(names(&s, &s.emp_rows(&f, Scope::All, true)), ["Уволенный"]);
        assert_eq!(s.emp_rows(&f, Scope::Active, true).len(), 3, "the work-status filter does not apply to the active scope");
    }

    #[test]
    fn took_leave_in_period() {
        let s = snap();
        let year = |p| PeriodFilter { preset: p, ..Default::default() };
        let mk = |took, preset| EmpFilter { took, period: year(preset), ..Default::default() };
        // This year (2026): only Сидоров took leave; Иванов's 2025 leave is outside.
        assert_eq!(names(&s, &s.emp_rows(&mk(Took::Taken, PeriodPreset::ThisYear), Scope::Active, true)), ["Сидоров"]);
        assert_eq!(names(&s, &s.emp_rows(&mk(Took::NotTaken, PeriodPreset::ThisYear), Scope::Active, true)), ["Иванов", "Петров"]);
        // Last year: Иванов.
        assert_eq!(names(&s, &s.emp_rows(&mk(Took::Taken, PeriodPreset::LastYear), Scope::Active, true)), ["Иванов"]);
        // Planned next year: Сидоров.
        assert_eq!(names(&s, &s.emp_rows(&mk(Took::Planned, PeriodPreset::NextYear), Scope::Active, true)), ["Сидоров"]);
        // Whole time: the undated leave still counts for Иванов; Петров never took any.
        assert_eq!(names(&s, &s.emp_rows(&mk(Took::NotTaken, PeriodPreset::All), Scope::Active, true)), ["Петров"]);
        // Custom range with an open end.
        let custom = EmpFilter {
            took: Took::Taken,
            period: PeriodFilter { preset: PeriodPreset::Custom, from: Some(d(2026, 1, 1)), to: None },
            ..Default::default()
        };
        assert_eq!(names(&s, &s.emp_rows(&custom, Scope::Active, true)), ["Сидоров"]);
        let lines = custom.describe(&s, false);
        assert_eq!(lines, ["Отпуск: был в отпуске (с 01.01.2026)"]);
    }

    #[test]
    fn summary_rows_are_urgent_first_and_counts_match() {
        let s = snap();
        let rows = s.summary_rows(&EmpFilter::default());
        assert_eq!(names(&s, &rows)[0], "Иванов");
        let c = s.counts_for(&rows);
        assert_eq!(c.overdue, 1);
        let only_office = s.summary_rows(&EmpFilter { dept: Some(2), ..Default::default() });
        assert_eq!(names(&s, &only_office), ["Петров"]);
        assert_eq!(s.counts_for(&only_office).on_leave, 0);
    }

    #[test]
    fn period_rows_by_remainder() {
        let s = snap();
        let f = EmpFilter::default();
        // Years 2022..2029: everybody active still has unused days somewhere.
        assert_eq!(s.period_rows(&f, RestFilter::HasLeft, 2022, 8).len(), 3);
        assert!(s.period_rows(&f, RestFilter::NoLeft, 2022, 8).is_empty());
        assert_eq!(s.period_rows(&f, RestFilter::Any, 2022, 8).len(), 3);
        // A window that has no working years of anybody leaves nothing to take.
        let nl = s.period_rows(&f, RestFilter::NoLeft, 2018, 2);
        assert_eq!(names(&s, &nl), ["Иванов", "Петров", "Сидоров"]);
    }

    #[test]
    fn leave_filters() {
        let s = snap();
        let all = LeaveFilter::default();
        assert_eq!(s.leave_rows(&all).len(), 4);
        let active = LeaveFilter::upcoming();
        assert_eq!(s.leave_rows(&active).len(), 1, "only the leave planned for 2027");
        assert!(!active.is_active(LeaveShow::Active, s.params.as_of));
        assert!(active.is_active(LeaveShow::All, s.params.as_of));
        let dept = LeaveFilter { dept: Some(1), ..Default::default() };
        assert_eq!(s.leave_rows(&dept).len(), 4);
        let dept2 = LeaveFilter { dept: Some(2), ..Default::default() };
        assert!(s.leave_rows(&dept2).is_empty());
        let in_2026 = LeaveFilter { period: PeriodFilter { preset: PeriodPreset::ThisYear, ..Default::default() }, ..Default::default() };
        assert_eq!(s.leave_rows(&in_2026).len(), 1);
        assert_eq!(in_2026.describe(&s, LeaveShow::All), ["Период: 01.01.2026 – 31.12.2026"]);
        let extra = LeaveFilter { kind: LeaveKind::Extra, ..Default::default() };
        assert!(s.leave_rows(&extra).is_empty());
        // A leave that starts before and ends inside the period is included.
        let spill = LeaveFilter {
            period: PeriodFilter { preset: PeriodPreset::Custom, from: Some(d(2025, 7, 10)), to: Some(d(2025, 7, 12)) },
            ..Default::default()
        };
        assert_eq!(s.leave_rows(&spill).len(), 1);
    }
}
