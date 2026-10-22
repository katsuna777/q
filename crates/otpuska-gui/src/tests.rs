//! Widget and application-level tests (no GPU needed).

use crate::app::{App, Route, Target};
use crate::theme;
use crate::ui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use otpuska_core::config::Config;
use otpuska_core::date::Date;
use otpuska_core::model::*;
use otpuska_core::{Db, State};

fn d(y: i32, m: u32, day: u32) -> Date {
    Date::from_ymd_opt(y, m, day).unwrap()
}

fn harness<S: 'static>(state: S, mut f: impl FnMut(&mut egui::Ui, &mut S) + 'static) -> Harness<'static, S> {
    let mut fonts_ready = false;
    let mut h = Harness::new_ui_state(
        move |ui, s| {
            // Fonts are applied at the start of the next frame, so the first one draws nothing.
            if !fonts_ready {
                theme::install_fonts(ui.ctx());
                fonts_ready = true;
                ui.ctx().request_repaint();
                return;
            }
            ui.ctx().data_mut(|d| d.insert_temp(egui::Id::NULL, theme::palette(true, "mono")));
            f(ui, s)
        },
        state,
    );
    h.run();
    h
}

#[test]
fn date_field_takes_typed_dates() {
    let mut h = harness(None::<Date>, |ui, v| {
        ui::date_field(ui, "date", v, "дд.мм.гггг", 160.0);
    });
    let input = h.get_by_role(egui::accesskit::Role::TextInput);
    input.focus();
    input.type_text("28.09.2022");
    h.run();
    assert_eq!(*h.state(), Some(d(2022, 9, 28)));

    // Invalid text keeps the last good value; clearing empties it.
    let input = h.get_by_role(egui::accesskit::Role::TextInput);
    input.focus();
    input.type_text("9");
    h.run();
    assert_eq!(*h.state(), Some(d(2022, 9, 28)), "28.09.20229 is not a date -> unchanged");
}

#[test]
fn date_field_shows_the_value_and_clears() {
    let mut h = harness(Some(d(2024, 3, 1)), |ui, v| {
        ui::date_field(ui, "date", v, "дд.мм.гггг", 160.0);
    });
    assert_eq!(h.get_by_role(egui::accesskit::Role::TextInput).value().as_deref(), Some("01.03.2024"));
    // External change while not focused is reflected.
    *h.state_mut() = Some(d(2025, 12, 31));
    h.run();
    assert_eq!(h.get_by_role(egui::accesskit::Role::TextInput).value().as_deref(), Some("31.12.2025"));
}

#[test]
fn number_field_accepts_comma_and_empty() {
    let mut h = harness(None::<f64>, |ui, v| {
        ui::num_field(ui, "n", v, "0", 100.0);
    });
    let input = h.get_by_role(egui::accesskit::Role::TextInput);
    input.focus();
    input.type_text("14,5");
    h.run();
    assert_eq!(*h.state(), Some(14.5));
    assert_eq!(ui::parse_num("abc"), None);
    assert_eq!(ui::parse_num("-3"), None);
    assert_eq!(ui::parse_num(" 7 "), Some(7.0));
}

fn test_app() -> App {
    let ctx = egui::Context::default();
    let mut config = Config::default();
    config.ui.theme = Some("dark".into());
    App::new(&ctx, Db::open_memory().unwrap(), config).unwrap()
}

#[test]
fn app_workflow_hire_book_leave_dismiss() {
    let mut app = test_app();
    assert!(app.snap.emps.is_empty());
    assert!(app.add_department("Цех №1"));
    assert!(!app.add_department("Цех №1"), "duplicate department is rejected");
    let dept = app.snap.departments[0].id;

    let hired = Employee { fio: "Иванов Иван".into(), department_id: Some(dept), hire_date: Some(d(2024, 1, 10)), ..Default::default() };
    let id = app.save_employee(&hired).expect("saved");
    assert_eq!(app.snap.emp_by_id(id).unwrap().dept, "Цех №1");
    let before = app.snap.emp_by_id(id).unwrap().calc.left;

    // Book 14 days: the remainder drops by exactly 14.
    assert!(app.save_leave(&Leave { employee_id: id, start: Some(d(2025, 6, 2)), base_days: 14.0, ..Default::default() }));
    let after = app.snap.emp_by_id(id).unwrap().calc.left;
    assert_eq!(before.unwrap() - after.unwrap(), 14.0);
    assert_eq!(app.snap.leaves.len(), 1);

    // Edit the leave, then delete it.
    let mut leave = app.snap.leaves[0].leave.clone();
    leave.base_days = 28.0;
    assert!(app.save_leave(&leave));
    assert_eq!(app.snap.emp_by_id(id).unwrap().calc.taken, 28.0);
    app.delete_leave(leave.id);
    assert!(app.snap.leaves.is_empty());

    // Dismiss: leaves the summary but stays in the directory.
    let mut e = app.snap.emp_by_id(id).unwrap().emp.clone();
    e.status = Status::Dismissed;
    app.save_employee(&e).unwrap();
    assert_eq!(app.snap.emp_by_id(id).unwrap().calc.state, State::Dismissed);
    assert!(app.snap.summary_order(None).is_empty());
    assert_eq!(app.snap.emps.len(), 1);

    // A department in use cannot be deleted; deleting the employee frees it.
    app.delete_department(dept);
    assert_eq!(app.snap.departments.len(), 1);
    assert!(app.delete_employee(id));
    app.delete_department(dept);
    assert!(app.snap.departments.is_empty());
}

#[test]
fn navigation_remembers_where_the_card_was_opened_from() {
    let mut app = test_app();
    let id = app.save_employee(&Employee { fio: "А".into(), ..Default::default() }).unwrap();
    app.go(Route::Summary);
    app.open_employee(id);
    assert_eq!(app.route, Route::Employee(Target::Id(id)));
    assert_eq!(app.came_from, Route::Summary);
    assert_eq!(app.f.employee.draft.fio, "А");
    app.new_employee();
    assert!(app.f.employee.is_new);
    assert_eq!(app.came_from, Route::Summary, "going card -> new card keeps the original origin");
}

#[test]
fn import_replaces_data_in_one_step() {
    let mut app = test_app();
    app.save_employee(&Employee { fio: "Старый".into(), ..Default::default() }).unwrap();
    let import = otpuska_core::db::Import {
        settings: Some(Settings { base_days: 30.0, ..Default::default() }),
        departments: vec!["Офис".into()],
        employees: vec![otpuska_core::db::ImportEmployee {
            employee: Employee { fio: "Новый".into(), hire_date: Some(d(2025, 1, 1)), ..Default::default() },
            department: Some("Офис".into()),
        }],
        leaves: vec![],
        warnings: vec![],
    };
    app.apply_import(import);
    assert_eq!(app.snap.emps.len(), 1);
    assert_eq!(app.snap.emps[0].emp.fio, "Новый");
    assert_eq!(app.snap.settings.base_days, 30.0);
    assert_eq!(app.route, Route::Summary);
}

// ---- "what is printed is what the screen shows" ---------------------------------------------

fn seeded_db() -> Db {
    let db = Db::open_memory().unwrap();
    let shop = db.add_department("Цех").unwrap();
    let office = db.add_department("Офис").unwrap();
    let people = [("Яковлев", shop), ("Андреев", shop), ("Борисов", office), ("Васильев", office), ("Григорьев", office)];
    let mut andreev = 0;
    for (fio, dept) in people {
        let e = Employee { fio: fio.into(), department_id: Some(dept), hire_date: Some(d(2024, 1, 10)), ..Default::default() };
        let id = db.insert_employee(&e).unwrap();
        if fio == "Андреев" {
            andreev = id;
        }
    }
    // Андреев took a leave last year; the others did not.
    let year = otpuska_core::date::today().year() - 1;
    db.insert_leave(&Leave { employee_id: andreev, start: Some(d(year, 6, 2)), base_days: 14.0, ..Default::default() }).unwrap();
    db
}

use chrono::Datelike;

/// A window with the app on `route`, built the way the real one is (fonts, theme, first frame).
fn screen(route: Route) -> Harness<'static, App> {
    let db = seeded_db();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1360.0, 860.0))
        .build_eframe(move |cc| App::new(&cc.egui_ctx, db, Config::default()).expect("app"));
    h.state_mut().go(route);
    h.run();
    h
}

fn dept_id(h: &Harness<'static, App>, name: &str) -> i64 {
    h.state().snap.departments.iter().find(|d| d.name == name).unwrap().id
}

fn print_now(h: &mut Harness<'static, App>) -> String {
    let before = h.state().printed.len();
    h.get_by_label("Печать").click();
    h.run();
    assert_eq!(h.state().printed.len(), before + 1, "the print button produced a report");
    h.state().printed.last().unwrap().1.clone()
}

#[test]
fn summary_prints_only_filtered_rows() {
    let mut h = screen(Route::Summary);
    let office = dept_id(&h, "Офис");

    // No filter: everybody, and the header says so.
    let html = print_now(&mut h);
    for n in ["Яковлев", "Андреев", "Борисов", "Васильев", "Григорьев"] {
        assert!(html.contains(n), "{n} is printed without filters");
    }
    assert!(html.contains("Без фильтров — все записи: 5"));

    // Department filter: only the office.
    h.state_mut().f.summary.filter.dept = Some(office);
    h.run();
    let html = print_now(&mut h);
    assert!(html.contains("Борисов") && html.contains("Васильев") && html.contains("Григорьев"));
    assert!(!html.contains("Яковлев") && !html.contains("Андреев"));
    assert!(html.contains("Подразделение: Офис — показано 3 из 5"));

    // Plus a search: a single row.
    h.state_mut().f.summary.filter.search = "борис".into();
    h.run();
    let html = print_now(&mut h);
    assert!(html.contains("Борисов") && !html.contains("Васильев"));
    assert!(html.contains("показано 1 из 5"));

    // "Did not take leave last year" over the whole company.
    let f = &mut h.state_mut().f.summary.filter;
    f.reset();
    f.took = otpuska_core::filter::Took::NotTaken;
    f.period.preset = otpuska_core::filter::PeriodPreset::LastYear;
    h.run();
    let html = print_now(&mut h);
    assert!(!html.contains("Андреев"), "Андреев did take leave last year");
    assert!(html.contains("Яковлев") && html.contains("Борисов"));
    assert!(html.contains("не был в отпуске"));
}

#[test]
fn employees_print_follows_filter_and_sort() {
    let mut h = screen(Route::Employees);
    let shop = dept_id(&h, "Цех");
    // Sorted by name, descending, only the shop.
    h.state_mut().f.employees.filter.dept = Some(shop);
    h.state_mut().f.employees.sort = crate::views::Sort::new(crate::views::employees::Col::Fio, false);
    h.run();
    let html = print_now(&mut h);
    assert!(html.contains("Яковлев") && html.contains("Андреев") && !html.contains("Борисов"));
    assert!(html.find("Яковлев").unwrap() < html.find("Андреев").unwrap(), "printed in the screen's order (Я before А)");
}

#[test]
fn periods_journal_and_reports_print_the_screen() {
    let mut h = screen(Route::Periods);
    let office = dept_id(&h, "Офис");

    // Periods: only those who still have days to take, in the office.
    h.state_mut().f.periods.filter.dept = Some(office);
    h.state_mut().f.periods.rest = otpuska_core::filter::RestFilter::HasLeft;
    h.run();
    let html = print_now(&mut h);
    assert!(html.contains("Борисов") && !html.contains("Яковлев"));
    assert!(html.contains("Отпуск: не отгулял"));

    // Journal: the only leave belongs to the shop, so filtering by the office prints nothing.
    h.state_mut().go(Route::Journal);
    h.run();
    let html = print_now(&mut h);
    assert!(html.contains("Андреев") && html.contains("Без фильтров"));
    h.state_mut().f.journal.filter.dept = Some(office);
    h.run();
    let html = print_now(&mut h);
    assert!(!html.contains("Андреев") && html.contains("ничего не найдено"));

    // Reports: with a period that contains the leave it shows up; the department table is limited.
    h.state_mut().go(Route::Reports);
    h.state_mut().f.reports.filter.show = otpuska_core::filter::LeaveShow::All;
    h.run();
    let html = print_now(&mut h);
    assert!(html.contains("Андреев"));
    h.state_mut().f.reports.filter.dept = Some(office);
    h.run();
    let html = print_now(&mut h);
    assert!(!html.contains("Андреев") && html.contains("Офис"));
}
