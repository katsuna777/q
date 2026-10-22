//! Printable reports as self-contained HTML: opened in the browser, printed or saved as PDF
//! from there (A4 landscape, 10 pt text, repeated table headers).

use crate::date;
use crate::engine::{self, PeriodTone, Snapshot, State};
use crate::model::Status;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

const CSS: &str = r#"
@page { size: A4 landscape; margin: 12mm; }
* { box-sizing: border-box; }
body { font: 10pt/1.35 -apple-system, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif; color: #111; margin: 0; }
h1 { font-size: 15pt; margin: 0 0 2pt; }
h2 { font-size: 12pt; margin: 14pt 0 5pt; }
.sub { color: #555; margin: 0 0 9pt; }
.bar { display: none; }
table { border-collapse: collapse; width: 100%; }
th, td { border: 0.5pt solid #b5b5b5; padding: 2.5pt 5pt; text-align: left; vertical-align: top; }
th { background: #eeeef0; font-weight: 600; }
thead { display: table-header-group; }
tr { break-inside: avoid; }
.n { text-align: right; white-space: nowrap; }
.c { text-align: center; }
.nw { white-space: nowrap; }
.muted { color: #666; }
.total td { font-weight: 700; background: #f6f6f7; }
.kpis { display: flex; gap: 7pt; margin: 0 0 9pt; }
.kpi { border: 0.5pt solid #b5b5b5; border-radius: 4pt; padding: 3pt 9pt; min-width: 92pt; }
.kpi b { display: block; font-size: 14pt; }
.kpi span { color: #555; font-size: 8.5pt; }
.page { break-after: page; }
.page:last-child { break-after: auto; }
.st-overdue { background: #FFC7CE; color: #9C0006; }
.st-burning { background: #FFD9B3; color: #9C4A00; }
.st-soon { background: #FFF2B3; color: #7F6000; }
.st-ok { background: #E2F0D9; color: #375623; }
.st-info { background: #DDEBF7; color: #1F4E79; }
.st-check { background: #F8CBAD; color: #833C0B; }
.st-susp { background: #E4DFEC; color: #5B3F86; }
.st-gray { background: #EDEDED; color: #666; }
.st-none { color: #C00000; }
.dim { color: #888; }
@media screen {
  body { padding: 18px 28px 40px; max-width: 1280px; margin: 0 auto; background: #fff; }
  .bar { display: block; margin-bottom: 14px; padding: 8px 12px; background: #f3f4f6; border-radius: 8px; color: #444; }
  .bar button { font: inherit; padding: 5px 12px; margin-right: 10px; border: 1px solid #bbb; border-radius: 6px; background: #fff; cursor: pointer; }
  .page { margin-bottom: 28px; }
}
@media print { .bar { display: none !important; } }
"#;

fn document(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html lang=\"ru\"><head><meta charset=\"utf-8\"><title>{}</title><style>{CSS}</style></head><body>\
         <div class=\"bar\"><button onclick=\"window.print()\">Печать / сохранить как PDF</button>Страница настроена на A4 альбомная, шрифт 10 pt. В окне печати выберите масштаб 100%.</div>\
         {body}</body></html>",
        esc(title)
    )
}

/// Title, calculation date and what exactly is printed: the active filters and the row count.
fn head(title: &str, snap: &Snapshot, filters: &[String], shown: usize, total: usize) -> String {
    let scope = if filters.is_empty() {
        format!("Без фильтров — все записи: {total}")
    } else {
        format!("Фильтры: {} — показано {shown} из {total}", filters.join(" · "))
    };
    format!("<h1>{}</h1><p class=\"sub\">На дату {} · {}</p>", esc(title), date::fmt(snap.params.as_of), esc(&scope))
}

fn nothing() -> String {
    "<p class=\"muted\">По выбранным фильтрам ничего не найдено.</p>".to_string()
}

fn state_class(s: State) -> &'static str {
    match s {
        State::Overdue => "st-overdue",
        State::Burning => "st-burning",
        State::Soon => "st-soon",
        State::Normal | State::AllUsed => "st-ok",
        State::Advance => "st-info",
        State::CheckBooks => "st-check",
        State::Suspended => "st-susp",
        State::Dismissed | State::NoRight => "st-gray",
        State::NoHireDate => "st-none",
    }
}

fn tone_class(t: PeriodTone) -> &'static str {
    match t {
        PeriodTone::Future => "st-gray",
        PeriodTone::Suspended => "st-susp",
        PeriodTone::Used => "st-ok",
        PeriodTone::Overdue => "st-overdue",
        PeriodTone::Burning => "st-burning",
        PeriodTone::Soon => "st-soon",
        PeriodTone::Open => "st-info",
    }
}

fn leave_class(s: Option<engine::LeaveStatus>) -> &'static str {
    match s {
        Some(engine::LeaveStatus::InLeave) => "st-ok",
        Some(engine::LeaveStatus::Planned) => "st-info",
        Some(engine::LeaveStatus::Done) => "",
        Some(engine::LeaveStatus::Undated) => "st-gray",
        None => "st-overdue",
    }
}

fn kpi(value: usize, label: &str) -> String {
    format!("<div class=\"kpi\"><b>{value}</b><span>{}</span></div>", esc(label))
}

// ---- reports ------------------------------------------------------------------------

/// `rows`: indexes into `snap.emps` exactly as the summary screen shows them.
pub fn summary(snap: &Snapshot, rows: &[usize], filters: &[String]) -> String {
    let counts = snap.counts_for(rows);
    let mut b = head("Учёт отпусков — сводка", snap, filters, rows.len(), snap.active_count());
    b += &format!(
        "<div class=\"kpis\">{}{}{}{}{}{}</div>",
        kpi(counts.overdue, "просрочено"),
        kpi(counts.burning, &format!("горит (до срока ≤ {} дн.)", snap.params.burning_days)),
        kpi(counts.soon, &format!("скоро (до срока ≤ {} дн.)", snap.params.soon_days)),
        kpi(counts.ok, "всё в порядке"),
        kpi(counts.suspended, "приостановка ТД"),
        kpi(counts.on_leave, "в отпуске сейчас"),
    );
    if rows.is_empty() {
        return document("Сводка по отпускам", &(b + &nothing()));
    }
    b += "<table><thead><tr><th>№</th><th>ФИО</th><th>Подразделение</th><th>Должность</th><th>Состояние</th><th class=\"n\">Остаток, дн.</th><th class=\"n\">в т.ч. доп.</th><th>Крайний срок</th><th class=\"n\">До срока, дн.</th></tr></thead><tbody>";
    for (n, &i) in rows.iter().enumerate() {
        let r = &snap.emps[i];
        let c = &r.calc;
        b += &format!(
            "<tr><td class=\"n muted\">{}</td><td>{}</td><td>{}</td><td>{}</td><td class=\"{} nw\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"nw\">{}</td><td class=\"n\">{}</td></tr>",
            n + 1,
            esc(&r.emp.fio),
            esc(&r.dept),
            esc(&r.emp.position),
            state_class(c.state),
            c.state.label(),
            engine::fmt_days_opt(c.left),
            engine::fmt_days_opt(c.left_extra_shown()),
            date::fmt_opt(c.deadline()),
            c.days_left().map(|d| d.to_string()).unwrap_or_default(),
        );
    }
    b += "</tbody></table>";
    document("Сводка по отпускам", &b)
}

fn leaves_table(snap: &Snapshot, rows: &[usize]) -> String {
    if rows.is_empty() {
        return nothing();
    }
    let mut b = String::from("<table><thead><tr><th>№</th><th>ФИО</th><th>Подразделение</th><th>Статус</th><th class=\"n\">Дней</th><th>Даты отпуска</th><th>Примечание</th></tr></thead><tbody>");
    for (n, &i) in rows.iter().enumerate() {
        let l = &snap.leaves[i];
        let dates = match (l.leave.start, l.end) {
            (Some(s), Some(e)) => format!("{} – {}", date::fmt(s), date::fmt(e)),
            _ => String::new(),
        };
        b += &format!(
            "<tr><td class=\"n muted\">{}</td><td>{}</td><td>{}</td><td class=\"{} nw\">{}</td><td class=\"n\">{}</td><td class=\"nw\">{}</td><td>{}</td></tr>",
            n + 1,
            esc(&l.fio),
            esc(&l.dept),
            leave_class(l.status),
            l.status.map(|s| s.label()).unwrap_or(""),
            engine::fmt_days(l.leave.total_days()),
            dates,
            esc(&l.leave.note),
        );
    }
    b + "</tbody></table>"
}

/// `leaves`: the first table as the screen shows it; `dept` limits the second one.
pub fn reports(snap: &Snapshot, leaves: &[usize], dept: Option<i64>, filters: &[String]) -> String {
    let mut b = head("Отчёты по отпускам", snap, filters, leaves.len(), snap.leaves.len());
    b += "<h2>Отпуска</h2>";
    b += &leaves_table(snap, leaves);
    b += "<h2>По подразделениям (действующие сотрудники)</h2>";
    let (depts, total) = snap.dept_report(dept);
    b += "<table><thead><tr><th>Подразделение</th><th class=\"n\">Требуют внимания, чел.</th><th class=\"n\">Дней к использованию</th><th class=\"n\">Работает, чел.</th><th class=\"n\">Просроч.</th><th class=\"n\">Горит</th><th class=\"n\">Скоро</th><th class=\"n\">Приост. ТД</th></tr></thead><tbody>";
    for (i, d) in std::iter::once(&total).chain(depts.iter()).enumerate() {
        b += &format!(
            "<tr{}><td>{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td></tr>",
            if i == 0 { " class=\"total\"" } else { "" },
            esc(&d.name),
            d.attention,
            engine::fmt_days(d.days_left),
            d.working,
            d.overdue,
            d.burning,
            d.soon,
            d.suspended,
        );
    }
    b += "</tbody></table>";
    document("Отчёты по отпускам", &b)
}

/// Working years of the employees in `rows`, two calendar years per page.
pub fn periods(snap: &Snapshot, rows: &[usize], first_year: i32, span: usize, filters: &[String]) -> String {
    let total = snap.emps.iter().filter(|r| r.emp.status != Status::Dismissed).count();
    let mut b = String::new();
    let years: Vec<i32> = (0..span as i32).map(|y| first_year + y).collect();
    if rows.is_empty() {
        b = head("Периоды", snap, filters, 0, total) + &nothing();
    }
    for chunk in years.chunks(2).filter(|_| !rows.is_empty()) {
        b += "<div class=\"page\">";
        b += &head(
            &format!("Периоды: рабочие годы, начавшиеся в {}", chunk.iter().map(|y| y.to_string()).collect::<Vec<_>>().join(" и ")),
            snap,
            filters,
            rows.len(),
            total,
        );
        b += "<table><thead><tr><th rowspan=\"2\">№</th><th rowspan=\"2\">ФИО</th><th rowspan=\"2\">Состояние</th>";
        for y in chunk {
            b += &format!("<th colspan=\"4\" class=\"c\">Рабочие годы, начавшиеся в {y} г.</th>");
        }
        b += "</tr><tr>";
        for _ in chunk {
            b += "<th>Период</th><th class=\"n\">Отгул.</th><th class=\"n\">Остаток</th><th class=\"n\">в т.ч. доп.</th>";
        }
        b += "</tr></thead><tbody>";
        for (n, &i) in rows.iter().enumerate() {
            let r = &snap.emps[i];
            b += &format!("<tr><td class=\"n muted\">{}</td><td>{}</td><td class=\"{} nw\">{}</td>", n + 1, esc(&r.emp.fio), state_class(r.calc.state), r.calc.state.label());
            for &y in chunk {
                match engine::period_for_year(&r.emp, &r.calc, &snap.params, y) {
                    Some(pr) => {
                        b += &format!(
                            "<td class=\"nw\">{}–{}</td><td class=\"n\">{}</td><td class=\"n {}\">{}</td><td class=\"n\">{}</td>",
                            date::fmt_short(pr.start),
                            date::fmt_short(pr.end),
                            engine::fmt_days_opt(pr.taken),
                            tone_class(pr.tone),
                            engine::fmt_days_opt(pr.left),
                            engine::fmt_days_opt(pr.left_extra),
                        );
                    }
                    None => b += "<td></td><td></td><td></td><td></td>",
                }
            }
            b += "</tr>";
        }
        b += "</tbody></table></div>";
    }
    document("Периоды отпусков", &b)
}

/// `rows`: indexes into `snap.leaves` in the order of the journal screen.
pub fn journal(snap: &Snapshot, rows: &[usize], filters: &[String]) -> String {
    let mut b = head("Журнал отпусков", snap, filters, rows.len(), snap.leaves.len());
    if rows.is_empty() {
        return document("Журнал отпусков", &(b + &nothing()));
    }
    b += "<table><thead><tr><th>№</th><th>ФИО</th><th>Подразделение</th><th>Начало</th><th>Окончание</th><th class=\"n\">Основной, дн.</th><th class=\"n\">Доп., дн.</th><th>Статус</th><th class=\"n\">Остаток, дн.</th><th>Примечание</th></tr></thead><tbody>";
    for (n, &i) in rows.iter().enumerate() {
        let l = &snap.leaves[i];
        b += &format!(
            "<tr><td class=\"n muted\">{}</td><td>{}</td><td>{}</td><td class=\"nw\">{}</td><td class=\"nw\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"{} nw\">{}</td><td class=\"n\">{}</td><td>{}</td></tr>",
            n + 1,
            esc(&l.fio),
            esc(&l.dept),
            date::fmt_opt(l.leave.start),
            date::fmt_opt(l.end),
            if l.leave.base_days != 0.0 { engine::fmt_days(l.leave.base_days) } else { String::new() },
            if l.leave.extra_days != 0.0 { engine::fmt_days(l.leave.extra_days) } else { String::new() },
            leave_class(l.status),
            l.status.map(|s| s.label()).unwrap_or("нет дней"),
            engine::fmt_days_opt(l.emp_left),
            esc(&l.leave.note),
        );
    }
    b += "</tbody></table>";
    document("Журнал отпусков", &b)
}

/// `rows`: indexes into `snap.emps` in the order of the directory screen.
pub fn employees(snap: &Snapshot, rows: &[usize], filters: &[String]) -> String {
    let mut b = head("Сотрудники", snap, filters, rows.len(), snap.emps.len());
    if rows.is_empty() {
        return document("Сотрудники", &(b + &nothing()));
    }
    b += "<table><thead><tr><th>№</th><th>ФИО</th><th>Должность</th><th>Подразделение</th><th>Принят</th><th>Статус</th><th class=\"n\">Положено</th><th class=\"n\">Отгуляно</th><th class=\"n\">Остаток</th><th>Состояние</th><th>Крайний срок</th></tr></thead><tbody>";
    for &i in rows {
        let r = &snap.emps[i];
        let c = &r.calc;
        let dim = if r.emp.status == Status::Dismissed { " class=\"dim\"" } else { "" };
        b += &format!(
            "<tr{dim}><td class=\"n\">{}</td><td>{}</td><td>{}</td><td>{}</td><td class=\"nw\">{}</td><td class=\"nw\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"{} nw\">{}</td><td class=\"nw\">{}</td></tr>",
            r.emp.id,
            esc(&r.emp.fio),
            esc(&r.emp.position),
            esc(&r.dept),
            date::fmt_opt(r.emp.hire_date),
            r.emp.status.label(),
            engine::fmt_days_opt(c.entitled),
            engine::fmt_days(c.taken),
            engine::fmt_days_opt(c.left),
            state_class(c.state),
            c.state.label(),
            date::fmt_opt(c.deadline()),
        );
    }
    b += "</tbody></table>";
    document("Сотрудники", &b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date::Date;
    use crate::filter::{EmpFilter, LeaveFilter, LeaveShow, RestFilter, Scope, Took};
    use crate::model::*;

    fn snap() -> Snapshot {
        let emp = |id, fio: &str, dept| Employee {
            id,
            fio: fio.into(),
            department_id: Some(dept),
            hire_date: Date::from_ymd_opt(2022, 9, 28),
            ..Default::default()
        };
        let data = Data {
            settings: Settings { as_of_manual: Date::from_ymd_opt(2026, 10, 22), ..Default::default() },
            departments: vec![Department { id: 1, name: "Цех <1> & Ко".into() }, Department { id: 2, name: "Офис".into() }],
            employees: vec![emp(1, "Иванов \"Иван\"", 1), emp(2, "Петров", 2), emp(3, "Сидоров", 2)],
            leaves: vec![Leave { id: 1, employee_id: 1, start: Date::from_ymd_opt(2026, 10, 20), base_days: 14.0, ..Default::default() }],
        };
        Snapshot::build(&data, date::today())
    }

    #[test]
    fn reports_render_and_escape() {
        let s = snap();
        let all = EmpFilter::default();
        let rows = s.summary_rows(&all);
        let periods_rows = s.period_rows(&all, RestFilter::Any, 2022, 8);
        let leaves = s.leave_rows(&LeaveFilter::default());
        let emps = s.emp_rows(&all, Scope::All, true);
        for html in [
            summary(&s, &rows, &[]),
            reports(&s, &leaves, None, &[]),
            periods(&s, &periods_rows, 2022, 8, &[]),
            journal(&s, &leaves, &[]),
            employees(&s, &emps, &[]),
        ] {
            assert!(html.starts_with("<!doctype html>"));
            assert!(html.contains("Иванов &quot;Иван&quot;") || html.contains("Цех &lt;1&gt;"));
            assert!(!html.contains("Цех <1>"));
            assert!(html.contains("Без фильтров — все записи"));
        }
        assert!(summary(&s, &rows, &[]).contains("st-overdue"));
        assert_eq!(periods(&s, &periods_rows, 2022, 8, &[]).matches("class=\"page\"").count(), 4);
        assert!(reports(&s, &leaves, None, &[]).contains("В отпуске"));
    }

    #[test]
    fn print_contains_only_the_filtered_rows() {
        let s = snap();
        // Department "Офис" only: Петров and Сидоров, not Иванов.
        let f = EmpFilter { dept: Some(2), ..Default::default() };
        let rows = s.summary_rows(&f);
        let html = summary(&s, &rows, &f.describe(&s, false));
        assert!(html.contains("Петров") && html.contains("Сидоров"));
        assert!(!html.contains("Иванов"));
        assert!(html.contains("Фильтры: Подразделение: Офис — показано 2 из 3"));

        // Took leave: only Иванов; the printed list is the same as on screen.
        let f = EmpFilter { took: Took::Taken, ..Default::default() };
        let rows = s.emp_rows(&f, Scope::All, true);
        let html = employees(&s, &rows, &f.describe(&s, true));
        assert!(html.contains("Иванов") && !html.contains("Петров"));
        assert!(html.contains("Отпуск: был в отпуске (за всё время)"));

        // Journal with a department that has no leaves: nothing found, header still says why.
        let lf = LeaveFilter { dept: Some(2), ..Default::default() };
        let html = journal(&s, &s.leave_rows(&lf), &lf.describe(&s, LeaveShow::All));
        assert!(html.contains("ничего не найдено"));
        assert!(html.contains("показано 0 из 1"));

        // Periods: the screen's rows and the window decide what is printed.
        let f = EmpFilter { search: "пет".into(), ..Default::default() };
        let rows = s.period_rows(&f, RestFilter::Any, 2022, 4);
        let html = periods(&s, &rows, 2022, 4, &f.describe(&s, false));
        assert!(html.contains("Петров") && !html.contains("Сидоров"));
        assert_eq!(html.matches("class=\"page\"").count(), 2);
    }
}
