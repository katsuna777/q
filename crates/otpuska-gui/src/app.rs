//! Application state, database actions and the window shell (sidebar, header, toasts).

use crate::theme::{self, Pal};
use crate::ui;
use crate::views;
use anyhow::Result;
use egui::{
    vec2, Align, Align2, Color32, CornerRadius, Frame, Id, Layout, Margin, Sense, Stroke, Ui, UiBuilder,
};
use egui_phosphor::regular as icon;
use otpuska_core::config::{self, Config};
use otpuska_core::date::{self, Date};
use otpuska_core::db::Import;
use otpuska_core::model::*;
use otpuska_core::{Db, Snapshot};
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    New,
    Id(i64),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Route {
    Summary,
    Employees,
    Employee(Target),
    Periods,
    Journal,
    Reports,
    Settings,
}

impl Route {
    /// The sidebar item that stays highlighted for this route.
    fn nav(&self) -> Route {
        match self {
            Route::Employee(_) => Route::Employees,
            other => other.clone(),
        }
    }
}

pub struct Toast {
    pub ok: bool,
    pub text: String,
    born: f64,
}

/// A file picked for import, waiting for the user's confirmation.
pub struct ImportPreview {
    pub path: PathBuf,
    pub import: Import,
}

pub struct App {
    pub db: Db,
    pub snap: Rc<Snapshot>,
    pub config: Config,
    pub pal: Pal,
    pub route: Route,
    /// Where the "back" link of an employee card leads.
    pub came_from: Route,
    pub toasts: Vec<Toast>,
    pub today: Date,
    pub f: views::Forms,
    pub import_preview: Option<ImportPreview>,
    armed: Option<(String, f64)>,
    applied: Option<(bool, String, bool, i32)>,
    logo: Option<egui::TextureHandle>,
    ctx: egui::Context,
    /// Tests read the printed reports here instead of opening a browser.
    #[cfg(test)]
    pub printed: Vec<(String, String)>,
}

impl App {
    pub fn new(ctx: &egui::Context, db: Db, config: Config) -> Result<Self> {
        theme::install_fonts(ctx);
        let logo = image::load_from_memory(include_bytes!("../assets/icon.png")).ok().map(|img| {
            let rgba = img.to_rgba8();
            let size = [rgba.width() as usize, rgba.height() as usize];
            ctx.load_texture("logo", egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()), egui::TextureOptions::LINEAR)
        });
        let today = date::today();
        let data = db.load()?;
        let snap = Rc::new(Snapshot::build(&data, today));
        let pal = theme::palette(true, "mono");
        let mut app = App {
            db,
            snap,
            config,
            pal,
            route: Route::Summary,
            came_from: Route::Employees,
            toasts: Vec::new(),
            today,
            f: views::Forms::default(),
            import_preview: None,
            armed: None,
            applied: None,
            logo,
            ctx: ctx.clone(),
            #[cfg(test)]
            printed: Vec::new(),
        };
        app.f.periods.first_year = app.snap.default_first_year();
        Ok(app)
    }

    // ---- navigation ----------------------------------------------------------------

    pub fn go(&mut self, route: Route) {
        if let Route::Employee(_) = route {
            if !matches!(self.route, Route::Employee(_)) {
                self.came_from = self.route.clone();
            }
        }
        if self.route != route {
            self.f.on_leave(&self.route);
            self.route = route;
        }
        self.ctx.request_repaint();
    }

    pub fn open_employee(&mut self, id: i64) {
        self.f.employee.load(&self.snap, id);
        self.go(Route::Employee(Target::Id(id)));
    }

    pub fn new_employee(&mut self) {
        self.f.employee.start_new();
        self.go(Route::Employee(Target::New));
    }

    // ---- feedback --------------------------------------------------------------------

    pub fn toast(&mut self, ok: bool, text: impl Into<String>) {
        let born = self.ctx.input(|i| i.time);
        self.toasts.push(Toast { ok, text: text.into(), born });
        self.ctx.request_repaint();
    }

    fn report(&mut self, result: Result<()>, ok_text: &str) -> bool {
        match result {
            Ok(()) => {
                if !ok_text.is_empty() {
                    self.toast(true, ok_text);
                }
                true
            }
            Err(e) => {
                self.toast(false, format!("{e:#}"));
                false
            }
        }
    }

    /// Two-step confirmation for destructive buttons: the first click arms, the second confirms.
    pub fn confirm(&mut self, ui: &Ui, key: &str) -> bool {
        let now = ui.input(|i| i.time);
        if self.armed(ui, key) {
            self.armed = None;
            true
        } else {
            self.armed = Some((key.to_string(), now));
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(4100));
            false
        }
    }

    pub fn armed(&self, ui: &Ui, key: &str) -> bool {
        let now = ui.input(|i| i.time);
        self.armed.as_ref().is_some_and(|(k, t)| k == key && now - t < 4.0)
    }

    // ---- data ------------------------------------------------------------------------

    pub fn reload(&mut self) {
        match self.db.load() {
            Ok(data) => self.snap = Rc::new(Snapshot::build(&data, self.today)),
            Err(e) => self.toast(false, format!("Не удалось прочитать базу: {e:#}")),
        }
        self.ctx.request_repaint();
    }

    /// Inserts (`id == 0`) or updates an employee. Returns the id on success.
    pub fn save_employee(&mut self, e: &Employee) -> Option<i64> {
        let result = if e.id == 0 { self.db.insert_employee(e) } else { self.db.update_employee(e).map(|_| e.id) };
        match result {
            Ok(id) => {
                self.reload();
                Some(id)
            }
            Err(err) => {
                self.toast(false, format!("{err:#}"));
                None
            }
        }
    }

    pub fn delete_employee(&mut self, id: i64) -> bool {
        let r = self.db.delete_employee(id);
        let ok = self.report(r, "Сотрудник удалён");
        self.reload();
        ok
    }

    pub fn save_leave(&mut self, l: &Leave) -> bool {
        let r = if l.id == 0 { self.db.insert_leave(l).map(|_| ()) } else { self.db.update_leave(l) };
        let ok = self.report(r, "");
        self.reload();
        ok
    }

    pub fn delete_leave(&mut self, id: i64) {
        let r = self.db.delete_leave(id);
        self.report(r, "Запись удалена");
        self.reload();
    }

    pub fn save_settings(&mut self, s: &Settings) {
        let r = self.db.save_settings(s);
        self.report(r, "");
        self.reload();
    }

    pub fn add_department(&mut self, name: &str) -> bool {
        let r = self.db.add_department(name).map(|_| ());
        let ok = self.report(r, "");
        self.reload();
        ok
    }

    pub fn rename_department(&mut self, id: i64, name: &str) -> bool {
        let r = self.db.rename_department(id, name);
        let ok = self.report(r, "");
        self.reload();
        ok
    }

    pub fn delete_department(&mut self, id: i64) {
        let r = self.db.delete_department(id);
        self.report(r, "Подразделение удалено");
        self.reload();
    }

    pub fn move_department(&mut self, id: i64, up: bool) {
        let r = self.db.move_department(id, up);
        self.report(r, "");
        self.reload();
    }

    // ---- files -----------------------------------------------------------------------

    fn dialog_dir(&self) -> Option<PathBuf> {
        self.config.last_dir.clone().filter(|d| d.is_dir()).or_else(dirs_home)
    }

    fn remember_dir(&mut self, file: &std::path::Path) {
        if let Some(dir) = file.parent() {
            self.config.last_dir = Some(dir.to_path_buf());
            self.save_config();
        }
    }

    pub fn save_config(&mut self) {
        if let Err(e) = self.config.save() {
            self.toast(false, format!("Не удалось сохранить настройки: {e:#}"));
        }
    }

    /// Pick an Excel workbook; an empty database is filled right away, otherwise the user confirms.
    pub fn pick_import(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_title("Выберите книгу Excel с учётом отпусков").add_filter("Excel", &["xlsx", "xlsm"]);
        if let Some(dir) = self.dialog_dir() {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.pick_file() else { return };
        self.remember_dir(&path);
        match otpuska_core::xlsx_import::read_workbook(&path) {
            Ok(import) => {
                if self.db.is_empty().unwrap_or(false) {
                    self.apply_import(import);
                } else {
                    self.import_preview = Some(ImportPreview { path, import });
                    self.go(Route::Settings);
                }
            }
            Err(e) => self.toast(false, format!("{e:#}")),
        }
    }

    pub fn apply_import(&mut self, import: Import) {
        // Keep the previous state next to the daily backups.
        if self.db.is_empty().unwrap_or(true) == false {
            let dir = config::backups_dir(self.db.path());
            if std::fs::create_dir_all(&dir).is_ok() {
                let stamp = chrono::Local::now().format("%Y-%m-%d-%H%M%S");
                let _ = self.db.backup_to(&dir.join(format!("otpuska-before-import-{stamp}.db")));
            }
        }
        let summary = import.summary();
        match self.db.replace_all(&import) {
            Ok(()) => {
                self.import_preview = None;
                self.reload();
                self.toast(true, format!("Загружено: {summary}"));
                self.go(Route::Summary);
            }
            Err(e) => self.toast(false, format!("Импорт не удался: {e:#}")),
        }
    }

    pub fn backup_now(&mut self) {
        let stamp = chrono::Local::now().format("%Y-%m-%d");
        let mut dialog = rfd::FileDialog::new().set_title("Сохранить резервную копию").set_file_name(format!("otpuska-{stamp}.db")).add_filter("База «Отпуска»", &["db"]);
        if let Some(dir) = self.dialog_dir() {
            dialog = dialog.set_directory(dir);
        }
        let Some(path) = dialog.save_file() else { return };
        self.remember_dir(&path);
        let r = self.db.backup_to(&path);
        self.report(r, "Резервная копия сохранена");
    }

    /// Replace the database with a backup file (the current one is kept as `*.before-restore`).
    pub fn restore_backup(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_title("Выберите резервную копию").add_filter("База «Отпуска»", &["db"]);
        if let Some(dir) = self.dialog_dir().or_else(|| Some(config::backups_dir(self.db.path()))) {
            dialog = dialog.set_directory(dir);
        }
        let Some(file) = dialog.pick_file() else { return };
        self.remember_dir(&file);
        let path = self.db.path().to_path_buf();
        match otpuska_core::Db::verify_file(&file) {
            Ok(_) => {}
            Err(e) => {
                self.toast(false, format!("{e:#}"));
                return;
            }
        }
        // Close the open connection by swapping in an in-memory one while the file is replaced.
        let old = std::mem::replace(&mut self.db, Db::open_memory().expect("in-memory database"));
        drop(old);
        let result = config::restore_file(&path, &file).and_then(|_| Db::open(&path));
        match result {
            Ok(db) => {
                self.db = db;
                self.reload();
                self.toast(true, "База восстановлена из копии");
            }
            Err(e) => {
                if let Ok(db) = Db::open(&path) {
                    self.db = db;
                }
                self.reload();
                self.toast(false, format!("Не удалось восстановить: {e:#}"));
            }
        }
    }

    /// Writes a printable HTML report and opens it in the default browser.
    pub fn print(&mut self, html: String, name: &str) {
        #[cfg(test)]
        {
            self.printed.push((name.to_string(), html));
            return;
        }
        #[allow(unreachable_code)]
        let dir = std::env::temp_dir().join("otpuska-print");
        let result = std::fs::create_dir_all(&dir).and_then(|_| {
            let file = dir.join(format!("{name}.html"));
            std::fs::write(&file, html).map(|_| file)
        });
        match result {
            Ok(file) => {
                if let Err(e) = crate::platform::open(&file) {
                    self.toast(false, format!("Не удалось открыть отчёт: {e}"));
                }
            }
            Err(e) => self.toast(false, format!("Не удалось подготовить отчёт: {e}")),
        }
    }

    pub fn reveal(&mut self, path: &std::path::Path) {
        if let Err(e) = crate::platform::reveal(path) {
            self.toast(false, format!("Не удалось открыть папку: {e}"));
        }
    }

    // ---- frame -------------------------------------------------------------------------

    fn apply_theme(&mut self, ctx: &egui::Context) {
        let dark = match self.config.ui.theme.as_deref() {
            Some("light") => false,
            Some("dark") => true,
            _ => ctx.system_theme().map(|t| t == egui::Theme::Dark).unwrap_or(true),
        };
        let accent = self.config.ui.accent.clone().unwrap_or_else(|| "mono".into());
        let reduce = self.config.ui.reduce_motion.unwrap_or(false);
        let scale = (self.config.ui.scale.unwrap_or(1.0) * 100.0).round() as i32;
        let key = (dark, accent.clone(), reduce, scale);
        if self.applied.as_ref() != Some(&key) {
            self.pal = theme::palette(dark, &accent);
            theme::apply(ctx, &self.pal, reduce);
            ctx.set_zoom_factor(scale as f32 / 100.0);
            let pal = self.pal;
            ctx.data_mut(|d| d.insert_temp(Id::NULL, pal));
            self.applied = Some(key);
        }
    }

    /// The calculation date follows the clock; rebuild when midnight passes.
    fn check_day(&mut self) {
        let now = date::today();
        if now != self.today {
            self.today = now;
            self.reload();
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, Modifiers};
        let routes = [
            (Key::Num1, Route::Summary),
            (Key::Num2, Route::Employees),
            (Key::Num3, Route::Periods),
            (Key::Num4, Route::Journal),
            (Key::Num5, Route::Reports),
            (Key::Num6, Route::Settings),
        ];
        for (key, route) in routes {
            if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, key)) {
                self.go(route);
            }
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::N)) {
            self.new_employee();
        }
    }
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.apply_theme(&ctx);
        self.check_day();
        self.shortcuts(&ctx);
        ctx.request_repaint_after(std::time::Duration::from_secs(60));

        let p = self.pal;
        egui::Panel::left("sidebar")
            .exact_size(228.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(p.sidebar))
            .show(ui, |ui| self.sidebar(ui));
        egui::CentralPanel::no_frame().frame(Frame::new().fill(p.bg)).show(ui, |ui| {
            // Hairline between sidebar and content.
            let r = ui.max_rect();
            ui.painter().line_segment([r.left_top(), r.left_bottom()], Stroke::new(1.0, p.border));
            match self.route.clone() {
                Route::Summary => views::summary::show(self, ui),
                Route::Employees => views::employees::show(self, ui),
                Route::Employee(target) => views::employee::show(self, ui, &target),
                Route::Periods => views::periods::show(self, ui),
                Route::Journal => views::journal::show(self, ui),
                Route::Reports => views::reports::show(self, ui),
                Route::Settings => views::settings::show(self, ui),
            }
        });
        self.draw_toasts(&ctx);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.pal.bg.to_normalized_gamma_f32()
    }
}

// ---- shell ------------------------------------------------------------------------

/// Window-dragging strip (the title bar is transparent on macOS).
pub fn drag_region(ui: &mut Ui, height: f32) -> egui::Rect {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click_and_drag());
    if resp.drag_started() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    if resp.double_clicked() {
        let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
    rect
}

/// Fixed page header: draggable, title on the left, actions on the right.
pub fn top_bar(ui: &mut Ui, left: impl FnOnce(&mut Ui), right: impl FnOnce(&mut Ui)) {
    let p = ui::pal(ui);
    let rect = drag_region(ui, 56.0);
    let inner = rect.shrink2(vec2(28.0, 0.0));
    // Actions first, so the title side can be clipped before it runs under them.
    let mut r = ui.new_child(UiBuilder::new().max_rect(inner).layout(Layout::right_to_left(Align::Center)));
    right(&mut r);
    let left_rect = egui::Rect::from_min_max(inner.min, egui::pos2((r.min_rect().left() - 20.0).max(inner.left()), inner.max.y));
    let mut l = ui.new_child(UiBuilder::new().max_rect(left_rect).layout(Layout::left_to_right(Align::Center)));
    l.set_clip_rect(left_rect.intersect(ui.clip_rect()));
    left(&mut l);
    ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, p.border));
}

/// Scrollable page body with a centered column.
pub fn page<R>(ui: &mut Ui, id: &str, max_width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            Frame::new()
                .inner_margin(Margin { left: 28, right: 28, top: 22, bottom: 40 })
                .show(ui, |ui| ui::centered(ui, max_width, add))
                .inner
        })
        .inner
}

/// Page body without scrolling (the table inside scrolls by itself).
pub fn fixed_page<R>(ui: &mut Ui, max_width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .inner_margin(Margin { left: 28, right: 28, top: 18, bottom: 22 })
        .show(ui, |ui| ui::centered(ui, max_width, add))
        .inner
}

impl App {
    fn nav_item(&mut self, ui: &mut Ui, glyph: &str, label: &str, shortcut: &str, route: Route, badge: Option<(String, Color32)>) {
        let p = self.pal;
        let active = self.route.nav() == route;
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
        if active {
            ui.painter().rect_filled(rect, CornerRadius::same(7), p.active);
        } else if resp.hovered() {
            ui.painter().rect_filled(rect, CornerRadius::same(7), p.hover);
        }
        let color = if active || resp.hovered() { p.text } else { p.text2 };
        ui.painter().text(rect.left_center() + vec2(12.0, 0.0), Align2::LEFT_CENTER, glyph, egui::FontId::new(16.0, theme::icons()), color);
        ui.painter().text(rect.left_center() + vec2(38.0, 0.0), Align2::LEFT_CENTER, label, egui::FontId::new(13.0, theme::medium()), color);
        if let Some((text, c)) = badge {
            ui.painter().text(rect.right_center() - vec2(10.0, 0.0), Align2::RIGHT_CENTER, text, egui::FontId::proportional(11.5), c);
        } else if resp.hovered() {
            ui.painter().text(rect.right_center() - vec2(10.0, 0.0), Align2::RIGHT_CENTER, shortcut, egui::FontId::proportional(11.0), p.text3);
        }
        if resp.clicked() {
            self.go(route);
        }
    }

    fn sidebar(&mut self, ui: &mut Ui) {
        let p = self.pal;
        // macOS: traffic lights sit over the sidebar; elsewhere the native title bar is above.
        drag_region(ui, if cfg!(target_os = "macos") { 38.0 } else { 14.0 });
        Frame::new().inner_margin(Margin { left: 10, right: 10, top: 0, bottom: 10 }).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                if let Some(logo) = &self.logo {
                    ui.add(egui::Image::new((logo.id(), vec2(26.0, 26.0))));
                }
                ui.label(ui::title("Отпуска", 15.5, p.text));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui::icon_button(ui, icon::USER_PLUS, "Новый сотрудник", true).clicked() {
                        self.new_employee();
                    }
                });
            });
            ui.add_space(12.0);
            ui.spacing_mut().item_spacing.y = 2.0;
            let snap = self.snap.clone();
            let counts = snap.counts(None);
            let hot = counts.overdue + counts.burning;
            let hot_badge = (hot > 0).then(|| (hot.to_string(), if counts.overdue > 0 { p.danger } else { p.orange }));
            let cmd = if cfg!(target_os = "macos") { "⌘" } else { "Ctrl+" };
            self.nav_item(ui, icon::SQUARES_FOUR, "Сводка", &format!("{cmd}1"), Route::Summary, hot_badge);
            self.nav_item(ui, icon::USERS, "Сотрудники", &format!("{cmd}2"), Route::Employees, Some((snap.active_count().to_string(), p.text3)));
            self.nav_item(ui, icon::TABLE, "Периоды", &format!("{cmd}3"), Route::Periods, None);
            let leaves = (!snap.leaves.is_empty()).then(|| (snap.leaves.len().to_string(), p.text3));
            self.nav_item(ui, icon::BOOK_OPEN_TEXT, "Журнал отпусков", &format!("{cmd}4"), Route::Journal, leaves);
            self.nav_item(ui, icon::CHART_BAR, "Отчёты", &format!("{cmd}5"), Route::Reports, None);
            self.nav_item(ui, icon::GEAR, "Настройки", &format!("{cmd}6"), Route::Settings, None);

            // Footer: the date everything is calculated for.
            ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                let fixed = snap.settings.as_of_manual.is_some();
                let resp = Frame::new()
                    .fill(p.panel)
                    .stroke(Stroke::new(1.0, p.border))
                    .corner_radius(CornerRadius::same(9))
                    .inner_margin(Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(ui::icon_text(icon::CALENDAR_CHECK, 15.0, if fixed { p.warn } else { p.text2 }));
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                ui.label(ui::strong(format!("На {}", date::fmt(snap.params.as_of)), 12.5, p.text));
                                ui.label(ui::text(if fixed { "дата зафиксирована" } else { "сегодня, автоматически" }, 11.0, if fixed { p.warn } else { p.text3 }));
                            });
                        });
                    })
                    .response
                    .interact(Sense::click());
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if resp.on_hover_text("Изменить дату расчёта — в «Настройках»").clicked() {
                    self.go(Route::Settings);
                }
            });
        });
    }

    fn draw_toasts(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        self.toasts.retain(|t| now - t.born < if t.ok { 4.0 } else { 8.0 });
        if self.toasts.is_empty() {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        let p = self.pal;
        let mut dismiss = None;
        egui::Area::new(Id::new("toasts")).anchor(Align2::RIGHT_BOTTOM, vec2(-18.0, -18.0)).order(egui::Order::Foreground).show(ctx, |ui| {
            ui.set_max_width(400.0);
            for (i, t) in self.toasts.iter().enumerate() {
                Frame::new()
                    .fill(p.panel)
                    .stroke(Stroke::new(1.0, p.border_strong))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::symmetric(12, 10))
                    .shadow(egui::Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(90) })
                    .show(ui, |ui| {
                        ui.set_width(376.0);
                        ui.horizontal_top(|ui| {
                            let (glyph, color) = if t.ok { (icon::CHECK_CIRCLE, p.ok) } else { (icon::WARNING_CIRCLE, p.danger) };
                            ui.label(ui::icon_text(glyph, 17.0, color));
                            ui.add(egui::Label::new(ui::text(&t.text, 12.5, p.text)).wrap());
                            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                                if ui::icon_button(ui, icon::X, "Закрыть", true).clicked() {
                                    dismiss = Some(i);
                                }
                            });
                        });
                    });
                ui.add_space(6.0);
            }
        });
        if let Some(i) = dismiss {
            self.toasts.remove(i);
        }
    }
}
