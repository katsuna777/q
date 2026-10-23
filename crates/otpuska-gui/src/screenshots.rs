//! Renders every screen to PNG for visual review:
//! `OTPUSKA_DEMO_XLSX=book.xlsx SHOT_DIR=/tmp/shots cargo test -p otpuska-gui -- --ignored screenshots`

use crate::app::{App, Route};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use otpuska_core::config::Config;
use otpuska_core::{xlsx_import, Db};
use std::time::Duration;

fn settle<S>(h: &mut Harness<'_, S>, frames: usize) {
    for _ in 0..frames {
        h.step();
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn save<S>(h: &mut Harness<'_, S>, dir: &str, name: &str) {
    let img = h.render().expect("render");
    img.save(format!("{dir}/{name}.png")).expect("save png");
}

fn app_with(db: Db) -> impl FnOnce(&mut eframe::CreationContext<'_>) -> App {
    move |cc| App::new(&cc.egui_ctx, db, Config::default()).expect("app")
}

#[test]
#[ignore]
fn screenshots() {
    let dir = std::env::var("SHOT_DIR").expect("SHOT_DIR");
    let home = tempfile::tempdir().expect("tempdir");
    std::env::set_var("OTPUSKA_HOME", home.path());
    std::fs::create_dir_all(&dir).expect("shot dir");

    // Empty database first.
    let mut h = Harness::builder()
        .with_size(egui::vec2(1360.0, 860.0))
        .with_pixels_per_point(1.5)
        .wgpu()
        .build_eframe(app_with(Db::open_memory().expect("db")));
    h.state_mut().config.ui.theme = Some("dark".into());
    settle(&mut h, 8);
    save(&mut h, &dir, "00-empty");
    drop(h);

    // Demo data.
    let mut db = Db::open_memory().expect("db");
    let xlsx = std::env::var("OTPUSKA_DEMO_XLSX").expect("OTPUSKA_DEMO_XLSX");
    db.replace_all(&xlsx_import::read_workbook(xlsx.as_ref()).expect("import")).expect("replace");
    let mut h = Harness::builder()
        .with_size(egui::vec2(1360.0, 860.0))
        .with_pixels_per_point(1.5)
        .wgpu()
        .build_eframe(app_with(db));
    h.state_mut().config.ui.theme = Some("dark".into());
    settle(&mut h, 10);
    save(&mut h, &dir, "01-summary");

    // Filters: state popup, then "took no leave this year" in one department.
    h.get_by_label("Все состояния").click();
    settle(&mut h, 6);
    save(&mut h, &dir, "01a-summary-state-popup");
    h.key_press(egui::Key::Escape);
    settle(&mut h, 4);
    {
        let dept = h.state().snap.departments.iter().find(|d| d.name.starts_with("Цех №2")).map(|d| d.id);
        let f = &mut h.state_mut().f.summary.filter;
        f.dept = dept;
        f.took = otpuska_core::filter::Took::NotTaken;
        f.period.preset = otpuska_core::filter::PeriodPreset::ThisYear;
        f.states = vec![otpuska_core::State::Overdue, otpuska_core::State::Burning];
    }
    settle(&mut h, 8);
    save(&mut h, &dir, "01b-summary-filtered");
    h.state_mut().f.summary.filter.reset();

    h.state_mut().go(Route::Employees);
    settle(&mut h, 10);
    save(&mut h, &dir, "02-employees");

    h.state_mut().open_employee(17); // Крылов: доп. отпуск, просрочено
    settle(&mut h, 10);
    save(&mut h, &dir, "03-employee-card");

    h.state_mut().new_employee();
    settle(&mut h, 8);
    save(&mut h, &dir, "04-employee-new");

    h.state_mut().go(Route::Periods);
    settle(&mut h, 10);
    save(&mut h, &dir, "05-periods");

    h.state_mut().f.periods.rest = otpuska_core::filter::RestFilter::HasLeft;
    h.state_mut().f.periods.filter.search = "а".into();
    settle(&mut h, 8);
    save(&mut h, &dir, "05a-periods-filtered");
    h.state_mut().f.periods.rest = otpuska_core::filter::RestFilter::Any;
    h.state_mut().f.periods.filter.search.clear();

    h.state_mut().go(Route::Journal);
    settle(&mut h, 10);
    save(&mut h, &dir, "06-journal");

    // Popups: the calendar and the employee picker of the journal form.
    h.get_by_label("Календарь").click();
    settle(&mut h, 6);
    save(&mut h, &dir, "06a-journal-calendar");
    h.key_press(egui::Key::Escape);
    settle(&mut h, 4);
    h.get_by_label_contains("Выбор сотрудника").click();
    settle(&mut h, 6);
    save(&mut h, &dir, "06b-journal-picker");
    h.key_press(egui::Key::Escape);
    settle(&mut h, 4);

    h.state_mut().go(Route::Reports);
    settle(&mut h, 10);
    save(&mut h, &dir, "07-reports");

    use crate::views::settings::Tab;
    h.state_mut().go(Route::Settings);
    for (tab, name) in [(Tab::Calc, "08-settings-calc"), (Tab::Depts, "08a-settings-depts"), (Tab::Data, "08b-settings-data"), (Tab::Look, "08c-settings-look"), (Tab::Help, "08d-settings-help")] {
        h.state_mut().f.settings.tab = tab;
        settle(&mut h, 8);
        save(&mut h, &dir, name);
    }
    h.state_mut().f.settings.tab = Tab::Calc;

    h.state_mut().config.ui.theme = Some("light".into());
    h.state_mut().config.ui.accent = Some("blue".into());
    h.state_mut().go(Route::Summary);
    settle(&mut h, 10);
    save(&mut h, &dir, "09-summary-light");
    h.state_mut().go(Route::Periods);
    settle(&mut h, 10);
    save(&mut h, &dir, "10-periods-light");
}

/// The database chooser, the sidebar plate and the «Базы данных» tab:
/// `OTPUSKA_DEMO_XLSX=book.xlsx SHOT_DIR=/tmp/shots cargo test -p otpuska-gui -- --ignored screenshots_databases`
#[test]
#[ignore]
fn screenshots_databases() {
    use crate::launcher::Root;
    use crate::views::settings::Tab;
    let dir = std::env::var("SHOT_DIR").expect("SHOT_DIR");
    std::fs::create_dir_all(&dir).expect("shot dir");
    let home = tempfile::tempdir().expect("tempdir");
    let today = otpuska_core::date::today();
    let mut config = Config { home: Some(home.path().to_path_buf()), ..Default::default() };
    config.ui.theme = Some("dark".into());

    let window = |config: Config| {
        Harness::builder().with_size(egui::vec2(1360.0, 860.0)).with_pixels_per_point(1.5).wgpu().build_eframe(move |cc| Root::new(&cc.egui_ctx, config))
    };

    // A fresh install: nothing yet.
    let mut h = window(config.clone());
    settle(&mut h, 8);
    save(&mut h, &dir, "20-chooser-empty");
    drop(h);

    // Two organizations; the first has the demo data.
    let school = config.create_database("Школа №5").expect("create");
    config.create_database("Детский сад «Солнышко»").expect("create");
    config.create_database("ООО «Ромашка»").expect("create");
    let mut db = config.open_database(&school, today).expect("open");
    let xlsx = std::env::var("OTPUSKA_DEMO_XLSX").expect("OTPUSKA_DEMO_XLSX");
    db.replace_all(&xlsx_import::read_workbook(xlsx.as_ref()).expect("import")).expect("replace");
    drop(db);
    config.open_database(&school, today).expect("open again"); // last used

    let mut h = window(config.clone());
    settle(&mut h, 8);
    save(&mut h, &dir, "21-chooser");
    h.get_by_label("Открыть базу Школа №5").click();
    settle(&mut h, 10);
    save(&mut h, &dir, "22-work-with-database-plate");

    h.state_mut().app_mut().unwrap().go(Route::Settings);
    h.state_mut().app_mut().unwrap().f.settings.tab = Tab::Bases;
    settle(&mut h, 8);
    save(&mut h, &dir, "23-settings-databases");
    h.state_mut().app_mut().unwrap().f.settings.tab = Tab::Data;
    settle(&mut h, 8);
    save(&mut h, &dir, "24-settings-data");
    drop(h);

    config.ui.theme = Some("light".into());
    config.ui.accent = Some("blue".into());
    let mut h = window(config);
    settle(&mut h, 8);
    save(&mut h, &dir, "25-chooser-light");
}

/// Unpaid / maternity / childcare leave, the schedule and the reminders, on made-up data:
/// `SHOT_DIR=/tmp/shots cargo test -p otpuska-gui -- --ignored screenshots_new_leave`
#[test]
#[ignore]
fn screenshots_new_leave() {
    use crate::views::{LeaveDraft, PlanDraft};
    use chrono::Duration;
    use otpuska_core::model::{Employee, Leave, LeaveType, Plan};
    let dir = std::env::var("SHOT_DIR").expect("SHOT_DIR");
    std::fs::create_dir_all(&dir).expect("shot dir");
    let today = otpuska_core::date::today();
    let day = |n: i64| today + Duration::days(n);

    let db = Db::open_memory().expect("db");
    let shop = db.add_department("Цех №1").unwrap();
    let office = db.add_department("Бухгалтерия").unwrap();
    let hire = |y, m, d| chrono::NaiveDate::from_ymd_opt(y, m, d);
    let mut ids = Vec::new();
    for (fio, dept, pos, hired) in [
        ("Смирнова Анна Петровна", office, "Бухгалтер", hire(2019, 3, 11)),
        ("Кузнецов Игорь Олегович", shop, "Слесарь", hire(2021, 9, 6)),
        ("Орлова Мария Сергеевна", office, "Экономист", hire(2020, 1, 20)),
        ("Фёдоров Павел Андреевич", shop, "Мастер участка", hire(2022, 5, 16)),
        ("Васильева Елена Игоревна", office, "Кадровик", hire(2018, 11, 1)),
        ("Морозов Денис Викторович", shop, "Токарь", hire(2023, 2, 13)),
    ] {
        let e = Employee { fio: fio.into(), position: pos.into(), department_id: Some(dept), hire_date: hired, ..Default::default() };
        ids.push(db.insert_employee(&e).unwrap());
    }
    let leave = |emp, kind, start, end| Leave { employee_id: emp, kind, start: Some(start), end, ..Default::default() };
    // Unpaid leave of 25 days in the third working year of Смирнова (hired 11 Mar 2019).
    db.insert_leave(&leave(ids[0], LeaveType::Unpaid, hire(2021, 6, 1).unwrap(), Some(hire(2021, 6, 25).unwrap()))).unwrap();
    // Childcare leave of Орлова, ends in 11 days; maternity of Васильева ends in 40 days.
    db.insert_leave(&leave(ids[2], LeaveType::Childcare, day(-700), Some(day(11)))).unwrap();
    db.insert_leave(&leave(ids[4], LeaveType::Maternity, day(-100), Some(day(40)))).unwrap();
    // A booked leave in 12 days (Кузнецов) and one in 25 days (Фёдоров), both as in the schedule.
    for (emp, start) in [(ids[1], day(12)), (ids[3], day(25))] {
        db.insert_leave(&Leave { employee_id: emp, start: Some(start), base_days: 14.0, ..Default::default() }).unwrap();
        db.insert_plan(&Plan { employee_id: emp, start: Some(start), base_days: 14.0, ..Default::default() }).unwrap();
    }
    // Schedule entries without a journal record: one soon, one that was missed, one far away.
    db.insert_plan(&Plan { employee_id: ids[5], start: Some(day(9)), base_days: 14.0, note: "по заявлению".into(), ..Default::default() }).unwrap();
    db.insert_plan(&Plan { employee_id: ids[0], start: Some(day(-8)), base_days: 14.0, ..Default::default() }).unwrap();
    db.insert_plan(&Plan { employee_id: ids[0], start: Some(day(120)), base_days: 14.0, ..Default::default() }).unwrap();
    db.insert_leave(&Leave { employee_id: ids[3], start: Some(day(-50)), base_days: 14.0, ..Default::default() }).unwrap();
    db.insert_plan(&Plan { employee_id: ids[3], start: Some(day(-50)), base_days: 14.0, ..Default::default() }).unwrap();

    // SHOT_SIZE=1180x700 renders the narrowest window the program allows.
    let size = std::env::var("SHOT_SIZE").ok().and_then(|s| s.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))).unwrap_or((1360.0f32, 860.0f32));
    let mut h = Harness::builder().with_size(egui::vec2(size.0, size.1)).with_pixels_per_point(1.5).wgpu().build_eframe(app_with(db));
    h.state_mut().config.ui.theme = Some("dark".into());
    settle(&mut h, 10);
    save(&mut h, &dir, "30-summary-with-reminders");

    h.state_mut().go(Route::Journal);
    h.state_mut().f.journal.draft = LeaveDraft {
        employee: Some(ids[0]),
        kind: LeaveType::Unpaid,
        start: Some(hire(2025, 3, 3).unwrap()),
        end: Some(hire(2025, 3, 30).unwrap()),
        ..Default::default()
    };
    settle(&mut h, 10);
    save(&mut h, &dir, "31-journal-unpaid-form");

    h.state_mut().f.journal.draft =
        LeaveDraft { employee: Some(ids[2]), kind: LeaveType::Childcare, start: Some(day(-700)), child_birth: Some(day(-800)), ..Default::default() };
    settle(&mut h, 8);
    save(&mut h, &dir, "32-journal-childcare-form");
    h.state_mut().f.journal.draft = LeaveDraft::default();

    h.state_mut().open_employee(ids[0]);
    settle(&mut h, 10);
    save(&mut h, &dir, "33-employee-moved-year");

    h.state_mut().go(Route::Periods);
    h.state_mut().f.periods.first_year = 2019;
    settle(&mut h, 10);
    save(&mut h, &dir, "34-periods-moved-year");

    h.state_mut().go(Route::Schedule);
    h.state_mut().f.schedule.draft = PlanDraft { employee: Some(ids[1]), start: Some(day(200)), base: 14.0, ..Default::default() };
    h.state_mut().f.schedule.filter.year = 0;
    settle(&mut h, 10);
    save(&mut h, &dir, "35-schedule");

    h.state_mut().go(Route::Reminders);
    settle(&mut h, 10);
    save(&mut h, &dir, "36-reminders");
    h.state_mut().f.reminders.filter.show = otpuska_core::filter::ReminderShow::Near;
    settle(&mut h, 6);
    save(&mut h, &dir, "37-reminders-14-days");

    h.state_mut().go(Route::Settings);
    h.state_mut().f.settings.tab = crate::views::settings::Tab::Calc;
    settle(&mut h, 8);
    save(&mut h, &dir, "38-settings-reminders");

    h.state_mut().config.ui.theme = Some("light".into());
    h.state_mut().config.ui.accent = Some("blue".into());
    h.state_mut().go(Route::Reminders);
    h.state_mut().f.reminders.filter.show = otpuska_core::filter::ReminderShow::All;
    settle(&mut h, 10);
    save(&mut h, &dir, "39-reminders-light");
}

/// How long one frame of every screen takes to build (layout and painting commands, no GPU), on a
/// database of any size: `OTPUSKA_PERF_DB=copy.db cargo test --release -p otpuska-gui -- --ignored perf --nocapture`
#[test]
#[ignore]
fn perf_frames() {
    use crate::views::settings::Tab;
    use std::time::Instant;
    let file = std::env::var("OTPUSKA_PERF_DB").expect("OTPUSKA_PERF_DB (a copy of a database)");
    let db = Db::open(file.as_ref()).expect("db");
    let mut h = Harness::builder().with_size(egui::vec2(1360.0, 860.0)).build_eframe(app_with(db));
    h.state_mut().config.ui.theme = Some("dark".into());
    h.run_steps(4);
    let id = h.state().snap.emps.iter().find(|r| !r.calc.work_years.is_empty()).map(|r| r.emp.id).unwrap_or(1);
    println!("{} employees, {} records", h.state().snap.emps.len(), h.state().snap.leaves.len());
    let measure = |h: &mut Harness<'_, App>, name: &str| {
        h.run_steps(3);
        let frames = 60;
        let start = Instant::now();
        for _ in 0..frames {
            h.step();
        }
        println!("{name:<28} {:>7.2} ms/frame", start.elapsed().as_secs_f64() * 1000.0 / f64::from(frames));
    };
    for (route, name) in [
        (Route::Summary, "Сводка"),
        (Route::Employees, "Сотрудники"),
        (Route::Periods, "Периоды"),
        (Route::Journal, "Журнал"),
        (Route::Schedule, "График"),
        (Route::Reminders, "Напоминания"),
        (Route::Reports, "Отчёты"),
    ] {
        h.state_mut().go(route);
        measure(&mut h, name);
    }
    h.state_mut().open_employee(id);
    measure(&mut h, "Карточка сотрудника");
    h.state_mut().go(Route::Settings);
    for (tab, name) in [(Tab::Calc, "Настройки: расчёт"), (Tab::Help, "Настройки: справка")] {
        h.state_mut().f.settings.tab = tab;
        measure(&mut h, name);
    }
    // After an edit everything is recalculated and the snapshot replaced.
    let start = Instant::now();
    for _ in 0..50 {
        h.state_mut().reload();
    }
    println!("{:<28} {:>7.2} ms", "reload() (БД + пересчёт)", start.elapsed().as_secs_f64() * 1000.0 / 50.0);
}

/// The list of organizations that opens from the plate in the sidebar:
/// `SHOT_DIR=/tmp/shots cargo test -p otpuska-gui -- --ignored screenshots_switcher`
#[test]
#[ignore]
fn screenshots_switcher() {
    use crate::launcher::Root;
    use otpuska_core::model::Employee;
    let dir = std::env::var("SHOT_DIR").expect("SHOT_DIR");
    std::fs::create_dir_all(&dir).expect("shot dir");
    let home = tempfile::tempdir().expect("tempdir");
    let today = otpuska_core::date::today();
    let mut config = Config { home: Some(home.path().to_path_buf()), ask_on_start: Some(false), ..Default::default() };
    config.ui.theme = Some("dark".into());
    let school = config.create_database("Школа №5").expect("create");
    let kinder = config.create_database("Детский сад «Солнышко»").expect("create");
    config.create_database("ООО «Ромашка»").expect("create");
    for (id, n) in [(&school, 47), (&kinder, 18)] {
        let db = config.open_database(id, today).expect("open");
        for i in 0..n {
            db.insert_employee(&Employee { fio: format!("Сотрудник {i}"), ..Default::default() }).expect("employee");
        }
    }
    config.last_db = Some(school);
    let mut h = Harness::builder().with_size(egui::vec2(1360.0, 860.0)).with_pixels_per_point(1.5).wgpu().build_eframe(move |cc| Root::new(&cc.egui_ctx, config));
    settle(&mut h, 8);
    h.get_by_label_contains("База: Школа №5").click();
    settle(&mut h, 8);
    save(&mut h, &dir, "40-database-switcher");
}

/// A program at rest must not keep drawing, wherever the pointer lies: moves the pointer over
/// every screen on a grid and reports the places where the program keeps asking for new frames.
/// `OTPUSKA_PERF_DB=copy.db cargo test -p otpuska-gui -- --ignored idle_repaint_scan --nocapture`
#[test]
#[ignore]
fn idle_repaint_scan() {
    let file = std::env::var("OTPUSKA_PERF_DB").expect("OTPUSKA_PERF_DB (a copy of a database)");
    let db = Db::open(file.as_ref()).expect("db");
    let mut h = Harness::builder().with_size(egui::vec2(1360.0, 860.0)).build_eframe(app_with(db));
    h.state_mut().config.ui.theme = Some("dark".into());
    h.run_steps(4);
    let id = h.state().snap.emps.iter().find(|r| !r.calc.work_years.is_empty()).map(|r| r.emp.id).unwrap_or(1);
    let mut bad = 0;
    for (route, name) in [
        (Route::Summary, "Сводка"),
        (Route::Employees, "Сотрудники"),
        (Route::Periods, "Периоды"),
        (Route::Journal, "Журнал"),
        (Route::Schedule, "График"),
        (Route::Reminders, "Напоминания"),
        (Route::Reports, "Отчёты"),
        (Route::Settings, "Настройки"),
    ] {
        h.state_mut().go(route);
        h.run_steps(8);
        let mut spots = Vec::new();
        for gy in 0..22 {
            for gx in 0..34 {
                let pos = egui::pos2(10.0 + gx as f32 * 40.0, 10.0 + gy as f32 * 39.0);
                h.hover_at(pos);
                if h.run_ok().is_none() {
                    // Let it settle before the next point (a toast or an animation may be running).
                    h.run_steps(20);
                    spots.push((pos.x as i32, pos.y as i32));
                }
            }
        }
        println!("{name:<14} точек, где кадры не прекращаются: {}", spots.len());
        if !spots.is_empty() {
            println!("               {:?}", &spots[..spots.len().min(12)]);
            bad += spots.len();
        }
    }
    h.state_mut().open_employee(id);
    h.run_steps(8);
    let mut spots = 0;
    for gy in 0..22 {
        for gx in 0..34 {
            h.hover_at(egui::pos2(10.0 + gx as f32 * 40.0, 10.0 + gy as f32 * 39.0));
            if h.run_ok().is_none() {
                h.run_steps(20);
                spots += 1;
            }
        }
    }
    println!("Карточка       точек, где кадры не прекращаются: {spots}");
    assert_eq!(bad + spots, 0, "the program keeps drawing while nothing happens");
}
