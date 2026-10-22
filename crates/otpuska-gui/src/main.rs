// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod platform;
#[cfg(test)]
mod screenshots;
#[cfg(test)]
mod tests;
mod theme;
mod ui;
mod views;

use otpuska_core::config::{self, Config};
use otpuska_core::{date, Db};
use std::sync::Arc;

fn main() -> eframe::Result {
    let config = Config::load();
    // First the default graphics backend (Metal / DirectX 12); if the window cannot be created
    // (old driver, remote desktop), once more through OpenGL before giving up.
    if let Err(first) = run(&config) {
        eprintln!("otpuska: {first}");
        std::env::set_var("WGPU_BACKEND", "gl");
        if let Err(second) = run(&config) {
            fatal(&format!(
                "Не удалось открыть окно программы.\nОбновите драйвер видеокарты и запустите снова.\n\n{first}\n{second}"
            ));
        }
    }
    Ok(())
}

fn run(config: &Config) -> eframe::Result {
    let db_path = config.db_path();
    let db = match Db::open(&db_path) {
        Ok(db) => db,
        Err(e) => {
            fatal(&format!("Не удалось открыть базу данных\n{}\n\n{e:#}", db_path.display()));
            return Ok(());
        }
    };
    // One backup per day, the last two weeks are kept.
    if let Err(e) = config::auto_backup(&db, date::today(), 14) {
        eprintln!("otpuska: автокопия не создана: {e:#}");
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Отпуска")
        .with_app_id("dev.otpuska.desktop")
        .with_inner_size([1360.0, 860.0])
        .with_min_inner_size([1180.0, 640.0]);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")) {
        viewport = viewport.with_icon(Arc::new(icon));
    }
    if cfg!(target_os = "macos") {
        // Content under a transparent title bar, traffic lights over the sidebar.
        viewport = viewport.with_fullsize_content_view(true).with_titlebar_shown(false).with_title_shown(false);
    }

    // A light app: prefer the integrated GPU (no waking a discrete card, less battery drain).
    let mut wgpu_options = eframe::egui_wgpu::WgpuConfiguration::default();
    if std::env::var_os("WGPU_POWER_PREF").is_none() {
        if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut wgpu_options.wgpu_setup {
            setup.power_preference = eframe::egui_wgpu::wgpu::PowerPreference::LowPower;
        }
    }
    let options = eframe::NativeOptions { viewport, wgpu_options, ..Default::default() };
    let config = config.clone();
    eframe::run_native(
        "Отпуска",
        options,
        Box::new(move |cc| match app::App::new(&cc.egui_ctx, db, config) {
            Ok(app) => Ok(Box::new(app)),
            Err(e) => Err(format!("{e:#}").into()),
        }),
    )
}

/// A fatal startup error shown in a native dialog (the window never opens).
fn fatal(message: &str) {
    eprintln!("otpuska: {message}");
    rfd::MessageDialog::new().set_level(rfd::MessageLevel::Error).set_title("Отпуска").set_description(message).show();
}
