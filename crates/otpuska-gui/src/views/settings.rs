//! «Настройки»: five short tabs instead of one long page — calculation rules, departments,
//! data (backups, Excel import), appearance and help.

use crate::app::{page, top_bar, App};
use crate::theme::ACCENTS;
use crate::ui::{self, Kind, Tone};
use egui::{vec2, Align, Layout, Sense, Stroke, Ui};
use egui_phosphor::regular as icon;
use otpuska_core::config;
use otpuska_core::date;
use otpuska_core::engine::State as St;
use otpuska_core::model::Settings;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Tab {
    #[default]
    Calc,
    Depts,
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
}

impl Default for State {
    fn default() -> Self {
        State { tab: Tab::Calc, draft: Settings::default(), sync: true, new_dept: String::new(), rename: HashMap::new() }
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
            &[(Tab::Calc, "Расчёт"), (Tab::Depts, "Подразделения"), (Tab::Data, "Данные"), (Tab::Look, "Внешний вид"), (Tab::Help, "Справка")],
        );
        ui.add_space(4.0);
        match app.f.settings.tab {
            Tab::Calc => calc_tab(app, ui, dirty),
            Tab::Depts => depts_tab(app, ui, &snap),
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
                format!("Сейчас в базе: {} сотрудников, {} записей журнала — они будут заменены. Перед заменой программа сохранит копию.", snap.emps.len(), snap.leaves.len()),
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
            text(ui, "1. «Настройки» → «Подразделения»: добавьте подразделения (или загрузите старую книгу Excel на вкладке «Данные»).");
            text(ui, "2. «Сотрудники» → «Новый сотрудник»: ФИО, подразделение, дата приема. Дальше всё считается само.");
            text(ui, "3. «Журнал отпусков»: выберите сотрудника, дату начала и число дней — «Добавить». «14+14» — это две записи.");
            text(ui, "4. «Сводка» покажет, у кого отпуск просрочен, «горит» или скоро. Клик по строке открывает карточку.");
        });
        ui::divider(ui);
        ui::accordion(ui, "help-calc", icon::CALCULATOR, "Как считается остаток", false, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            text(ui, "Рабочий год считается от даты приема и длится 12 месяцев. За каждый год положено 28 дней основного отпуска плюс дополнительный (за вредность), если он задан в карточке.");
            text(ui, "Отгулянные дни списываются с самого раннего периода — отдельно основной и дополнительный. Отпуск с датой начала в будущем считается использованным (запланированным).");
            text(ui, "Крайний срок для периода — конец рабочего года плюс срок использования (по умолчанию 12 месяцев). После него отпуск «просрочен».");
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
            text(ui, &format!("{cmd} + 1…6 — вкладки: Сводка, Сотрудники, Периоды, Журнал, Отчёты, Настройки."));
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
