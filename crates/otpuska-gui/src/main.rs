// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod launcher;
mod platform;
#[cfg(test)]
mod screenshots;
#[cfg(test)]
mod tests;
mod theme;
mod ui;
mod views;

use otpuska_core::config::Config;
use std::sync::Arc;

/// When the process started: `OTPUSKA_TIMING=1` prints how long the start takes (for diagnosing slow machines).
pub static STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

pub fn timing_enabled() -> bool {
    std::env::var_os("OTPUSKA_TIMING").is_some_and(|v| !v.is_empty())
}

fn main() -> eframe::Result {
    STARTED.get_or_init(std::time::Instant::now);
    let mut config = Config::load();
    // First start of a newer version: the database of the older one joins the list, in place.
    if config.migrate() {
        if let Err(e) = config.save() {
            eprintln!("otpuska: не удалось сохранить настройки: {e:#}");
        }
    }
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
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Отпуска")
        .with_app_id("dev.otpuska.desktop")
        .with_inner_size(launcher::WINDOW_SIZE)
        // The real smallest size is set once the screen is known (`launcher::Root::fit_window`).
        .with_min_inner_size([900.0, 540.0]);
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
        // The database is opened inside: straight away when there is one, after a choice otherwise.
        Box::new(move |cc| Ok(Box::new(launcher::Root::new(&cc.egui_ctx, config)))),
    )
}

/// A fatal startup error shown in a native dialog (the window never opens).
fn fatal(message: &str) {
    eprintln!("otpuska: {message}");
    rfd::MessageDialog::new().set_level(rfd::MessageLevel::Error).set_title("Отпуска").set_description(message).show();
}
