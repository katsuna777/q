//! The window root: first the database chooser, then the working screen of the chosen database.
//! Switching databases closes one `App` (and its SQLite connection) and builds another, so two
//! organizations can never get mixed up on one screen.

use crate::app::{drag_region, load_logo, sync_theme, App, Switch, ThemeKey};
use crate::theme::{self, Pal};
use crate::ui::{self, Kind, Tone};
use egui::{vec2, Align, Align2, CornerRadius, FontId, Frame, Key, Layout, Margin, Sense, Stroke, StrokeKind, Ui, UiBuilder};
use egui_phosphor::regular as icon;
use otpuska_core::config::Config;
use otpuska_core::databases::DbEntry;
use otpuska_core::db::Peek;
use otpuska_core::{date, Db};
use std::path::Path;

enum Mode {
    Chooser(Box<Chooser>),
    Work(Box<App>),
}

pub struct Root {
    ctx: egui::Context,
    mode: Mode,
    title: String,
    first_frame_reported: bool,
    /// `OTPUSKA_TIMING`: when the last report of the frame rate was made and the frames since.
    frames: (std::time::Instant, u32),
    /// The window was fitted to the screen (done once, as soon as the screen size is known).
    fitted: bool,
}

/// The size the window is made for. Smaller screens get the window cut down to fit (see `fit_to_screen`).
pub const WINDOW_SIZE: egui::Vec2 = egui::vec2(1360.0, 860.0);
pub const WINDOW_MIN: egui::Vec2 = egui::vec2(1180.0, 640.0);

/// The window and its smallest size for a screen of `monitor` points: never larger than what
/// is left of the screen after the taskbar / Dock and the title bar (a 1366×768 laptop, or a
/// Windows screen scaled to 125% or 150%). `None` when the window is fine as it is.
pub fn fit_to_screen(monitor: egui::Vec2, window: egui::Vec2) -> (Option<egui::Vec2>, egui::Vec2) {
    let room = egui::vec2(monitor.x * 0.97, monitor.y * 0.88);
    let size = egui::vec2(window.x.min(room.x), window.y.min(room.y));
    let min = egui::vec2(WINDOW_MIN.x.min(room.x), WINDOW_MIN.y.min(room.y));
    ((size != window).then_some(size), min)
}

/// Milliseconds since the process started (`OTPUSKA_TIMING`).
fn since_start() -> u128 {
    crate::STARTED.get().map_or(0, |t| t.elapsed().as_millis())
}

impl Root {
    /// Opens the database straight away when there is only one (or asking is switched off),
    /// otherwise shows the chooser.
    pub fn new(ctx: &egui::Context, config: Config) -> Root {
        theme::install_fonts(ctx);
        config.purge_deleted(date::today());
        let mut chooser = Chooser::new(ctx, config);
        let mode = match chooser.config.start_choice().and_then(|id| chooser.open(&id)) {
            Some(app) => Mode::Work(app),
            None => Mode::Chooser(Box::new(chooser)),
        };
        if crate::timing_enabled() {
            eprintln!("otpuska: окно и база готовы через {} мс после запуска процесса", since_start());
        }
        Root { ctx: ctx.clone(), mode, title: String::new(), first_frame_reported: false, frames: (std::time::Instant::now(), 0), fitted: false }
    }

    /// The working screen, if a database is open (the chooser otherwise).
    #[cfg(test)]
    pub fn app(&self) -> Option<&App> {
        match &self.mode {
            Mode::Work(app) => Some(app),
            Mode::Chooser(_) => None,
        }
    }

    #[cfg(test)]
    pub fn app_mut(&mut self) -> Option<&mut App> {
        match &mut self.mode {
            Mode::Work(app) => Some(app),
            Mode::Chooser(_) => None,
        }
    }

    /// Once the size of the screen is known: keep the window within it.
    fn fit_window(&mut self, ctx: &egui::Context) {
        if self.fitted {
            return;
        }
        let (monitor, window) = ctx.input(|i| (i.viewport().monitor_size, i.viewport().inner_rect.map(|r| r.size())));
        let (Some(monitor), Some(window)) = (monitor, window) else { return };
        self.fitted = true;
        let (size, min) = fit_to_screen(monitor, window);
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(min));
        if let Some(size) = size {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let title = match &self.mode {
            Mode::Work(app) => app.db_name().map_or_else(|| "Отпуска".to_string(), |n| format!("Отпуска — {n}")),
            Mode::Chooser(_) => "Отпуска".to_string(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }
}

impl eframe::App for Root {
    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let mut next = None;
        match &mut self.mode {
            Mode::Chooser(chooser) => {
                if let Some(app) = chooser.ui(ui) {
                    next = Some(Mode::Work(app));
                }
            }
            Mode::Work(app) => {
                eframe::App::ui(app.as_mut(), ui, frame);
                if let Some(switch) = app.pending.take() {
                    let mut chooser = Chooser::new(&self.ctx, std::mem::take(&mut app.config));
                    next = Some(match switch {
                        Switch::Chooser => Mode::Chooser(Box::new(chooser)),
                        Switch::Open(id) => match chooser.open(&id) {
                            Some(app) => Mode::Work(app),
                            None => Mode::Chooser(Box::new(chooser)),
                        },
                    });
                }
            }
        }
        if let Some(mode) = next {
            self.mode = mode;
            ui.ctx().request_repaint();
        }
        self.update_title(ui.ctx());
        self.fit_window(ui.ctx());
        if crate::timing_enabled() {
            // A program at rest draws nothing: this shows how many frames it really draws and why.
            self.frames.1 += 1;
            if self.frames.0.elapsed() >= std::time::Duration::from_secs(3) {
                eprintln!("otpuska: за {:.0} с нарисовано кадров: {}; причина последнего: {:?}", self.frames.0.elapsed().as_secs_f32(), self.frames.1, ui.ctx().repaint_causes());
                self.frames = (std::time::Instant::now(), 0);
            }
        }
        if !self.first_frame_reported {
            self.first_frame_reported = true;
            if crate::timing_enabled() {
                eprintln!("otpuska: первый кадр нарисован через {} мс после запуска процесса", since_start());
            }
        }
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        match &self.mode {
            Mode::Work(app) => eframe::App::clear_color(app.as_ref(), visuals),
            Mode::Chooser(chooser) => chooser.pal.bg.to_normalized_gamma_f32(),
        }
    }
}

// ---- helpers shared with «Настройки» -------------------------------------------------------------

/// What is in a database file, or why it cannot be read.
pub fn peek_entry(config: &Config, entry: &DbEntry) -> Result<Peek, String> {
    let file = config.db_file(entry);
    if !file.is_file() {
        return Err("Файл не найден".to_string());
    }
    Db::peek(&file).map_err(|e| format!("{e:#}"))
}

/// «1 сотрудник», «3 сотрудника», «5 сотрудников».
pub fn plural(n: i64, one: &str, few: &str, many: &str) -> String {
    let word = match (n % 10, n % 100) {
        (_, 11..=14) => many,
        (1, _) => one,
        (2..=4, _) => few,
        _ => many,
    };
    format!("{n} {word}")
}

/// «47 сотрудников, 213 записей журнала · изменена 21.10.2026»
pub fn describe(info: &Result<Peek, String>) -> String {
    match info {
        Err(e) => e.clone(),
        Ok(p) if p.employees == 0 && p.leaves == 0 => "Пока пусто".to_string(),
        Ok(p) => {
            let mut s = format!("{}, {} журнала", plural(p.employees, "сотрудник", "сотрудника", "сотрудников"), plural(p.leaves, "запись", "записи", "записей"));
            if let Some(t) = p.modified {
                s += &format!(" · изменена {}", chrono::DateTime::<chrono::Local>::from(t).format("%d.%m.%Y"));
            }
            s
        }
    }
}

/// `base`, or `base (2)`, `base (3)`… when that name is taken.
fn free_name(config: &Config, base: &str) -> String {
    let taken = |n: &str| config.databases.iter().any(|d| d.name.to_lowercase() == n.to_lowercase());
    (1..100).map(|i| if i == 1 { base.to_string() } else { format!("{base} ({i})") }).find(|n| !taken(n)).unwrap_or_else(|| base.to_string())
}

/// File dialog plus `Config::add_database_file`; `None` when the dialog was cancelled.
/// Without a typed name the file's own name is used.
pub fn pick_and_add_database(config: &mut Config, typed_name: &str) -> Option<anyhow::Result<String>> {
    let mut dialog = rfd::FileDialog::new().set_title("Выберите файл базы или резервной копии").add_filter("База «Отпуска»", &["db"]);
    if let Some(dir) = config.last_dir.clone().filter(|d| d.is_dir()) {
        dialog = dialog.set_directory(dir);
    }
    let file = dialog.pick_file()?;
    config.last_dir = file.parent().map(Path::to_path_buf);
    let name = if typed_name.trim().is_empty() {
        let stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).filter(|s| !s.is_empty());
        free_name(config, stem.as_deref().unwrap_or("База из файла"))
    } else {
        typed_name.to_string()
    };
    Some(config.add_database_file(&name, &file))
}

// ---- the chooser --------------------------------------------------------------------------------

struct Row {
    id: String,
    name: String,
    info: Result<Peek, String>,
}

#[derive(Default)]
struct Act {
    open: Option<String>,
    forget: Option<String>,
    create: bool,
    add_file: bool,
    ask: Option<bool>,
}

pub struct Chooser {
    pub config: Config,
    pub pal: Pal,
    applied: Option<ThemeKey>,
    ctx: egui::Context,
    logo: Option<egui::TextureHandle>,
    rows: Vec<Row>,
    new_name: String,
    /// A message above the list: `true` — done, `false` — a problem.
    notice: Option<(bool, String)>,
    focus_new: bool,
}

impl Chooser {
    pub fn new(ctx: &egui::Context, config: Config) -> Chooser {
        let mut c = Chooser {
            config,
            pal: theme::palette(true, "mono"),
            applied: None,
            ctx: ctx.clone(),
            logo: load_logo(ctx),
            rows: Vec::new(),
            new_name: String::new(),
            notice: None,
            focus_new: true,
        };
        c.refresh();
        c
    }

    fn refresh(&mut self) {
        let rows = self.config.databases.iter().map(|d| Row { id: d.id.clone(), name: d.name.clone(), info: peek_entry(&self.config, d) }).collect();
        self.rows = rows;
    }

    fn save(&mut self) {
        if let Err(e) = self.config.save() {
            self.notice = Some((false, format!("Не удалось сохранить настройки: {e:#}")));
        }
    }

    /// Opens a database for work. On failure the reason is shown above the list.
    pub fn open(&mut self, id: &str) -> Option<Box<App>> {
        let opened = self.config.open_database(id, date::today());
        match opened {
            Ok(db) => {
                self.save(); // remembers the last database
                match App::new(&self.ctx, db, self.config.clone()) {
                    Ok(mut app) => {
                        app.current_db = Some(id.to_string());
                        Some(Box::new(app))
                    }
                    Err(e) => {
                        self.notice = Some((false, format!("Не удалось прочитать базу: {e:#}")));
                        None
                    }
                }
            }
            Err(e) => {
                self.notice = Some((false, format!("{e:#}")));
                self.refresh();
                None
            }
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) -> Option<Box<App>> {
        let ctx = ui.ctx().clone();
        sync_theme(&ctx, &self.config.ui, &mut self.pal, &mut self.applied);
        let typing = ctx.egui_wants_keyboard_input();
        let p = self.pal;
        let mut act = Act::default();
        egui::CentralPanel::no_frame().frame(Frame::new().fill(p.bg)).show(ui, |ui| {
            drag_region(ui, if cfg!(target_os = "macos") { 38.0 } else { 14.0 });
            egui::ScrollArea::vertical().id_salt("chooser").auto_shrink([false, false]).show(ui, |ui| {
                Frame::new()
                    .inner_margin(Margin { left: 28, right: 28, top: 20, bottom: 40 })
                    .show(ui, |ui| ui::centered(ui, 560.0, |ui| self.content(ui, &mut act)));
            });
        });
        // Enter opens the one used last (unless the name field is being typed in).
        if act.open.is_none() && !act.create && !typing && ctx.input(|i| i.key_pressed(Key::Enter)) {
            let last = self.config.last_db.as_deref();
            act.open = self.rows.iter().find(|r| r.info.is_ok() && Some(r.id.as_str()) == last).map(|r| r.id.clone());
        }
        self.apply(act)
    }

    fn apply(&mut self, act: Act) -> Option<Box<App>> {
        if let Some(ask) = act.ask {
            self.config.ask_on_start = Some(ask);
            self.save();
        }
        if let Some(id) = act.forget {
            self.notice = match self.config.forget_database(&id) {
                Ok(()) => {
                    self.save();
                    Some((true, "База убрана из списка".to_string()))
                }
                Err(e) => Some((false, format!("{e:#}"))),
            };
            self.refresh();
        }
        if act.add_file {
            let typed = std::mem::take(&mut self.new_name);
            match pick_and_add_database(&mut self.config, &typed) {
                Some(Ok(id)) => {
                    self.save();
                    let name = self.config.find_db(&id).map(|d| d.name.clone()).unwrap_or_default();
                    self.notice = Some((true, format!("База «{name}» добавлена — выберите её в списке")));
                }
                Some(Err(e)) => {
                    self.new_name = typed;
                    self.notice = Some((false, format!("{e:#}")));
                }
                None => self.new_name = typed,
            }
            self.refresh();
        }
        if act.create {
            match self.config.create_database(&self.new_name) {
                Ok(id) => {
                    self.new_name.clear();
                    self.save();
                    self.refresh();
                    return self.open(&id);
                }
                Err(e) => self.notice = Some((false, format!("{e:#}"))),
            }
        }
        act.open.and_then(|id| self.open(&id))
    }

    fn content(&mut self, ui: &mut Ui, act: &mut Act) {
        let p = self.pal;
        let empty = self.rows.is_empty();

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if let Some(logo) = &self.logo {
                ui.add(egui::Image::new((logo.id(), vec2(40.0, 40.0))));
            }
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(ui::title("Отпуска", 18.0, p.text));
                ui.label(ui::text(format!("версия {}", env!("CARGO_PKG_VERSION")), 11.5, p.text3));
            });
        });
        ui.add_space(26.0);
        ui.label(ui::title(if empty { "Создайте первую базу" } else { "Выберите базу данных" }, 24.0, p.text));
        ui.add_space(4.0);
        let hint = if empty {
            "В базе хранятся сотрудники, отпуска и настройки одной организации. Назовите её по организации — данные из Excel можно загрузить потом."
        } else {
            "У каждой организации своя база: сотрудники, отпуска и настройки не смешиваются. Сменить базу можно в любой момент — плашка с названием в левом верхнем углу."
        };
        ui.add(egui::Label::new(ui::text(hint, 13.0, p.text2)).wrap());
        ui.add_space(16.0);

        if let Some((ok, msg)) = &self.notice {
            ui::banner(ui, if *ok { Tone::Ok } else { Tone::Danger }, if *ok { icon::CHECK_CIRCLE } else { icon::WARNING_CIRCLE }, msg);
            ui.add_space(12.0);
        }

        let last = self.config.last_db.clone();
        for row in &self.rows {
            match db_card(ui, &p, row, last.as_deref() == Some(row.id.as_str())) {
                Card::Open => act.open = Some(row.id.clone()),
                Card::Forget => act.forget = Some(row.id.clone()),
                Card::None => {}
            }
            ui.add_space(8.0);
        }

        ui.add_space(if empty { 0.0 } else { 14.0 });
        if !empty {
            ui::section_title(ui, "Новая база");
            ui.add_space(6.0);
        }
        ui::card(ui, |ui| {
            ui.horizontal(|ui| {
                let width = (ui.available_width() - 190.0).max(160.0);
                let r = ui::input(ui, &mut self.new_name, "Название организации, например «Школа №5»", width);
                if std::mem::take(&mut self.focus_new) && empty {
                    r.request_focus();
                }
                let can = !self.new_name.trim().is_empty();
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                let clicked = ui::button(ui, Kind::Primary, Some(icon::PLUS), "Создать и открыть", can).clicked();
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

        if self.rows.len() >= 2 {
            ui.add_space(18.0);
            ui.horizontal(|ui| {
                let mut ask = self.config.ask_on_start();
                if ui::toggle(ui, &mut ask).changed() {
                    act.ask = Some(ask);
                }
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(ui::strong("Спрашивать при каждом запуске", 13.0, p.text));
                    ui.label(ui::text("Если выключить, программа сразу откроет ту базу, что была открыта в прошлый раз", 11.5, p.text3));
                });
            });
        }
    }
}

enum Card {
    None,
    Open,
    Forget,
}

/// One database: the whole card opens it. A database whose file has gone offers to leave the list.
fn db_card(ui: &mut Ui, p: &Pal, row: &Row, last: bool) -> Card {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 66.0), Sense::hover());
    let ok = row.info.is_ok();
    // Registered before the button inside, so that the button wins where they overlap.
    let bg = ui.interact(rect, ui.id().with(("db-card", &row.id)), if ok { Sense::click() } else { Sense::hover() });
    bg.widget_info(|| {
        let label = match &row.info {
            Ok(_) => format!("Открыть базу {}", row.name),
            Err(e) => format!("База {} недоступна: {}", row.name, e.lines().next().unwrap_or("")),
        };
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ok, label)
    });
    let hot = ok && bg.hovered();
    let marked = last && ok;
    ui.painter().rect(
        rect,
        CornerRadius::same(10),
        if hot { p.sunken } else { p.panel },
        Stroke::new(if marked { 1.5 } else { 1.0 }, if marked { p.accent } else if hot { p.border_strong } else { p.border }),
        StrokeKind::Inside,
    );
    ui.painter().text(rect.left_center() + vec2(28.0, 0.0), Align2::LEFT_CENTER, icon::DATABASE, FontId::new(22.0, theme::icons()), if ok { p.text2 } else { p.text3 });

    let right = if ok { 112.0 } else { 150.0 };
    let clip = egui::Rect::from_min_max(rect.left_top() + vec2(64.0, 0.0), rect.right_bottom() - vec2(right, 0.0));
    let painter = ui.painter().with_clip_rect(clip);
    let name = painter.layout_no_wrap(row.name.clone(), FontId::new(14.5, theme::semibold()), p.text);
    let name_w = name.size().x;
    painter.galley(rect.left_top() + vec2(64.0, 14.0), name, p.text);
    if marked && 64.0 + name_w + 100.0 < rect.width() - right {
        painter.text(rect.left_top() + vec2(64.0 + name_w + 10.0, 17.0), Align2::LEFT_TOP, "последняя", FontId::proportional(11.0), p.text3);
    }
    let (sub, color) = match &row.info {
        Ok(_) => (describe(&row.info), p.text3),
        Err(e) => (e.lines().next().unwrap_or("").to_string(), p.danger),
    };
    painter.text(rect.left_top() + vec2(64.0, 38.0), Align2::LEFT_TOP, sub, FontId::proportional(12.0), color);

    if ok {
        let c = if hot { p.text } else { p.text2 };
        ui.painter().text(rect.right_center() - vec2(16.0, 0.0), Align2::RIGHT_CENTER, icon::ARROW_RIGHT, FontId::new(16.0, theme::icons()), c);
        ui.painter().text(rect.right_center() - vec2(40.0, 0.0), Align2::RIGHT_CENTER, "Открыть", FontId::new(13.0, theme::medium()), c);
        if hot {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        return if bg.clicked() { Card::Open } else { Card::None };
    }
    let mut inner = ui.new_child(UiBuilder::new().max_rect(rect.shrink2(vec2(12.0, 0.0))).layout(Layout::right_to_left(Align::Center)));
    if ui::button(&mut inner, Kind::Danger, Some(icon::TRASH), "Убрать из списка", true).clicked() {
        return Card::Forget;
    }
    Card::None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn russian_plurals() {
        let f = |n| plural(n, "сотрудник", "сотрудника", "сотрудников");
        assert_eq!(f(1), "1 сотрудник");
        assert_eq!(f(2), "2 сотрудника");
        assert_eq!(f(5), "5 сотрудников");
        assert_eq!(f(11), "11 сотрудников");
        assert_eq!(f(21), "21 сотрудник");
        assert_eq!(f(112), "112 сотрудников");
        assert_eq!(f(0), "0 сотрудников");
    }

    #[test]
    fn database_summary_lines() {
        let p = |e, l| Ok(Peek { employees: e, leaves: l, modified: None });
        assert_eq!(describe(&p(0, 0)), "Пока пусто");
        assert_eq!(describe(&p(47, 213)), "47 сотрудников, 213 записей журнала");
        assert_eq!(describe(&p(1, 2)), "1 сотрудник, 2 записи журнала");
        assert_eq!(describe(&Err("Файл не найден".into())), "Файл не найден");
    }
}
