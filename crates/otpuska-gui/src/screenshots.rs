//! Renders every screen to PNG for visual review:
//! `OTPUSKA_DEMO_XLSX=book.xlsx SHOT_DIR=/tmp/shots cargo test -p otpuska-gui -- --ignored screenshots`

use crate::app::{App, Route};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use otpuska_core::config::Config;
use otpuska_core::{xlsx_import, Db};
use std::time::Duration;

fn settle(h: &mut Harness<'_, App>, frames: usize) {
    for _ in 0..frames {
        h.step();
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn save(h: &mut Harness<'_, App>, dir: &str, name: &str) {
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
