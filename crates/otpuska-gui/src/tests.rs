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

fn picker_snapshot() -> otpuska_core::Snapshot {
    let emp = |id, fio: &str| Employee { id, fio: fio.into(), hire_date: Some(d(2024, 1, 10)), ..Default::default() };
    let data = Data { employees: vec![emp(1, "Иванов Иван"), emp(2, "Петров Пётр"), emp(3, "Сидоров Семён")], ..Default::default() };
    otpuska_core::Snapshot::build(&data, d(2026, 10, 23))
}

/// The search box of the employee picker (journal, schedule) must keep what is typed into it.
#[test]
fn employee_picker_search_keeps_the_typed_text_and_narrows_the_list() {
    let mut h = harness((picker_snapshot(), None::<i64>), |ui, (snap, value)| {
        ui::emp_picker(ui, "picker", snap, value, "Выберите сотрудника…", 300.0, true);
    });
    h.get_by_label_contains("Выбор сотрудника").click();
    h.run();
    let input = h.get_by_role(egui::accesskit::Role::TextInput);
    input.focus();
    input.type_text("сид");
    h.run();
    assert_eq!(h.get_by_role(egui::accesskit::Role::TextInput).value().as_deref(), Some("сид"), "typed text stays in the box");

    // Enter chooses the first match.
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(h.state().1, Some(3));
}

#[test]
fn employee_picker_search_starts_empty_every_time() {
    let mut h = harness((picker_snapshot(), None::<i64>), |ui, (snap, value)| {
        ui::emp_picker(ui, "picker", snap, value, "Выберите сотрудника…", 300.0, true);
    });
    h.get_by_label_contains("Выбор сотрудника").click();
    h.run();
    let input = h.get_by_role(egui::accesskit::Role::TextInput);
    input.focus();
    input.type_text("пет");
    h.run();
    // Close without choosing (click the button again), open again: the old query is gone.
    h.get_by_label_contains("Выбор сотрудника").click();
    h.run();
    h.get_by_label_contains("Выбор сотрудника").click();
    h.run();
    assert_eq!(h.get_by_role(egui::accesskit::Role::TextInput).value().as_deref().unwrap_or(""), "");
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

// ---- several databases ---------------------------------------------------------------------------

mod databases {
    use super::*;
    use crate::launcher::Root;
    use egui::accesskit::Role;
    use std::path::Path;

    fn today() -> Date {
        otpuska_core::date::today()
    }

    /// Two organizations, one employee in the first.
    fn two_orgs(home: &Path) -> (Config, String, String) {
        let mut c = Config { home: Some(home.to_path_buf()), ..Default::default() };
        let a = c.create_database("Школа №5").unwrap();
        let b = c.create_database("Детский сад").unwrap();
        let db = c.open_database(&a, today()).unwrap();
        db.insert_employee(&Employee { fio: "Иванов".into(), ..Default::default() }).unwrap();
        (c, a, b)
    }

    fn window(config: Config) -> Harness<'static, Root> {
        let mut h = Harness::builder().with_size(egui::vec2(1360.0, 860.0)).build_eframe(move |cc| Root::new(&cc.egui_ctx, config));
        h.run();
        h
    }

    fn employees(h: &Harness<'static, Root>) -> usize {
        h.state().app().expect("a database is open").snap.emps.len()
    }

    #[test]
    fn start_asks_which_database_and_the_sidebar_switches() {
        let home = tempfile::tempdir().unwrap();
        let (c, _, _) = two_orgs(home.path());
        let mut h = window(c);
        assert!(h.state().app().is_none(), "two databases: the chooser comes first");

        h.get_by_label("Открыть базу Школа №5").click();
        h.run();
        assert_eq!(h.state().app().unwrap().db_name().as_deref(), Some("Школа №5"));
        assert_eq!(employees(&h), 1);

        // The plate with the name opens a list of the databases; choosing one switches right there.
        h.get_by_label_contains("База: Школа №5").click();
        h.run();
        assert!(h.state().app().is_some(), "the list opens over the work screen, nothing is closed yet");
        h.get_by_label("Переключиться на базу Детский сад").click();
        h.run();
        assert_eq!(h.state().app().unwrap().db_name().as_deref(), Some("Детский сад"));
        assert_eq!(employees(&h), 0, "nothing from the first database is visible here");

        // The choice is remembered for the next start.
        assert_eq!(h.state().app().unwrap().config.last_db, h.state().app().unwrap().current_db);
    }

    /// Opens the program on the first of two organizations, asking nothing.
    fn working_in_first(home: &Path) -> (Harness<'static, Root>, String, String) {
        let (mut c, a, b) = two_orgs(home);
        c.ask_on_start = Some(false);
        c.last_db = Some(a.clone());
        let h = window(c);
        assert_eq!(h.state().app().unwrap().db_name().as_deref(), Some("Школа №5"));
        (h, a, b)
    }

    #[test]
    fn the_sidebar_list_switches_between_databases_inside_the_app() {
        let home = tempfile::tempdir().unwrap();
        let (mut h, _, _) = working_in_first(home.path());
        assert_eq!(employees(&h), 1);

        // One click on the plate shows every organization; the open one is marked and does nothing.
        h.get_by_label_contains("База: Школа №5").click();
        h.run();
        h.get_by_label("Переключиться на базу Школа №5"); // the open one is in the list too
        h.get_by_label("Переключиться на базу Детский сад").click();
        h.run();
        let app = h.state().app().expect("switched without going through the chooser");
        assert_eq!((app.db_name().as_deref(), app.snap.emps.len()), (Some("Детский сад"), 0));

        // And back again; the choice is remembered for the next start.
        h.get_by_label_contains("База: Детский сад").click();
        h.run();
        h.get_by_label("Переключиться на базу Школа №5").click();
        h.run();
        assert_eq!(employees(&h), 1);
        assert_eq!(h.state().app().unwrap().config.last_db, h.state().app().unwrap().current_db);
    }

    #[test]
    fn the_sidebar_list_also_leads_to_new_organizations_and_to_the_chooser() {
        let home = tempfile::tempdir().unwrap();
        let (mut h, _, _) = working_in_first(home.path());
        h.get_by_label_contains("База: Школа №5").click();
        h.run();
        h.get_by_label("Новая организация…").click();
        h.run();
        let app = h.state().app().expect("still working");
        assert_eq!((app.route.clone(), app.f.settings.tab), (Route::Settings, crate::views::settings::Tab::Bases));

        h.get_by_label_contains("База: Школа №5").click();
        h.run();
        h.get_by_label("Окно выбора базы").click();
        h.run();
        assert!(h.state().app().is_none(), "the chooser window");
    }

    #[test]
    fn switching_is_refused_while_something_typed_in_would_be_lost() {
        let home = tempfile::tempdir().unwrap();
        let (mut h, _, other) = working_in_first(home.path());
        let app = h.state_mut().app_mut().unwrap();
        app.new_employee();
        app.f.employee.draft.fio = "Ещё не сохранён".into();
        h.run();
        h.get_by_label_contains("База: Школа №5").click();
        h.run();
        h.get_by_label("Переключиться на базу Детский сад").click();
        h.run_steps(6); // the toast keeps asking for repaints
        let app = h.state().app().expect("nothing was closed");
        assert_eq!(app.db_name().as_deref(), Some("Школа №5"));
        assert!(app.toasts.iter().any(|t| !t.ok && t.text.contains("несохранённые изменения")));
        // Once the form is clean the switch works.
        let app = h.state_mut().app_mut().unwrap();
        app.f.employee.draft = app.f.employee.orig.clone();
        app.switch_database(&other);
        h.run();
        assert_eq!(h.state().app().unwrap().db_name().as_deref(), Some("Детский сад"));
    }

    #[test]
    fn a_new_database_is_created_from_the_chooser() {
        let home = tempfile::tempdir().unwrap();
        let (c, _, _) = two_orgs(home.path());
        let mut h = window(c);
        let input = h.get_by_role(Role::TextInput);
        input.focus();
        input.type_text("Лицей");
        h.run();
        h.get_by_label("Создать и открыть").click();
        h.run();
        let app = h.state().app().expect("the new database opens at once");
        assert_eq!(app.db_name().as_deref(), Some("Лицей"));
        assert_eq!(app.snap.emps.len(), 0);
        assert_eq!(app.config.databases.len(), 3);
        // Saved: the next start sees it.
        let saved: Config = serde_json::from_str(&std::fs::read_to_string(home.path().join("config.json")).unwrap()).unwrap();
        assert_eq!(saved.databases.len(), 3);
    }

    #[test]
    fn upgrading_from_the_single_database_version_keeps_everything_in_place() {
        let home = tempfile::tempdir().unwrap();
        {
            let db = Db::open(&home.path().join("otpuska.db")).unwrap();
            let id = db.insert_employee(&Employee { fio: "Старый сотрудник".into(), hire_date: Some(d(2023, 5, 1)), ..Default::default() }).unwrap();
            db.insert_leave(&Leave { employee_id: id, start: Some(d(2024, 6, 3)), base_days: 14.0, ..Default::default() }).unwrap();
            db.insert_employee(&Employee { fio: "Второй".into(), ..Default::default() }).unwrap();
        }
        // The configuration the old version wrote: no list of databases.
        let mut c: Config = serde_json::from_str(r#"{"ui":{"theme":"dark","accent":"blue"}}"#).unwrap();
        c.home = Some(home.path().to_path_buf());
        assert!(c.migrate());
        c.save().unwrap();

        let h = window(c);
        let app = h.state().app().expect("a single database opens without asking");
        assert_eq!(app.db.path(), home.path().join("otpuska.db"), "the same file, not a copy");
        assert_eq!(app.db_name().as_deref(), Some("Основная база"));
        assert_eq!((app.snap.emps.len(), app.snap.leaves.len()), (2, 1));
        assert_eq!(app.config.ui.accent.as_deref(), Some("blue"), "look and feel survive");

        // A second start from the saved configuration does the same.
        let mut again: Config = serde_json::from_str(&std::fs::read_to_string(home.path().join("config.json")).unwrap()).unwrap();
        again.home = Some(home.path().to_path_buf());
        assert!(!again.migrate());
        drop(h);
        let h = window(again);
        assert_eq!(employees(&h), 2);
    }

    #[test]
    fn a_missing_file_shows_a_problem_instead_of_an_empty_database() {
        let home = tempfile::tempdir().unwrap();
        let (c, a, _) = two_orgs(home.path());
        std::fs::remove_dir_all(home.path().join("databases").join(&a)).unwrap();
        let mut h = window(c);
        assert!(h.state().app().is_none());
        h.get_by_label_contains("Файл не найден");
        h.get_by_label("Убрать из списка").click();
        h.run();
        h.get_by_label("Открыть базу Детский сад").click();
        h.run();
        assert_eq!(h.state().app().unwrap().config.databases.len(), 1);
    }

    #[test]
    fn rename_create_and_delete_from_the_app() {
        let home = tempfile::tempdir().unwrap();
        let (mut c, a, b) = two_orgs(home.path());
        c.ask_on_start = Some(false);
        c.last_db = Some(a.clone());
        let mut h = window(c);
        assert_eq!(employees(&h), 1, "asking is off: the last database opens at once");

        let app = h.state_mut().app_mut().unwrap();
        assert!(app.rename_database(&a, "Школа №7"));
        assert_eq!(app.db_name().as_deref(), Some("Школа №7"));
        assert!(!app.rename_database(&b, "школа №7"), "the name is taken");
        assert!(!app.rename_database(&b, "  "), "empty name");

        assert!(!app.delete_database(&a), "the open database cannot be deleted");
        assert!(app.create_database("Лицей"));
        assert_eq!(app.config.databases.len(), 3);
        assert!(app.delete_database(&b));
        assert_eq!(app.config.databases.len(), 2);
        assert!(home.path().join("deleted").is_dir(), "files wait in deleted/ for a month");

        // Everything is on disk: the names, and the order of the list.
        let saved: Config = serde_json::from_str(&std::fs::read_to_string(home.path().join("config.json")).unwrap()).unwrap();
        let names: Vec<_> = saved.databases.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["Школа №7", "Лицей"]);

        // The settings tab draws with all of this without trouble.
        let app = h.state_mut().app_mut().unwrap();
        app.f.settings.tab = crate::views::settings::Tab::Bases;
        app.go(Route::Settings);
        h.run_steps(4); // the toasts keep asking for repaints
        assert_eq!(h.get_all_by_label("Показать файл в папке").count(), 2, "one row per database");
    }

    #[test]
    fn two_databases_do_not_share_data_even_with_the_same_employee_names() {
        let home = tempfile::tempdir().unwrap();
        let (mut c, a, b) = two_orgs(home.path());
        c.ask_on_start = Some(false);
        c.last_db = Some(b.clone());
        let mut h = window(c);
        let app = h.state_mut().app_mut().unwrap();
        let id = app.save_employee(&Employee { fio: "Иванов".into(), hire_date: Some(d(2024, 1, 10)), ..Default::default() }).unwrap();
        assert!(app.save_leave(&Leave { employee_id: id, start: Some(d(2025, 6, 2)), base_days: 14.0, ..Default::default() }));
        app.switch_database(&a);
        h.run();
        let first = h.state().app().unwrap();
        assert_eq!(first.snap.emps.len(), 1);
        assert!(first.snap.leaves.is_empty(), "the leave belongs to the other organization");
    }
}

// ---- unpaid, maternity and childcare leave; the schedule; reminders -----------------------------------

mod new_leave {
    use super::*;
    use crate::views::{LeaveDraft, PlanDraft};
    use chrono::Duration;
    use otpuska_core::model::LeaveType;
    use otpuska_core::schedule::{Band, PlanStatus, ReminderKind};

    fn today() -> Date {
        otpuska_core::date::today()
    }

    fn id_of(h: &Harness<'static, App>, fio: &str) -> i64 {
        h.state().snap.emps.iter().find(|r| r.emp.fio == fio).unwrap().emp.id
    }

    #[test]
    fn the_journal_form_takes_unpaid_leave_and_shows_what_it_does_to_the_working_year() {
        let mut h = screen(Route::Journal);
        let id = id_of(&h, "Андреев"); // hired 10 Jan 2024, so the second working year began on 10 Jan 2025
        let taken_before = h.state().snap.emp_by_id(id).unwrap().calc.taken;

        h.state_mut().f.journal.draft = LeaveDraft {
            employee: Some(id),
            kind: LeaveType::Unpaid,
            start: Some(d(2025, 3, 3)),
            end: Some(d(2025, 3, 23)), // 21 days
            ..Default::default()
        };
        h.run();
        // The form tells before saving: 14 days count, the other 7 do not.
        h.get_by_label_contains("остальные 7 дней");
        h.get_by_label("Добавить").click();
        h.run_steps(6); // the toast keeps asking for repaints

        let app = h.state();
        let leave = app.snap.leaves.iter().find(|l| l.leave.kind == LeaveType::Unpaid).expect("saved");
        assert_eq!((leave.leave.start, leave.end, leave.leave.length()), (Some(d(2025, 3, 3)), Some(d(2025, 3, 23)), 21.0));
        let calc = &app.snap.emp_by_id(id).unwrap().calc;
        assert_eq!(calc.taken, taken_before, "the balance is not touched");
        assert_eq!((calc.work_years[1].start, calc.work_years[1].shift, calc.work_years[1].end), (d(2025, 1, 10), 7, d(2026, 1, 16)));
        assert_eq!(calc.work_years[2].start, d(2026, 1, 17), "the next year starts the day after");
        // The kind stays chosen for the next entry; the dates are cleared.
        let draft = &app.f.journal.draft;
        assert_eq!((draft.kind, draft.employee, draft.start, draft.end), (LeaveType::Unpaid, Some(id), None, None));
    }

    #[test]
    fn a_leave_without_dates_cannot_be_saved() {
        let mut h = screen(Route::Journal);
        let id = id_of(&h, "Андреев");
        let before = h.state().snap.leaves.len();
        h.state_mut().f.journal.draft = LeaveDraft { employee: Some(id), kind: LeaveType::Childcare, start: Some(d(2025, 3, 3)), ..Default::default() };
        h.run();
        // The button is off (the draft is not valid) and nothing was added.
        assert!(h.state().f.journal.draft.problem().is_some());
        h.get_by_label("Добавить").click();
        h.run();
        assert_eq!(h.state().snap.leaves.len(), before);
        // The same through the application: the database refuses it.
        let id = h.state().snap.emps[0].emp.id;
        assert!(!h.state_mut().save_leave(&Leave { employee_id: id, kind: LeaveType::Maternity, start: Some(d(2025, 3, 3)), ..Default::default() }));
        assert_eq!(h.state().snap.leaves.len(), before);
    }

    #[test]
    fn childcare_end_date_is_filled_from_the_birth_date() {
        let mut h = screen(Route::Journal);
        let id = id_of(&h, "Борисов");
        h.state_mut().f.journal.draft = LeaveDraft { employee: Some(id), kind: LeaveType::Childcare, start: Some(d(2024, 6, 1)), child_birth: Some(d(2024, 2, 10)), ..Default::default() };
        h.run();
        h.get_by_label("3 лет").click();
        h.run();
        assert_eq!(h.state().f.journal.draft.end, Some(d(2027, 2, 9)), "until the third birthday");
        h.get_by_label("1,5 лет").click();
        h.run();
        assert_eq!(h.state().f.journal.draft.end, Some(d(2025, 8, 9)));
        // Maternity: the usual lengths from the start date.
        h.state_mut().f.journal.draft = LeaveDraft { employee: Some(id), kind: LeaveType::Maternity, start: Some(d(2024, 1, 15)), ..Default::default() };
        h.run();
        h.get_by_label("140 дн.").click();
        h.run();
        assert_eq!(h.state().f.journal.draft.end, Some(d(2024, 6, 2)), "the 140th day counting the first");
        h.get_by_label("194 дн.").click();
        h.run();
        assert_eq!(h.state().f.journal.draft.end, Some(d(2024, 7, 26)));
    }

    #[test]
    fn journal_and_reports_print_the_kind_of_leave() {
        let mut h = screen(Route::Journal);
        let id = id_of(&h, "Борисов");
        assert!(h.state_mut().save_leave(&Leave { employee_id: id, kind: LeaveType::Childcare, start: Some(d(2024, 6, 1)), end: Some(d(2024, 6, 30)), ..Default::default() }));
        h.run();
        let html = print_now(&mut h);
        assert!(html.contains("<th>Вид</th>") && html.contains("Уход за ребёнком") && html.contains("Ежегодный"));
        // Filtering by the kind prints only those leaves.
        h.state_mut().f.journal.filter.kind = otpuska_core::filter::LeaveKind::Childcare;
        h.run();
        let html = print_now(&mut h);
        assert!(html.contains("Борисов") && !html.contains("Андреев"));
        assert!(html.contains("Вид отпуска: по уходу за ребёнком"));
    }

    #[test]
    fn schedule_entry_is_tracked_against_the_journal_and_reminds() {
        let mut h = screen(Route::Schedule);
        let id = id_of(&h, "Васильев");
        let start = today() + Duration::days(10);
        h.state_mut().f.schedule.draft = PlanDraft { employee: Some(id), start: Some(start), base: 14.0, ..Default::default() };
        h.run();
        h.get_by_label("В график").click();
        h.run_steps(6);
        assert_eq!(h.state().snap.plans.len(), 1);
        assert_eq!(h.state().snap.plans[0].status, PlanStatus::Ahead);
        assert_eq!(h.state().snap.plans[0].end, Some(start + Duration::days(13)));
        // It reminds: ten days to the start, nothing in the journal yet.
        let r = &h.state().snap.reminders[0];
        assert_eq!((r.kind, r.band, r.days), (ReminderKind::PlanStart, Band::Near, 10));
        assert_eq!(r.notify_by(), Some(start - Duration::days(14)));

        // The reminders screen: the card, and a button that puts the leave into the journal.
        h.state_mut().go(Route::Reminders);
        h.run_steps(6);
        h.get_by_label_contains("Начало отпуска по графику");
        h.get_by_label("Внести в журнал").click();
        h.run_steps(6);
        let app = h.state();
        assert_eq!(app.snap.leaves.iter().filter(|l| l.leave.employee_id == id).count(), 1);
        assert_eq!(app.snap.plans[0].status, PlanStatus::Booked);
        assert_eq!(app.snap.reminders[0].kind, ReminderKind::Start, "now the journal record reminds, and knows its entry");
        assert!(app.snap.reminders[0].plan_ix.is_some());
    }

    #[test]
    fn schedule_prints_the_year_and_filters_of_the_screen() {
        let mut h = screen(Route::Schedule);
        let (a, b) = (id_of(&h, "Борисов"), id_of(&h, "Григорьев"));
        let year = today().year() + 1;
        for (emp, month) in [(a, 2), (b, 7)] {
            assert!(h.state_mut().save_plan(&otpuska_core::model::Plan { employee_id: emp, start: Some(d(year, month, 3)), base_days: 14.0, ..Default::default() }));
        }
        assert!(h.state_mut().save_plan(&otpuska_core::model::Plan { employee_id: a, start: Some(d(year + 1, 1, 12)), base_days: 7.0, ..Default::default() }));
        h.state_mut().f.schedule.filter.year = year;
        h.run();
        let html = print_now(&mut h);
        assert!(html.contains(&format!("График отпусков на {year} год")) && html.contains("Борисов") && html.contains("Григорьев"));
        assert!(!html.contains("12.01.") , "the entry of the next year is not printed");
        assert!(html.contains("показано 2 из 2") || html.contains("Без фильтров — все записи: 2"));
        h.state_mut().f.schedule.filter.search = "григ".into();
        h.run();
        let html = print_now(&mut h);
        assert!(html.contains("Григорьев") && !html.contains("Борисов"));
        assert!(html.contains("Поиск: «григ»") && html.contains("показано 1 из 2"));
    }

    #[test]
    fn the_summary_and_the_sidebar_announce_reminders_and_childcare_end() {
        let mut h = screen(Route::Summary);
        h.run();
        assert!(h.query_by_label_contains("Напоминания об отпусках").is_none(), "nothing to announce yet");
        let id = id_of(&h, "Яковлев");
        let end = today() + Duration::days(9);
        assert!(h.state_mut().save_leave(&Leave { employee_id: id, kind: LeaveType::Childcare, start: Some(end - Duration::days(500)), end: Some(end), ..Default::default() }));
        h.run();
        h.get_by_label_contains("Напоминания об отпусках");
        let r = &h.state().snap.reminders[0];
        assert_eq!((r.kind, r.days), (ReminderKind::End(LeaveType::Childcare), 9));
        assert_eq!(r.back_to_work(), Some(end + Duration::days(1)));
        h.get_by_label("Открыть напоминания").click();
        h.run();
        assert_eq!(h.state().route, Route::Reminders);
        h.get_by_label_contains("Окончание отпуска по уходу за ребёнком");
        // The switch on top narrows the list; the print follows it.
        h.state_mut().f.reminders.filter.show = otpuska_core::filter::ReminderShow::Missed;
        h.run();
        let html = print_now(&mut h);
        assert!(html.contains("ничего не найдено"));
        h.state_mut().f.reminders.filter.show = otpuska_core::filter::ReminderShow::End;
        h.run();
        let html = print_now(&mut h);
        assert!(html.contains("Яковлев") && html.contains("Окончание отпуска по уходу за ребёнком"));
    }
}

// ---- an installation of the previous version is upgraded in place -------------------------------------

mod upgrade {
    use super::*;
    use crate::launcher::Root;

    /// What the user has today: version 1.0's `otpuska.db` (first schema) and a `config.json` with no list of databases.
    #[test]
    fn a_version_1_installation_opens_with_all_its_data_and_a_copy_is_kept() {
        let home = tempfile::tempdir().unwrap();
        let file = home.path().join("otpuska.db");
        otpuska_core::db::v1::file(&file, true);
        let mut config: Config = serde_json::from_str(r#"{"ui":{"theme":"dark","accent":"blue"},"last_dir":"/tmp"}"#).unwrap();
        config.home = Some(home.path().to_path_buf());
        assert!(config.migrate());
        config.save().unwrap();

        let mut h = Harness::builder().with_size(egui::vec2(1360.0, 860.0)).build_eframe(move |cc| Root::new(&cc.egui_ctx, config));
        h.run();
        let app = h.state().app().expect("one database: it opens at once");
        assert_eq!(app.db.path(), file, "the same file, not a copy");
        assert_eq!((app.snap.emps.len(), app.snap.leaves.len(), app.snap.departments.len()), (3, 3, 2));
        assert!(app.snap.leaves.iter().all(|l| l.leave.kind == otpuska_core::model::LeaveType::Paid));
        assert_eq!(app.config.ui.accent.as_deref(), Some("blue"));
        assert_eq!(app.snap.settings.base_days, 31.0, "the settings of the old file are kept");

        // The user is told, and the copy exists in the old format.
        h.get_by_label_contains("База обновлена до новой версии");
        let backups: Vec<_> = std::fs::read_dir(home.path().join("backups")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert!(backups.iter().any(|n| n.starts_with("otpuska-before-update-")), "{backups:?}");
        assert!(backups.iter().any(|n| n.starts_with("otpuska-before-v2-")), "{backups:?}");
        for name in backups.iter().filter(|n| n.starts_with("otpuska-before-")) {
            assert_eq!(otpuska_core::Db::peek(&home.path().join("backups").join(name)).unwrap().employees, 3);
        }
        h.get_by_label("Понятно").click();
        h.run();
        assert!(h.state().app().unwrap().upgrade_notice.is_none());
        assert!(h.query_by_label_contains("База обновлена до новой версии").is_none());
    }
}

// ---- the window on small screens ------------------------------------------------------------------------------

mod window {
    use crate::launcher::{fit_to_screen, WINDOW_MIN, WINDOW_SIZE};
    use egui::vec2;

    #[test]
    fn the_window_is_left_alone_on_a_big_screen_and_cut_down_on_a_small_one() {
        // A 14" MacBook (1512x982 points) and a full-HD screen at 100%: nothing to do.
        for monitor in [vec2(1512.0, 982.0), vec2(1920.0, 1080.0)] {
            assert_eq!(fit_to_screen(monitor, WINDOW_SIZE), (None, WINDOW_MIN));
        }
        // A 1366x768 laptop: the window gets smaller than the screen, not larger.
        let (size, min) = fit_to_screen(vec2(1366.0, 768.0), WINDOW_SIZE);
        let size = size.expect("cut down");
        assert!(size.x <= 1366.0 && size.y <= 768.0 && size.x > 1250.0 && size.y > 600.0, "{size:?}");
        assert_eq!(min, WINDOW_MIN);
        // 1920x1080 at 150% is 1280x720 points: the width is enough, the height is cut down.
        let (size, min) = fit_to_screen(vec2(1280.0, 720.0), WINDOW_SIZE);
        assert!(size.is_some() && min.x == WINDOW_MIN.x && min.y <= 720.0 && min.y < WINDOW_MIN.y);
        // 1366x768 at 125% is 1093x614 points: the smallest size follows the screen on both sides.
        let (size, min) = fit_to_screen(vec2(1093.0, 614.0), WINDOW_SIZE);
        assert!(size.is_some() && min.x < WINDOW_MIN.x && min.x <= 1093.0 && min.y <= 614.0);
        // Only ever smaller: a window that is already small enough stays as it is.
        assert_eq!(fit_to_screen(vec2(1366.0, 768.0), vec2(1100.0, 600.0)).0, None);
    }
}
