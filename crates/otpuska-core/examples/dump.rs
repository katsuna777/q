//! Dev tool: imports a workbook and prints the computed state of every employee.
//! `cargo run -p otpuska-core --example dump -- book.xlsx 2026-10-22`

use otpuska_core::{date, engine, xlsx_import, Db};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let file = args.next().expect("workbook path");
    let as_of = args.next().and_then(|s| date::parse_iso(&s)).unwrap_or_else(date::today);
    let import = xlsx_import::read_workbook(file.as_ref())?;
    eprintln!("{}", import.summary());
    for w in &import.warnings {
        eprintln!("warning: {w}");
    }
    let mut db = Db::open_memory()?;
    db.replace_all(&import)?;
    let mut data = db.load()?;
    data.settings.as_of_manual = Some(as_of);
    let snap = engine::Snapshot::build(&data, as_of);
    println!("ix\tfio\tstate\tentitled\ttaken\tleft\tdeadline\tdays\tleft_extra");
    for (i, r) in snap.emps.iter().enumerate() {
        let c = &r.calc;
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            i + 1,
            r.emp.fio,
            c.state.label(),
            engine::fmt_days_opt(c.entitled),
            engine::fmt_days(c.taken),
            engine::fmt_days_opt(c.left),
            date::fmt_opt(c.deadline()),
            c.days_left().map(|d| d.to_string()).unwrap_or_default(),
            engine::fmt_days_opt(c.left_extra_shown()),
        );
    }
    if std::env::var_os("DUMP_PERIODS").is_some() {
        println!("---periods");
        let first = snap.default_first_year();
        for (i, r) in snap.emps.iter().enumerate() {
            for y in 0..8 {
                let p = engine::period_for_year(&r.emp, &r.calc, &snap.params, first + y);
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    i + 1,
                    y,
                    p.as_ref().map(|p| format!("{}-{}", date::fmt_short(p.start), date::fmt_short(p.end))).unwrap_or_default(),
                    engine::fmt_days_opt(p.as_ref().and_then(|p| p.taken)),
                    engine::fmt_days_opt(p.as_ref().and_then(|p| p.left)),
                    engine::fmt_days_opt(p.as_ref().and_then(|p| p.left_extra)),
                    p.as_ref().map(|p| p.days_to_deadline.to_string()).unwrap_or_default(),
                );
            }
        }
    }
    if let Some(dir) = std::env::var_os("DUMP_HTML") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir)?;
        let first = snap.default_first_year();
        use otpuska_core::filter::{EmpFilter, LeaveFilter, RestFilter, Scope};
        let all = EmpFilter::default();
        let leaves = snap.leave_rows(&LeaveFilter::default());
        for (name, html) in [
            ("svodka", otpuska_core::print::summary(&snap, &snap.summary_rows(&all), &[])),
            ("otchety", otpuska_core::print::reports(&snap, &leaves, None, &[])),
            ("periody", otpuska_core::print::periods(&snap, &snap.period_rows(&all, RestFilter::Any, first, 8), first, 8, &[])),
            ("zhurnal", otpuska_core::print::journal(&snap, &leaves, &[])),
            ("sotrudniki", otpuska_core::print::employees(&snap, &snap.emp_rows(&all, Scope::All, true), &[])),
        ] {
            std::fs::write(dir.join(format!("{name}.html")), html)?;
        }
    }
    let counts = snap.counts(None);
    eprintln!(
        "overdue={} burning={} soon={} ok={} suspended={} on_leave={}",
        counts.overdue, counts.burning, counts.soon, counts.ok, counts.suspended, counts.on_leave
    );
    Ok(())
}
