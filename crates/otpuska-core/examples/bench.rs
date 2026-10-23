//! Dev tool: how long the whole recalculation takes (`Snapshot::build`, done after every edit)
//! and how it grows with the size of the data.
//! `cargo run --release -p otpuska-core --example bench -- copy-of-otpuska.db`
//!
//! Works on a copy: opening a database with a newer version of the program upgrades it.

use otpuska_core::model::{Data, Employee, Leave, LeaveType, Plan};
use otpuska_core::{date, engine, Db};
use std::time::Instant;

fn time(label: &str, runs: u32, mut f: impl FnMut()) {
    f(); // warm up
    let start = Instant::now();
    for _ in 0..runs {
        f();
    }
    let ms = start.elapsed().as_secs_f64() * 1000.0 / f64::from(runs);
    println!("{label:<46} {ms:>8.3} ms");
}

/// `times` copies of the data, with ids shifted, plus unpaid and childcare leave and a schedule
/// for part of the employees: the worst case the program is likely to meet.
fn scaled(base: &Data, times: usize) -> Data {
    let mut d = base.clone();
    let (max_emp, max_leave) = (base.employees.iter().map(|e| e.id).max().unwrap_or(0), base.leaves.iter().map(|l| l.id).max().unwrap_or(0));
    for k in 1..times as i64 {
        for e in &base.employees {
            d.employees.push(Employee { id: e.id + k * (max_emp + 1), ..e.clone() });
        }
        for l in &base.leaves {
            d.leaves.push(Leave { id: l.id + k * (max_leave + 1), employee_id: l.employee_id + k * (max_emp + 1), ..l.clone() });
        }
    }
    let day = |y, m, dd| date::Date::from_ymd_opt(y, m, dd).expect("date");
    for (i, e) in d.employees.clone().iter().enumerate() {
        let id = 1_000_000 + i as i64 * 10;
        if i % 3 == 0 {
            d.leaves.push(Leave { id, employee_id: e.id, kind: LeaveType::Unpaid, start: Some(day(2022, 3, 1)), end: Some(day(2022, 3, 24)), ..Default::default() });
        }
        if i % 7 == 0 {
            d.leaves.push(Leave { id: id + 1, employee_id: e.id, kind: LeaveType::Childcare, start: Some(day(2023, 1, 9)), end: Some(day(2025, 8, 9)), ..Default::default() });
        }
        if i % 2 == 0 {
            d.plans.push(Plan { id: id + 2, employee_id: e.id, start: Some(day(2026, 11, 2)), base_days: 14.0, ..Default::default() });
        }
    }
    d
}

fn main() -> anyhow::Result<()> {
    let file = std::env::args().nth(1).expect("path to a copy of otpuska.db");
    let db = Db::open(file.as_ref())?;
    let data = db.load()?;
    let today = date::parse_iso("2026-10-23").expect("date");
    println!("{} employees, {} journal records", data.employees.len(), data.leaves.len());
    time("db.load()", 50, || {
        std::hint::black_box(db.load().expect("load"));
    });
    for times in [1usize, 3, 10] {
        let d = scaled(&data, times);
        let runs = if times >= 10 { 20 } else { 100 };
        time(&format!("Snapshot::build, {} employees, {} records, {} plans", d.employees.len(), d.leaves.len(), d.plans.len()), runs, || {
            std::hint::black_box(engine::Snapshot::build(&d, today));
        });
    }
    let d = scaled(&data, 3);
    let snap = engine::Snapshot::build(&d, today);
    use otpuska_core::filter::*;
    time("  summary_rows (filter + sort)", 500, || {
        std::hint::black_box(snap.summary_rows(&EmpFilter::default()));
    });
    time("  leave_rows", 500, || {
        std::hint::black_box(snap.leave_rows(&LeaveFilter::default()));
    });
    time("  plan_rows (x2 as the screen does)", 500, || {
        std::hint::black_box(snap.plan_rows(&PlanFilter::default(), true));
        std::hint::black_box(snap.plan_rows(&PlanFilter::default(), false));
    });
    time("  reminder_rows (x5 counts as the screen does)", 500, || {
        for s in ReminderShow::ALL {
            std::hint::black_box(snap.reminder_rows(&ReminderFilter { show: s, ..Default::default() }));
        }
    });
    time("  period_rows (8 years, rest filter on)", 200, || {
        std::hint::black_box(snap.period_rows(&EmpFilter::default(), RestFilter::HasLeft, 2022, 8));
    });
    time("  period_for_year x 8 for every employee", 200, || {
        for r in &snap.emps {
            for y in 2022..2030 {
                std::hint::black_box(engine::period_for_year(&r.emp, &r.calc, &snap.params, y));
            }
        }
    });
    println!("reminders: {}", snap.reminders.len());
    Ok(())
}
