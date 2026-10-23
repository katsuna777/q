//! «Настройки»: six short tabs instead of one long page — calculation rules, departments,
//! databases (one per organization), data (backups, Excel import), appearance and help.

use crate::app::{page, top_bar, App};
use crate::launcher::{describe, peek_entry};
use crate::theme::ACCENTS;
use crate::ui::{self, Kind, Tone};
use egui::{vec2, Align, Layout, Sense, Stroke, Ui};
use egui_phosphor::regular as icon;
use otpuska_core::config;
use otpuska_core::date;
use otpuska_core::engine::State as St;
use otpuska_core::db::Peek;
use otpuska_core::model::Settings;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Tab {
    #[default]
    Calc,
    Depts,
    Bases,
    Data,
    Look,
    Help,
}

pub struct State {
    pub tab: Tab,
    pub draft: Settings,
    /// Copy the stored settings into the draft on the next frame.
    pub sync: bool,
    pub new_dept: String,
    pub rename: HashMap<i64, String>,
    pub new_db: String,
    pub rename_db: HashMap<String, String>,
    /// What each database holds, read when the tab is opened (and after changes), not every frame.
    pub db_info: Option<HashMap<String, Result<Peek, String>>>,
}

impl Default for State {
    fn default() -> Self {
        State {
            tab: Tab::Calc,
            draft: Settings::default(),
            sync: true,
            new_dept: String::new(),
            rename: HashMap::new(),
            new_db: String::new(),
            rename_db: HashMap::new(),
            db_info: None,
        }
    }
}

/// One setting: title and a short hint on the left, the control on the right.
fn row(ui: &mut Ui, label: &str, hint: &str, add: impl FnOnce(&mut Ui)) {
    let p = ui::pal(ui);
    ui::split_row(
        ui,
        54.0,
        |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.add_space(if hint.is_empty() { 17.0 } else { 8.0 });
                ui.label(ui::strong(label, 13.5, p.text));
                if !hint.is_empty() {
                    ui.add(egui::Label::new(ui::text(hint, 11.5, p.text3)).truncate());
                }
            });
        },
        add,
    );
}

/// A whole-number field with its unit written after it.
fn int_field(ui: &mut Ui, id: &str, value: &mut i32, max: i32, unit: &str) {
    let p = ui::pal(ui);
    // Right-to-left layout: the unit first, then the field to its left.
    ui.label(ui::text(unit, 12.5, p.text3));
    let mut v = f64::from(*value);
    if ui::num_field_req(ui, id, &mut v, "0", 76.0) {
        *value = (v.round() as i32).clamp(0, max);
    }
}

fn group_title(ui: &mut Ui, s: &str) {
    ui.add_space(14.0);
    ui::section_title(ui, s);
    ui.add_space(2.0);
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let p = app.pal;
    let snap = app.snap.clone();
    if app.f.settings.sync {
        app.f.settings.draft = snap.settings.clone();
        app.f.settings.sync = false;
    }
    let dirty = app.f.settings.draft != snap.settings;
    let (mut save, mut reset) = (false, false);
    top_bar(
        ui,
        |ui| {
            ui.label(ui::title("Настройки", 17.0, p.text));
        },
        |ui| {
            if dirty {
                save = ui::button(ui, Kind::Primary, Some(icon::CHECK), "Сохранить", true).clicked();
                ui.add_space(6.0);
                reset = ui::button(ui, Kind::Secondary, None, "Отменить", true).clicked();
                ui.add_space(8.0);
                ui.label(ui::text("Есть несохранённые изменения расчёта", 12.0, p.warn));
            }
        },
    );

    let mut theme_changed = false;
    let mut reveal = None;
    let (mut do_import, mut do_backup, mut do_restore) = (false, false, false);
    let (mut import_confirm, mut import_cancel) = (false, false);

    page(ui, "settings", 780.0, |ui| {
        let tab = &mut app.f.settings.tab;
        ui::segmented(
            ui,
            tab,
            &[
                (Tab::Calc, "Расчёт"),
                (Tab::Depts, "Подразделения"),
                (Tab::Bases, "Базы данных"),
                (Tab::Data, "Данные"),
                (Tab::Look, "Внешний вид"),
                (Tab::Help, "Справка"),
            ],
        );
        ui.add_space(4.0);
        match app.f.settings.tab {
            Tab::Calc => calc_tab(app, ui, dirty),
            Tab::Depts => depts_tab(app, ui, &snap),
            Tab::Bases => bases_tab(app, ui),
            Tab::Data => {
                data_tab(app, ui, &snap, &mut reveal, &mut do_import, &mut do_backup, &mut do_restore, &mut import_confirm, &mut import_cancel)
            }
            Tab::Look => look_tab(app, ui, &mut theme_changed),
            Tab::Help => help_tab(app, ui),
        }
    });

    if save {
        let s = app.f.settings.draft.clone();
        app.save_settings(&s);
        app.f.settings.sync = true;
        app.toast(true, "Настройки сохранены, остатки пересчитаны");
    }
    if reset {
        app.f.settings.sync = true;
    }
    if theme_changed {
        app.save_config();
    }
    if let Some(path) = reveal {
        let _ = std::fs::create_dir_all(&path);
        app.reveal(&path);
    }
    if do_import {
        app.pick_import();
    }
    if do_backup {
        app.backup_now();
    }
    if do_restore {
        app.restore_backup();
        app.f.settings.sync = true;
    }
    if import_cancel {
        app.import_preview = None;
    }
    if import_confirm {
        if let Some(pv) = app.import_preview.take() {
            app.apply_import(pv.import);
            app.f.settings.sync = true;
        }
    }
}

// ---- Расчёт ---------------------------------------------------------------------------------

fn calc_tab(app: &mut App, ui: &mut Ui, _dirty: bool) {
    let p = app.pal;
    let d = &mut app.f.settings.draft;

    group_title(ui, "На какую дату считаем");
    ui::card(ui, |ui| {
        let mut fixed = d.as_of_manual.is_some();
        row(ui, "Дата расчёта", "Остатки, сроки и состояния показываются на эту дату", |ui| {
            ui.horizontal(|ui| {
                if fixed {
                    let mut v = d.as_of_manual;
                    if ui::date_field(ui, "settings-asof", &mut v, "дд.мм.гггг", 150.0) {
                        d.as_of_manual = v.or(Some(date::today()));
                    }
                    ui.add_space(6.0);
                }
                if ui::segmented(ui, &mut fixed, &[(false, "Сегодня"), (true, "Своя дата")]) {
                    d.as_of_manual = fixed.then(date::today);
                }
            });
        });
    });

    group_title(ui, "Отпуск и право на него");
    ui::card(ui, |ui| {
        row(ui, "Основной отпуск", "Дней в год. Для одного сотрудника можно задать другое в его карточке", |ui| {
            ui.label(ui::text("дн./год", 12.5, p.text3));
            let mut v = d.base_days;
            if ui::num_field_req(ui, "set-base", &mut v, "28", 76.0) {
                d.base_days = v.clamp(1.0, 100.0);
            }
        });
        ui::divider(ui);
        row(ui, "Право на первый отпуск", "Через сколько месяцев работы можно взять первый отпуск", |ui| {
            int_field(ui, "set-first", &mut d.first_leave_months, 12, "мес.");
        });
        ui::divider(ui);
        row(ui, "Срок использования", "Сколько месяцев после конца рабочего года отпуск можно взять (ст. 124 ТК РФ — 12)", |ui| {
            int_field(ui, "set-deadline", &mut d.deadline_months, 60, "мес.");
        });
    });

    group_title(ui, "Подсветка сроков");
    ui::card(ui, |ui| {
        row(ui, "«Горит» — оранжевым", "До крайнего срока осталось не больше этого — пора ставить в график", |ui| {
            int_field(ui, "set-burning", &mut d.burning_days, 1000, "дн.");
        });
        ui::divider(ui);
        row(ui, "«Скоро» — жёлтым", "Отпуск надо учесть в ближайшем графике", |ui| {
            int_field(ui, "set-soon", &mut d.soon_days, 1000, "дн.");
        });
        if d.soon_days < d.burning_days {
            ui.add_space(2.0);
            ui::banner(ui, Tone::Warn, icon::WARNING, "Порог «скоро» меньше порога «горит» — жёлтая подсветка не будет использоваться.");
        }
    });

    group_title(ui, "Напоминания");
    ui::card(ui, |ui| {
        row(ui, "О начале отпуска — срочно", "Карточка становится срочной. Работника уведомляют за 14 дней (ст. 123 ТК РФ)", |ui| {
            int_field(ui, "set-remind-near", &mut d.remind_near_days, 120, "дн.");
        });
        ui::divider(ui);
        row(ui, "О начале отпуска — заранее", "За сколько месяцев до начала отпуска показывать карточку", |ui| {
            int_field(ui, "set-remind-far", &mut d.remind_far_months, 12, "мес.");
        });
        ui::divider(ui);
        row(ui, "Окончание декретного отпуска и по уходу", "За сколько дней напомнить о выходе сотрудницы на работу", |ui| {
            int_field(ui, "set-remind-end", &mut d.remind_end_days, 120, "дн.");
        });
        if d.remind_far_months > 0 && i64::from(d.remind_near_days) > i64::from(d.remind_far_months) * 31 {
            ui.add_space(2.0);
            ui::banner(ui, Tone::Warn, icon::WARNING, "Срочный срок длиннее месяца «заранее» — второй этап напоминаний не будет использоваться.");
        }
    });

    group_title(ui, "Приостановка трудового договора");
    ui::card(ui, |ui| {
        row(ui, "Засчитывать в стаж", "«Нет» — рабочие годы сдвигаются на срок приостановки (мобилизация, контракт)", |ui| {
            let mut v = d.suspension_counts;
            if ui::segmented(ui, &mut v, &[(true, "Да"), (false, "Нет")]) {
                d.suspension_counts = v;
            }
        });
    });
    ui.add_space(8.0);
    ui.label(ui::text("Изменения применяются после нажатия «Сохранить» вверху — тогда же пересчитываются остатки.", 11.5, p.text3));
}

// ---- Подразделения -------------------------------------------------------------------------

fn depts_tab(app: &mut App, ui: &mut Ui, snap: &otpuska_core::Snapshot) {
    let p = app.pal;
    let mut rename: Option<(i64, String)> = None;
    let mut delete = None;
    let mut mv: Option<(i64, bool)> = None;
    let mut add = None;

    ui.add_space(10.0);
    ui.label(ui::text("Список для выбора в карточке сотрудника и для фильтров. Порядок меняется стрелками, название — прямо в строке.", 12.5, p.text2));
    ui.add_space(8.0);
    ui::card(ui, |ui| {
        let count = snap.departments.len();
        if snap.departments.is_empty() {
            ui.label(ui::text("Список пуст — добавьте первое подразделение ниже.", 12.5, p.text3));
            ui.add_space(6.0);
        }
        for (i, d) in snap.departments.iter().enumerate() {
            let used = snap.emps.iter().filter(|r| r.emp.department_id == Some(d.id)).count();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if ui::icon_button(ui, icon::ARROW_UP, "Выше", i > 0).clicked() {
                    mv = Some((d.id, true));
                }
                if ui::icon_button(ui, icon::ARROW_DOWN, "Ниже", i + 1 < count).clicked() {
                    mv = Some((d.id, false));
                }
                let buf = app.f.settings.rename.entry(d.id).or_insert_with(|| d.name.clone());
                let w = (ui.available_width() - 190.0).max(160.0);
                let r = ui::input(ui, buf, "", w);
                let changed = buf.trim() != d.name;
                if changed && (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) || ui::button(ui, Kind::Secondary, Some(icon::CHECK), "", true).on_hover_text("Переименовать").clicked()) {
                    rename = Some((d.id, buf.clone()));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let can_delete = used == 0;
                    let r = ui::icon_button(ui, icon::TRASH, if can_delete { "Удалить" } else { "Нельзя удалить: в подразделении есть сотрудники" }, can_delete);
                    if r.clicked() {
                        delete = Some(d.id);
                    }
                    ui.label(ui::text(format!("{used} сотр."), 12.0, p.text3));
                });
            });
        }
        ui.add_space(8.0);
        ui::divider(ui);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let st = &mut app.f.settings;
            let r = ui::input(ui, &mut st.new_dept, "Название нового подразделения", ui.available_width() - 120.0);
            let go = (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) || ui::button(ui, Kind::Primary, Some(icon::PLUS), "Добавить", !st.new_dept.trim().is_empty()).clicked();
            if go && !st.new_dept.trim().is_empty() {
                add = Some(st.new_dept.clone());
            }
        });
    });
    if let Some(name) = add {
        if app.add_department(&name) {
            app.f.settings.new_dept.clear();
        }
    }
    if let Some((id, name)) = rename {
        if app.rename_department(id, &name) {
            app.f.settings.rename.remove(&id);
        }
    }
    if let Some(id) = delete {
        app.delete_department(id);
        app.f.settings.rename.remove(&id);
    }
    if let Some((id, up)) = mv {
        app.move_department(id, up);
    }
}

// ---- Базы данных -------------------------------------------------------------------------------

/// Everything the tab asks for; carried out after drawing.
#[derive(Default)]
struct BaseActs {
    rename: Option<(String, String)>,
    open: Option<String>,
    delete: Option<String>,
    reveal: Option<std::path::PathBuf>,
    create: bool,
    add_file: bool,
    ask: Option<bool>,
}

fn bases_tab(app: &mut App, ui: &mut Ui) {
    let p = app.pal;
    if app.f.settings.db_info.is_none() {
        let info = app.config.databases.iter().map(|d| (d.id.clone(), peek_entry(&app.config, d))).collect();
        app.f.settings.db_info = Some(info);
    }
    let mut act = BaseActs::default();
    let current = app.current_db.clone();
    let entries = app.config.databases.clone();

    ui.add_space(10.0);
    ui.label(ui::text(
        "У каждой организации своя база: сотрудники, отпуска и настройки расчёта не смешиваются. Название меняется прямо в строке.",
        12.5,
        p.text2,
    ));
    ui.add_space(8.0);
    ui::card(ui, |ui| {
        if entries.is_empty() {
            ui.label(ui::text("Баз пока нет.", 12.5, p.text3));
        }
        for (i, d) in entries.iter().enumerate() {
            if i > 0 {
                ui.add_space(8.0);
                ui::divider(ui);
                ui.add_space(8.0);
            }
            let is_current = current.as_deref() == Some(d.id.as_str());
            let file = app.config.db_file(d);
            let info = app.f.settings.db_info.as_ref().and_then(|m| m.get(&d.id)).cloned().unwrap_or_else(|| Err(String::new()));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let buf = app.f.settings.rename_db.entry(d.id.clone()).or_insert_with(|| d.name.clone());
                let w = (ui.available_width() - 330.0).max(160.0);
                let r = ui::input(ui, buf, "", w);
                let changed = buf.trim() != d.name;
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if changed && (enter || ui::button(ui, Kind::Secondary, Some(icon::CHECK), "", true).on_hover_text("Переименовать").clicked()) {
                    act.rename = Some((d.id.clone(), buf.clone()));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let tip = if is_current { "Нельзя удалить открытую базу — сначала откройте другую" } else { "Удалить базу" };
                    if ui::icon_button(ui, icon::TRASH, tip, !is_current).clicked() {
                        act.delete = Some(d.id.clone());
                    }
                    if is_current {
                        ui::pill(ui, "Открыта", Tone::Ok);
                    } else if ui::button(ui, Kind::Secondary, Some(icon::ARROW_RIGHT), "Открыть", info.is_ok()).clicked() {
                        act.open = Some(d.id.clone());
                    }
                    if ui::icon_button(ui, icon::FOLDER_OPEN, "Показать файл в папке", file.exists()).clicked() {
                        act.reveal = Some(file.clone());
                    }
                });
            });
            // The open database is counted live; the others as they were when the tab was opened.
            let stats = if is_current {
                describe(&Ok(Peek { employees: app.snap.emps.len() as i64, leaves: app.snap.leaves.len() as i64, modified: None }))
            } else {
                describe(&info)
            };
            ui.add_space(2.0);
            ui.add(egui::Label::new(ui::text(format!("{stats}  ·  {}", ui::tildify(&file)), 11.5, if info.is_err() && !is_current { p.danger } else { p.text3 })).truncate());
        }
    });

    ui.add_space(14.0);
    ui::section_title(ui, "Новая база");
    ui.add_space(6.0);
    ui::card(ui, |ui| {
        ui.horizontal(|ui| {
            let st = &mut app.f.settings;
            let w = (ui.available_width() - 150.0).max(160.0);
            let r = ui::input(ui, &mut st.new_db, "Название организации", w);
            let can = !st.new_db.trim().is_empty();
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let clicked = ui::button(ui, Kind::Primary, Some(icon::PLUS), "Создать", can).clicked();
            if can && (clicked || enter) {
                act.create = true;
            }
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui::link(ui, Some(icon::FOLDER_OPEN), "Добавить из файла…").clicked() {
                act.add_file = true;
            }
            ui.label(ui::text("файл .db: резервная копия или база с другого компьютера", 11.5, p.text3));
        });
    });

    ui.add_space(14.0);
    ui::card(ui, |ui| {
        row(ui, "Спрашивать при запуске", "Выключено — сразу открывается база, с которой работали в прошлый раз", |ui| {
            let mut ask = app.config.ask_on_start();
            if ui::toggle(ui, &mut ask).changed() {
                act.ask = Some(ask);
            }
        });
    });
    ui.add_space(6.0);
    ui.label(ui::text("Если в списке одна база, она открывается сразу. Сменить базу можно и плашкой с названием в левом верхнем углу.", 11.5, p.text3));

    // ---- carry out
    let mut refresh = false;
    if let Some((id, name)) = act.rename {
        if app.rename_database(&id, &name) {
            app.f.settings.rename_db.remove(&id);
        }
    }
    if let Some(path) = act.reveal {
        app.reveal(&path);
    }
    if act.create {
        let name = app.f.settings.new_db.clone();
        if app.create_database(&name) {
            app.f.settings.new_db.clear();
            refresh = true;
        }
    }
    if act.add_file {
        let name = app.f.settings.new_db.clone();
        if app.add_database_from_file(&name) {
            app.f.settings.new_db.clear();
            refresh = true;
        }
    }
    if let Some(ask) = act.ask {
        app.set_ask_on_start(ask);
    }
    if let Some(id) = act.delete {
        let key = format!("delete-db-{id}");
        if app.armed(ui, &key) {
            app.confirm(ui, &key);
            if app.delete_database(&id) {
                app.f.settings.rename_db.remove(&id);
                refresh = true;
            }
        } else {
            app.confirm(ui, &key);
            let name = app.config.find_db(&id).map(|d| d.name.clone()).unwrap_or_default();
            app.toast(true, format!("Нажмите на корзину ещё раз, чтобы удалить базу «{name}»"));
        }
    }
    if refresh {
        app.f.settings.db_info = None;
    }
    if let Some(id) = act.open {
        app.switch_database(&id);
    }
}

// ---- Данные ------------------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn data_tab(
    app: &mut App,
    ui: &mut Ui,
    snap: &otpuska_core::Snapshot,
    reveal: &mut Option<std::path::PathBuf>,
    do_import: &mut bool,
    do_backup: &mut bool,
    do_restore: &mut bool,
    import_confirm: &mut bool,
    import_cancel: &mut bool,
) {
    let p = app.pal;
    let path = app.db.path().to_path_buf();

    if let Some(name) = app.db_name() {
        ui.add_space(10.0);
        ui.label(ui::text(format!("Всё на этой вкладке относится к базе «{name}»."), 12.5, p.text2));
    }
    group_title(ui, "Где хранятся данные");
    ui::card(ui, |ui| {
        row(ui, "Файл базы", &ui::tildify(&path), |ui| {
            if ui::button(ui, Kind::Secondary, Some(icon::FOLDER_OPEN), "Показать", true).clicked() {
                *reveal = Some(path.clone());
            }
        });
        ui::divider(ui);
        ui.add_space(8.0);
        ui.label(ui::text(format!("Сейчас в базе: {} сотрудников, {} записей журнала.", snap.emps.len(), snap.leaves.len()), 12.5, p.text2));
        ui.add_space(4.0);
    });

    group_title(ui, "Резервные копии");
    ui::card(ui, |ui| {
        row(ui, "Автоматически", "Раз в день при первом запуске, хранятся 14 последних", |ui| {
            if ui::button(ui, Kind::Secondary, Some(icon::FOLDER_OPEN), "Открыть папку", true).clicked() {
                *reveal = Some(config::backups_dir(&path));
            }
        });
        ui::divider(ui);
        row(ui, "Вручную", "Сохранить копию в нужное место или вернуть данные из копии", |ui| {
            if ui::button(ui, Kind::Secondary, Some(icon::DOWNLOAD_SIMPLE), "Восстановить…", true).clicked() {
                *do_restore = true;
            }
            if ui::button(ui, Kind::Secondary, Some(icon::FLOPPY_DISK), "Сохранить копию…", true).clicked() {
                *do_backup = true;
            }
        });
    });

    group_title(ui, "Перенос из Excel");
    ui::card(ui, |ui| {
        row(ui, "Загрузить книгу Excel", "Листы «Сотрудники», «Журнал отпусков» и «Настройки» старой книги", |ui| {
            if ui::button(ui, Kind::Secondary, Some(icon::FILE_XLS), "Выбрать файл…", true).clicked() {
                *do_import = true;
            }
        });
    });

    if let Some(pv) = &app.import_preview {
        ui.add_space(12.0);
        ui::card(ui, |ui| {
            ui.label(ui::strong("Заменить данные содержимым файла?", 14.0, p.text));
            ui.add_space(2.0);
            ui.label(ui::text(pv.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), 12.0, p.text3));
            ui.add_space(6.0);
            ui.label(ui::text(format!("В файле: {}.", pv.import.summary()), 13.0, p.text));
            ui.label(ui::text(
                format!(
                    "Сейчас в базе{}: {} сотрудников, {} записей журнала — они будут заменены. Перед заменой программа сохранит копию.",
                    app.db_name().map(|n| format!(" «{n}»")).unwrap_or_default(),
                    snap.emps.len(),
                    snap.leaves.len()
                ),
                12.5,
                p.text2,
            ));
            for w in &pv.import.warnings {
                ui.label(ui::text(format!("• {w}"), 12.0, p.warn));
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                *import_confirm = ui::button(ui, Kind::DangerSolid, Some(icon::ARROWS_CLOCKWISE), "Заменить данные", true).clicked();
                *import_cancel = ui::button(ui, Kind::Secondary, None, "Отмена", true).clicked();
            });
        });
    }
}

// ---- Внешний вид -----------------------------------------------------------------------------

fn look_tab(app: &mut App, ui: &mut Ui, changed: &mut bool) {
    let p = app.pal;
    group_title(ui, "Оформление");
    ui::card(ui, |ui| {
        let mut theme = app.config.ui.theme.clone().unwrap_or_else(|| "system".into());
        row(ui, "Тема", "", |ui| {
            let mut t = theme.as_str();
            if ui::segmented(ui, &mut t, &[("system", "Как в системе"), ("dark", "Тёмная"), ("light", "Светлая")]) {
                theme = t.to_string();
            }
        });
        if app.config.ui.theme.as_deref().unwrap_or("system") != theme {
            app.config.ui.theme = Some(theme);
            *changed = true;
        }
        ui::divider(ui);
        let accent = app.config.ui.accent.clone().unwrap_or_else(|| "mono".into());
        row(ui, "Цвет акцента", "Кнопки и выделение", |ui| {
            for (id, label) in ACCENTS.iter().rev() {
                let color = crate::theme::palette(p.dark, id).accent;
                let (rect, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
                resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, accent == *id, *label));
                if accent == *id {
                    ui.painter().circle_stroke(rect.center(), 12.0, Stroke::new(2.0, p.text));
                }
                ui.painter().circle_filled(rect.center(), 8.5, color);
                ui.painter().circle_stroke(rect.center(), 8.5, Stroke::new(1.0, p.border_strong));
                if resp.on_hover_text(*label).clicked() {
                    app.config.ui.accent = Some(id.to_string());
                    *changed = true;
                }
            }
        });
        ui::divider(ui);
        let scale = app.config.ui.scale.unwrap_or(1.0);
        row(ui, "Размер интерфейса", "Если мелко или крупно на вашем экране", |ui| {
            let mut s = (scale * 100.0).round() as i32;
            if ui::segmented(ui, &mut s, &[(90, "90%"), (100, "100%"), (110, "110%"), (125, "125%"), (150, "150%")]) {
                app.config.ui.scale = Some(s as f32 / 100.0);
                *changed = true;
            }
        });
        ui::divider(ui);
        let mut reduce = app.config.ui.reduce_motion.unwrap_or(false);
        row(ui, "Без анимации", "Отключить плавные переходы", |ui| {
            if ui::toggle(ui, &mut reduce).changed() {
                app.config.ui.reduce_motion = Some(reduce);
                *changed = true;
            }
        });
    });
}

// ---- Справка -----------------------------------------------------------------------------------

fn help_tab(app: &mut App, ui: &mut Ui) {
    let p = app.pal;
    ui.add_space(10.0);
    ui::card(ui, |ui| {
        let text = |ui: &mut Ui, s: &str| {
            ui.add(egui::Label::new(ui::text(s, 12.5, p.text2)).wrap());
        };
        ui::accordion(ui, "help-states", icon::PALETTE, "Что означают состояния и цвета", true, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            for s in St::ALL {
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(vec2(150.0, 22.0), Layout::left_to_right(Align::Center), |ui| {
                        ui::state_pill(ui, s);
                    });
                    ui.add(egui::Label::new(ui::text(s.hint(), 12.5, p.text2)).wrap());
                });
            }
        });
        ui::divider(ui);
        ui::accordion(ui, "help-start", icon::LIST_CHECKS, "С чего начать", false, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            text(ui, "1. «Настройки» → «Подразделения»: добавьте подразделения (или загрузите старую книгу Excel на вкладке «Данные»). Если организаций несколько — у каждой своя база, см. ниже.");
            text(ui, "2. «Сотрудники» → «Новый сотрудник»: ФИО, подразделение, дата приема. Дальше всё считается само.");
            text(ui, "3. «Журнал отпусков»: выберите сотрудника, дату начала и число дней — «Добавить». «14+14» — это две записи.");
            text(ui, "4. «График отпусков»: запланируйте отпуска на год — за две недели и за месяц до начала программа напомнит.");
            text(ui, "5. «Сводка» покажет, у кого отпуск просрочен, «горит» или скоро. Клик по строке открывает карточку.");
        });
        ui::divider(ui);
        ui::accordion(ui, "help-bases", icon::DATABASE, "Несколько баз: разные организации", false, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            text(ui, "У каждой организации своя база: сотрудники, журнал, подразделения и настройки расчёта не смешиваются. Какая база открыта сейчас — написано в левом верхнем углу.");
            text(ui, "Сменить базу или создать новую — нажмите на эту плашку (или «Настройки» → «Базы данных»). При запуске программа спрашивает, в какую базу войти; это можно выключить — тогда откроется последняя.");
            text(ui, "Переименовать и удалить базу — «Настройки» → «Базы данных». Удалённая база ещё 30 дней лежит в папке «deleted» рядом с данными, потом стирается. Резервные копии, загрузка из Excel и восстановление всегда работают с открытой базой.");
        });
        ui::divider(ui);
        ui::accordion(ui, "help-calc", icon::CALCULATOR, "Как считается остаток", false, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            text(ui, "Рабочий год считается от даты приема и длится 12 месяцев. За каждый год положено 28 дней основного отпуска плюс дополнительный (за вредность), если он задан в карточке.");
            text(ui, "Отгулянные дни списываются с самого раннего периода — отдельно основной и дополнительный. Отпуск с датой начала в будущем считается использованным (запланированным).");
            text(ui, "Крайний срок для периода — конец рабочего года плюс срок использования (по умолчанию 12 месяцев). После него отпуск «просрочен». Если рабочий год продлён отпуском без оплаты или по уходу за ребёнком, срок сдвигается вместе с ним.");
        });
        ui::divider(ui);
        ui::accordion(ui, "help-kinds", icon::BABY, "Отпуск без оплаты, декретный, по уходу за ребёнком", false, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            text(ui, "В «Журнале отпусков» выберите вид отпуска. Ежегодный списывается с остатка. Остальные виды вносятся датами «с — по» и остаток не меняют, зато влияют на рабочий год.");
            text(ui, "Без сохранения оплаты: в рабочем году в стаж идут первые 14 календарных дней. Всё, что сверх, не засчитывается — окончание рабочего года переносится на столько же дней, а следующий год считается со дня после него (ст. 121, 128 ТК РФ). Крайние сроки отпусков сдвигаются вместе с годом.");
            text(ui, "По уходу за ребёнком: не идёт в стаж (ст. 121 ТК РФ), поэтому рабочий год продлевается на весь срок отпуска. Для окончания можно указать дату рождения ребёнка — кнопки «1,5 лет» и «3 лет» подставят дату.");
            text(ui, "По беременности и родам: идёт в стаж, рабочие годы не сдвигаются. Кнопки 140 / 156 / 194 дн. подставляют обычную продолжительность (ст. 255 ТК РФ).");
            text(ui, "Продлённые годы отмечены на вкладке «Периоды» жёлтыми датами и в карточке сотрудника значком «+N дн.».");
        });
        ui::divider(ui);
        ui::accordion(ui, "help-schedule", icon::CALENDAR_DOTS, "График отпусков и напоминания", false, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            text(ui, "«График отпусков» — план на год: сотрудник, дата начала, число дней. Программа сверяет его с журналом: «По графику» — отпуск впереди; «Оформлен» — запись в журнале есть; «В отпуске» и «Использован»; «Не оформлен» — дата наступила, а в журнале отпуска нет.");
            text(ui, "Запись графика выполняет запись журнала того же сотрудника с пересекающимися датами. Если отпуск перенесли, измените дату в графике — иначе он останется «Не оформлен». Кнопка с книгой вносит отпуск из графика в журнал.");
            text(ui, "«Напоминания»: за 14 дней и за 1 месяц до начала отпуска (по журналу и по графику), а также за 14 дней до окончания отпуска по беременности и родам и по уходу за ребёнком. Переключатель сверху оставляет нужный срок. Сроки меняются в «Настройках» → «Расчёт». Число напоминаний видно у вкладки в боковой панели и на «Сводке».");
            text(ui, "Работника нужно письменно уведомить о начале отпуска под роспись не позднее чем за 2 недели (ст. 123 ТК РФ) — на карточке напоминания указана эта дата.");
        });
        ui::divider(ui);
        ui::accordion(ui, "help-staff", icon::USER_MINUS, "Увольнение, приостановка, вредность", false, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            text(ui, "Увольнение: в карточке выберите статус «Уволен» — сотрудник исчезнет из «Сводки» и отчётов, но останется в списке «Сотрудники».");
            text(ui, "Приостановка ТД (мобилизация, контракт): статус «Приостановка ТД» и даты — отпуск не планируется, в «горящие» не попадает.");
            text(ui, "Дополнительный отпуск за вредность: в карточке укажите число дней в год — основной и дополнительный учитываются отдельно.");
        });
        ui::divider(ui);
        ui::accordion(ui, "help-print", icon::PRINTER, "Фильтры и печать", false, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            text(ui, "На каждой вкладке есть свои фильтры. Кнопка «Печать» выводит ровно то, что сейчас на экране: нет фильтров — всё, есть — только отфильтрованное. В шапке листа написано, какие фильтры стояли.");
            text(ui, "Отчёт открывается в браузере — дальше Ctrl/⌘+P или «Сохранить как PDF», масштаб 100%.");
        });
        ui::divider(ui);
        ui::accordion(ui, "help-keys", icon::KEYBOARD, "Горячие клавиши", false, |ui| {
            let cmd = if cfg!(target_os = "macos") { "⌘" } else { "Ctrl" };
            ui.spacing_mut().item_spacing.y = 6.0;
            text(ui, &format!("{cmd} + 1…6 — вкладки: Сводка, Сотрудники, Периоды, Журнал, Отчёты, Настройки; {cmd} + 7 — График, {cmd} + 8 — Напоминания."));
            text(ui, &format!("{cmd} + N — новый сотрудник."));
        });
        ui::divider(ui);
        ui::accordion(ui, "help-about", icon::INFO, "О программе", false, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(ui::strong(format!("Отпуска {}", env!("CARGO_PKG_VERSION")), 13.0, p.text));
            text(ui, "Локальная база SQLite — данные не покидают компьютер. Исходный код и новые версии: github.com/katsuna777/q");
        });
    });
}
