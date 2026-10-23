//! Dev tool: prints everything the engine computes for a database file, one line per fact,
//! for fixed calculation dates. Run it before and after a change of the engine or the schema
//! and compare the outputs: for data without the new kinds of leave they must be identical.
//!
//! `cargo run -p otpuska-core --example baseline -- copy-of-otpuska.db > before.tsv`
//!
//! Works on a copy: opening a database with a newer version of the program upgrades it.

use otpuska_core::{date, engine, Db};

fn main() -> anyhow::Result<()> {
    let file = std::env::args().nth(1).expect("path to a copy of otpuska.db");
    let db = Db::open(file.as_ref())?;
    let data = db.load()?;
    for as_of in ["2026-10-23", "2027-03-15", "2024-01-01"] {
        let as_of = date::parse_iso(as_of).expect("date");
        let mut data = data.clone();
        data.settings.as_of_manual = Some(as_of);
        let snap = engine::Snapshot::build(&data, as_of);
        println!("## as of {as_of}");
        for r in &snap.emps {
            let c = &r.calc;
            println!(
                "emp {} {:?} state={:?} years={:?} started={} available={} tb={} te={} lb={:?} le={:?} ent={:?} left={:?} early={:?} dl={:?} days={:?} dates={:?}",
                r.emp.id,
                r.emp.status,
                c.state,
                c.years,
                c.started,
                c.available,
                c.taken_base,
                c.taken_extra,
                c.left_base,
                c.left_extra,
                c.entitled,
                c.left,
                c.early_period,
                c.deadline(),
                c.days_left(),
                c.early_period_dates(),
            );
            for p in engine::periods(&r.emp, c, &snap.params) {
                println!(
                    "  year {} {}..{} avail={} ent={} taken={:?} left={:?} extra={:?} dl={} days={} tone={:?}",
                    p.k, p.start, p.end, p.available, p.entitled, p.taken, p.left, p.left_extra, p.deadline, p.days_to_deadline, p.tone
                );
            }
            for y in 2015..2036 {
                if let Some(p) = engine::period_for_year(&r.emp, c, &snap.params, y) {
                    println!("  in-year {y} -> {}", p.k);
                }
            }
        }
        for l in &snap.leaves {
            println!("leave {} emp={:?} end={:?} status={:?} left={:?}", l.leave.id, l.emp_ix, l.end, l.status, l.emp_left);
        }
        println!("order {:?}", snap.summary_order(None));
        println!("counts {:?}", snap.counts(None));
        println!("upcoming {:?}", snap.upcoming());
        println!("warnings {:?}", snap.warnings());
        println!("depts {:?}", snap.dept_report(None));
    }
    Ok(())
}
