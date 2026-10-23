//! The vacation schedule (график отпусков, ст. 123 ТК РФ) checked against the journal, and the
//! reminders that follow from both: a leave is about to start, a planned leave was not put into
//! the journal, a maternity or childcare leave is about to end.
//!
//! An entry of the schedule is *fulfilled* by a journal record of annual leave of the same
//! employee whose dates overlap it. Every journal record fulfils at most one entry, the nearest
//! by start date first. A leave moved to other dates therefore shows up as "not put into the
//! journal" until the schedule entry is changed too — that is the point of tracking it.

use crate::date::{edate, Date};
use crate::engine::{EmpRow, LeaveRow, LeaveStatus, Snapshot};
use crate::model::*;
use chrono::Duration;
use std::collections::HashMap;

/// A planned leave whose date has passed stays among the reminders this many days.
pub const MISSED_WINDOW_DAYS: i64 = 30;

/// The employee has to be told of the start of a leave under signature at least this many days
/// ahead (ст. 123 ТК РФ).
pub const NOTICE_DAYS: i64 = 14;

// ---- schedule against the journal ----------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanStatus {
    /// Still ahead, nothing in the journal yet.
    Ahead,
    /// A journal record exists, the leave has not started.
    Booked,
    /// The leave is going on now.
    InLeave,
    /// The leave was taken.
    Done,
    /// The planned date has come (or passed) and there is no such leave in the journal.
    Missed,
}

impl PlanStatus {
    pub const ALL: [PlanStatus; 5] = [PlanStatus::Ahead, PlanStatus::Booked, PlanStatus::InLeave, PlanStatus::Done, PlanStatus::Missed];

    pub fn label(self) -> &'static str {
        match self {
            PlanStatus::Ahead => "По графику",
            PlanStatus::Booked => "Оформлен",
            PlanStatus::InLeave => "В отпуске",
            PlanStatus::Done => "Использован",
            PlanStatus::Missed => "Не оформлен",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            PlanStatus::Ahead => "Отпуск ещё впереди, в журнале записи нет",
            PlanStatus::Booked => "В журнале есть запись с этими датами, отпуск ещё не начался",
            PlanStatus::InLeave => "Сотрудник сейчас в этом отпуске",
            PlanStatus::Done => "Отпуск взят: запись в журнале завершена",
            PlanStatus::Missed => "Дата по графику наступила, а такого отпуска в журнале нет — внесите его или перенесите в графике",
        }
    }
}

#[derive(Clone, Debug)]
pub struct PlanRow {
    pub plan: Plan,
    pub emp_ix: Option<usize>,
    pub fio: String,
    pub dept: String,
    pub end: Option<Date>,
    pub status: PlanStatus,
    /// The journal record (index into `Snapshot::leaves`) that fulfils the entry.
    pub fact: Option<usize>,
    pub emp_dismissed: bool,
}

pub(crate) fn match_plans(plans: &[Plan], emps: &[EmpRow], leaves: &[LeaveRow], index: &HashMap<i64, usize>, as_of: Date) -> Vec<PlanRow> {
    // Journal records that can fulfil an entry: annual leave with both dates.
    let mut candidates: HashMap<i64, Vec<usize>> = HashMap::new();
    for (i, l) in leaves.iter().enumerate() {
        if l.leave.kind.is_paid() && l.leave.start.is_some() && l.end.is_some() {
            candidates.entry(l.leave.employee_id).or_default().push(i);
        }
    }
    let mut order: Vec<usize> = (0..plans.len()).collect();
    order.sort_by_key(|&i| (plans[i].start, plans[i].id));
    let mut used = vec![false; leaves.len()];
    let mut fact: Vec<Option<usize>> = vec![None; plans.len()];
    for &i in &order {
        let pl = &plans[i];
        let (Some(ps), Some(pe)) = (pl.start, pl.end()) else { continue };
        let Some(list) = candidates.get(&pl.employee_id) else { continue };
        let best = list
            .iter()
            .copied()
            .filter(|&li| !used[li])
            .filter(|&li| match (leaves[li].leave.start, leaves[li].end) {
                (Some(ls), Some(le)) => ls <= pe && le >= ps,
                _ => false,
            })
            .min_by_key(|&li| (leaves[li].leave.start.map_or(i64::MAX, |ls| (ls - ps).num_days().abs()), leaves[li].leave.id));
        if let Some(li) = best {
            used[li] = true;
            fact[i] = Some(li);
        }
    }
    plans
        .iter()
        .enumerate()
        .map(|(i, pl)| {
            let ix = index.get(&pl.employee_id).copied();
            let row = ix.map(|e| &emps[e]);
            let status = match fact[i] {
                Some(li) => match leaves[li].status {
                    Some(LeaveStatus::InLeave) => PlanStatus::InLeave,
                    Some(LeaveStatus::Done) => PlanStatus::Done,
                    _ => PlanStatus::Booked,
                },
                None if pl.start.is_some_and(|s| s <= as_of) => PlanStatus::Missed,
                None => PlanStatus::Ahead,
            };
            PlanRow {
                plan: pl.clone(),
                emp_ix: ix,
                fio: row.map(|r| r.emp.fio.clone()).unwrap_or_default(),
                dept: row.map(|r| r.dept.clone()).unwrap_or_default(),
                end: pl.end(),
                status,
                fact: fact[i],
                emp_dismissed: row.is_some_and(|r| r.emp.status == Status::Dismissed),
            }
        })
        .collect()
}

// ---- reminders ----------------------------------------------------------------------------------

/// How urgent a reminder is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Band {
    /// The planned date passed and the leave is not in the journal.
    Missed,
    /// Within the near stage (14 days by default).
    Near,
    /// Within the early stage (one month by default).
    Far,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReminderKind {
    /// An annual leave from the journal is about to start.
    Start,
    /// A leave from the schedule is about to start and is not in the journal yet.
    PlanStart,
    /// A leave from the schedule should have started and is not in the journal.
    PlanMissed,
    /// A maternity or childcare leave is about to end.
    End(LeaveType),
}

#[derive(Clone, Debug)]
pub struct Reminder {
    pub kind: ReminderKind,
    pub band: Band,
    pub emp_ix: usize,
    /// The day the reminder is about: the first day of the leave (last day for `End`).
    pub date: Date,
    /// Days from the calculation date to `date`; negative when it has passed.
    pub days: i64,
    /// Last day of the leave (for `Start`, `PlanStart` and `PlanMissed`).
    pub end: Option<Date>,
    /// Length of the leave in days.
    pub leave_days: f64,
    pub leave_ix: Option<usize>,
    pub plan_ix: Option<usize>,
}

impl Reminder {
    /// What the reminder is about.
    pub fn kind_label(&self) -> &'static str {
        match self.kind {
            ReminderKind::Start => "Начало отпуска",
            ReminderKind::PlanStart => "Начало отпуска по графику",
            ReminderKind::PlanMissed => "Отпуск по графику не оформлен",
            ReminderKind::End(LeaveType::Childcare) => "Окончание отпуска по уходу за ребёнком",
            ReminderKind::End(_) => "Окончание отпуска по беременности и родам",
        }
    }

    /// "сегодня", "завтра", "через 12 дн.", "3 дн. назад".
    pub fn when(&self) -> String {
        match self.days {
            0 => "сегодня".to_string(),
            1 => "завтра".to_string(),
            n if n > 1 => format!("через {n} дн."),
            n => format!("{} дн. назад", -n),
        }
    }

    /// The last day to tell the employee of the start of the leave under signature.
    pub fn notify_by(&self) -> Option<Date> {
        matches!(self.kind, ReminderKind::Start | ReminderKind::PlanStart).then(|| self.date - Duration::days(NOTICE_DAYS))
    }

    /// The first working day after a leave that ends on `date`.
    pub fn back_to_work(&self) -> Option<Date> {
        matches!(self.kind, ReminderKind::End(_)).then(|| self.date + Duration::days(1))
    }
}

impl Snapshot {
    pub(crate) fn compute_reminders(&self) -> Vec<Reminder> {
        let p = &self.params;
        let far_end = edate(p.as_of, p.remind_far_months.max(0));
        let band_of = |start: Date| -> Option<Band> {
            let days = (start - p.as_of).num_days();
            if days <= i64::from(p.remind_near_days.max(0)) {
                Some(Band::Near)
            } else if start <= far_end {
                Some(Band::Far)
            } else {
                None
            }
        };
        let plan_of: HashMap<usize, usize> = self.plans.iter().enumerate().filter_map(|(pi, pr)| pr.fact.map(|li| (li, pi))).collect();
        let mut out = Vec::new();

        for (li, l) in self.leaves.iter().enumerate() {
            let Some(ix) = l.emp_ix else { continue };
            if self.emps[ix].emp.status == Status::Dismissed {
                continue;
            }
            let (Some(start), Some(end)) = (l.leave.start, l.end) else { continue };
            match l.leave.kind {
                LeaveType::Paid if start > p.as_of => {
                    if let Some(band) = band_of(start) {
                        out.push(Reminder {
                            kind: ReminderKind::Start,
                            band,
                            emp_ix: ix,
                            date: start,
                            days: (start - p.as_of).num_days(),
                            end: Some(end),
                            leave_days: l.leave.length(),
                            leave_ix: Some(li),
                            plan_ix: plan_of.get(&li).copied(),
                        });
                    }
                }
                LeaveType::Maternity | LeaveType::Childcare if end >= p.as_of && (end - p.as_of).num_days() <= i64::from(p.remind_end_days.max(0)) => {
                    out.push(Reminder {
                        kind: ReminderKind::End(l.leave.kind),
                        band: Band::Near,
                        emp_ix: ix,
                        date: end,
                        days: (end - p.as_of).num_days(),
                        end: None,
                        leave_days: l.leave.length(),
                        leave_ix: Some(li),
                        plan_ix: None,
                    });
                }
                _ => {}
            }
        }

        for (pi, pr) in self.plans.iter().enumerate() {
            let (Some(ix), Some(start)) = (pr.emp_ix, pr.plan.start) else { continue };
            if pr.emp_dismissed {
                continue;
            }
            let days = (start - p.as_of).num_days();
            let (kind, band) = match pr.status {
                PlanStatus::Ahead => match band_of(start) {
                    Some(band) => (ReminderKind::PlanStart, band),
                    None => continue,
                },
                PlanStatus::Missed if -days <= MISSED_WINDOW_DAYS => (ReminderKind::PlanMissed, Band::Missed),
                _ => continue,
            };
            out.push(Reminder {
                kind,
                band,
                emp_ix: ix,
                date: start,
                days,
                end: pr.end,
                leave_days: pr.plan.total_days(),
                leave_ix: None,
                plan_ix: Some(pi),
            });
        }

        out.sort_by(|a, b| (a.band, a.date, &self.emps[a.emp_ix].emp.fio).cmp(&(b.band, b.date, &self.emps[b.emp_ix].emp.fio)));
        out
    }

    /// Reminders that need action now: missed entries and the near stage.
    pub fn reminders_urgent(&self) -> usize {
        self.reminders.iter().filter(|r| r.band != Band::Far).count()
    }

    /// Reminders of the early stage.
    pub fn reminders_early(&self) -> usize {
        self.reminders.iter().filter(|r| r.band == Band::Far).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> Date {
        Date::from_ymd_opt(y, m, day).unwrap()
    }

    fn data(as_of: Date) -> Data {
        let emp = |id, fio: &str| Employee { id, fio: fio.into(), hire_date: Some(d(2020, 1, 10)), ..Default::default() };
        Data {
            settings: Settings { as_of_manual: Some(as_of), ..Default::default() },
            departments: vec![],
            employees: vec![emp(1, "Иванов"), emp(2, "Петров"), emp(3, "Уволен")],
            leaves: vec![],
            plans: vec![],
        }
    }

    fn plan(id: i64, emp: i64, start: Date, days: f64) -> Plan {
        Plan { id, employee_id: emp, start: Some(start), base_days: days, ..Default::default() }
    }

    fn leave(id: i64, emp: i64, start: Date, days: f64) -> Leave {
        Leave { id, employee_id: emp, start: Some(start), base_days: days, ..Default::default() }
    }

    fn status_of(s: &Snapshot, plan_id: i64) -> PlanStatus {
        s.plans.iter().find(|p| p.plan.id == plan_id).unwrap().status
    }

    #[test]
    fn plan_status_follows_the_journal() {
        let mut dt = data(d(2026, 10, 23));
        dt.plans = vec![
            plan(1, 1, d(2026, 12, 1), 14.0), // ahead, nothing in the journal
            plan(2, 1, d(2026, 11, 2), 14.0), // booked
            plan(3, 2, d(2026, 10, 20), 14.0), // going on
            plan(4, 2, d(2026, 6, 1), 14.0),  // taken
            plan(5, 2, d(2026, 9, 1), 14.0),  // date passed, nothing in the journal
        ];
        dt.leaves = vec![leave(1, 1, d(2026, 11, 2), 14.0), leave(2, 2, d(2026, 10, 20), 14.0), leave(3, 2, d(2026, 6, 1), 14.0)];
        let s = Snapshot::build(&dt, d(2026, 10, 23));
        assert_eq!(status_of(&s, 1), PlanStatus::Ahead);
        assert_eq!(status_of(&s, 2), PlanStatus::Booked);
        assert_eq!(status_of(&s, 3), PlanStatus::InLeave);
        assert_eq!(status_of(&s, 4), PlanStatus::Done);
        assert_eq!(status_of(&s, 5), PlanStatus::Missed);
        assert_eq!(s.plans[1].fact, Some(0));
    }

    #[test]
    fn a_moved_leave_does_not_fulfil_the_old_entry_and_a_record_serves_one_entry() {
        let mut dt = data(d(2026, 10, 23));
        // Planned for July, taken in August: no overlap. Two entries for one record: only one gets it.
        dt.plans = vec![plan(1, 1, d(2026, 7, 1), 14.0), plan(2, 1, d(2026, 8, 3), 14.0), plan(3, 1, d(2026, 8, 5), 14.0)];
        dt.leaves = vec![leave(1, 1, d(2026, 8, 4), 14.0)];
        let s = Snapshot::build(&dt, d(2026, 10, 23));
        assert_eq!(status_of(&s, 1), PlanStatus::Missed);
        assert_eq!(status_of(&s, 2), PlanStatus::Done, "the nearer start (3 Aug, one day away) is served first");
        assert_eq!(status_of(&s, 3), PlanStatus::Missed);
        // Other kinds of leave never fulfil an entry.
        dt.leaves = vec![Leave { id: 1, employee_id: 1, kind: LeaveType::Unpaid, start: Some(d(2026, 8, 3)), end: Some(d(2026, 8, 16)), ..Default::default() }];
        let s = Snapshot::build(&dt, d(2026, 10, 23));
        assert_eq!(status_of(&s, 2), PlanStatus::Missed);
    }

    #[test]
    fn reminders_have_two_stages_and_only_for_active_employees() {
        let mut dt = data(d(2026, 10, 23));
        dt.leaves = vec![
            leave(1, 1, d(2026, 11, 6), 14.0),  // 14 days: near
            leave(2, 1, d(2026, 11, 7), 14.0),  // 15 days, within a month: early
            leave(3, 2, d(2026, 11, 23), 14.0), // exactly one month: early
            leave(4, 2, d(2026, 11, 24), 14.0), // one day too far
            leave(5, 3, d(2026, 11, 1), 14.0),  // dismissed employee
            leave(6, 2, d(2026, 10, 23), 14.0), // starts today: already going, no reminder
        ];
        dt.employees[2].status = Status::Dismissed;
        let s = Snapshot::build(&dt, d(2026, 10, 23));
        let got: Vec<(i64, Band)> = s.reminders.iter().map(|r| (r.days, r.band)).collect();
        assert_eq!(got, [(14, Band::Near), (15, Band::Far), (31, Band::Far)]);
        assert_eq!((s.reminders_urgent(), s.reminders_early()), (1, 2));
        assert_eq!(s.reminders[0].notify_by(), Some(d(2026, 10, 23)), "the notice is due two weeks before the start");
    }

    #[test]
    fn the_stages_can_be_changed_in_the_settings() {
        let mut dt = data(d(2026, 10, 23));
        dt.settings.remind_near_days = 7;
        dt.settings.remind_far_months = 2;
        dt.leaves = vec![leave(1, 1, d(2026, 10, 30), 14.0), leave(2, 1, d(2026, 11, 1), 14.0), leave(3, 2, d(2026, 12, 23), 14.0), leave(4, 2, d(2026, 12, 24), 14.0)];
        let s = Snapshot::build(&dt, d(2026, 10, 23));
        let got: Vec<(i64, Band)> = s.reminders.iter().map(|r| (r.days, r.band)).collect();
        assert_eq!(got, [(7, Band::Near), (9, Band::Far), (61, Band::Far)]);
    }

    #[test]
    fn schedule_entries_without_a_journal_record_remind_too() {
        let mut dt = data(d(2026, 10, 23));
        dt.plans = vec![
            plan(1, 1, d(2026, 11, 2), 14.0),  // 10 days, not in the journal: near
            plan(2, 2, d(2026, 11, 10), 14.0), // in the journal: reminded through the journal record
            plan(3, 2, d(2026, 10, 10), 14.0), // 13 days ago, not in the journal: missed
            plan(4, 2, d(2026, 8, 10), 14.0),  // long ago: only on the schedule screen
        ];
        dt.leaves = vec![leave(1, 2, d(2026, 11, 10), 14.0)];
        let s = Snapshot::build(&dt, d(2026, 10, 23));
        let kinds: Vec<(ReminderKind, Band)> = s.reminders.iter().map(|r| (r.kind, r.band)).collect();
        assert_eq!(kinds, [(ReminderKind::PlanMissed, Band::Missed), (ReminderKind::PlanStart, Band::Near), (ReminderKind::Start, Band::Far)]);
        assert!(s.reminders[2].plan_ix.is_some(), "the journal reminder knows its schedule entry");
        assert_eq!(s.reminders[0].days, -13);
    }

    #[test]
    fn end_of_maternity_and_childcare_leave_is_announced_two_weeks_ahead() {
        let mut dt = data(d(2026, 10, 23));
        let long = |id, emp, kind, start, end| Leave { id, employee_id: emp, kind, start: Some(start), end: Some(end), ..Default::default() };
        dt.leaves = vec![
            long(1, 1, LeaveType::Maternity, d(2026, 6, 1), d(2026, 11, 5)),  // ends in 13 days
            long(2, 2, LeaveType::Childcare, d(2024, 1, 1), d(2026, 11, 6)),  // ends in 14 days
            long(3, 2, LeaveType::Childcare, d(2023, 1, 1), d(2026, 11, 7)),  // 15 days: not yet
            long(4, 1, LeaveType::Unpaid, d(2026, 10, 1), d(2026, 10, 25)),   // unpaid leave: no reminder
        ];
        let s = Snapshot::build(&dt, d(2026, 10, 23));
        let got: Vec<(ReminderKind, i64)> = s.reminders.iter().map(|r| (r.kind, r.days)).collect();
        assert_eq!(got, [(ReminderKind::End(LeaveType::Maternity), 13), (ReminderKind::End(LeaveType::Childcare), 14)]);
        assert_eq!(s.reminders[0].back_to_work(), Some(d(2026, 11, 6)));
        // A leave that ended yesterday is over.
        dt.settings.as_of_manual = Some(d(2026, 11, 6));
        let s = Snapshot::build(&dt, d(2026, 11, 6));
        assert!(s.reminders.iter().all(|r| r.kind != ReminderKind::End(LeaveType::Maternity)));
    }
}
