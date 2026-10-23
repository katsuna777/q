//! Colors, fonts and egui style: a neutral black dark theme, a clean light theme
//! and five accent colors.

use egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Id, Shadow, Stroke, TextStyle, Vec2,
};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pal {
    pub dark: bool,
    pub bg: Color32,
    pub sidebar: Color32,
    pub panel: Color32,
    pub sunken: Color32,
    pub hover: Color32,
    pub active: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub text: Color32,
    pub text2: Color32,
    pub text3: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub on_accent: Color32,
    pub ok: Color32,
    pub warn: Color32,
    pub danger: Color32,
    /// Leave states that need their own color: "burning", "taken in advance",
    /// "suspended", "check the books".
    pub orange: Color32,
    pub info: Color32,
    pub violet: Color32,
    pub salmon: Color32,
}

impl Pal {
    pub fn soft(c: Color32) -> Color32 {
        c.gamma_multiply(0.14)
    }
}

const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

pub const ACCENTS: [(&str, &str); 5] =
    [("mono", "Моно"), ("blue", "Синий"), ("violet", "Фиолетовый"), ("green", "Зелёный"), ("orange", "Оранжевый")];

pub fn palette(dark: bool, accent: &str) -> Pal {
    let (accent, accent_hover, on_accent) = match (accent, dark) {
        ("blue", true) => (hex(0x5b8cff), hex(0x7aa2ff), hex(0xffffff)),
        ("blue", false) => (hex(0x2563eb), hex(0x1d4ed8), hex(0xffffff)),
        ("violet", true) => (hex(0x8b7bff), hex(0xa294ff), hex(0xffffff)),
        ("violet", false) => (hex(0x6d4cff), hex(0x5b3bf0), hex(0xffffff)),
        ("green", true) => (hex(0x3ecf8e), hex(0x5ddba1), hex(0x04130b)),
        ("green", false) => (hex(0x16a34a), hex(0x15803d), hex(0xffffff)),
        ("orange", true) => (hex(0xff8a4c), hex(0xffa06d), hex(0x1c0a00)),
        ("orange", false) => (hex(0xea580c), hex(0xc2410c), hex(0xffffff)),
        (_, true) => (hex(0xf2f2f2), hex(0xffffff), hex(0x0a0a0a)),
        (_, false) => (hex(0x111111), hex(0x000000), hex(0xffffff)),
    };
    if dark {
        Pal {
            dark,
            bg: hex(0x0a0a0a),
            sidebar: hex(0x0d0d0d),
            panel: hex(0x141414),
            sunken: hex(0x0f0f0f),
            hover: Color32::from_white_alpha(12),
            active: Color32::from_white_alpha(22),
            border: hex(0x1f1f1f),
            border_strong: hex(0x2e2e2e),
            text: hex(0xededed),
            text2: hex(0xa1a1a1),
            text3: hex(0x6b6b6b),
            accent,
            accent_hover,
            on_accent,
            ok: hex(0x3ecf8e),
            warn: hex(0xf2b84b),
            danger: hex(0xf26b7f),
            orange: hex(0xff8a4c),
            info: hex(0x6b9bff),
            violet: hex(0xa78bfa),
            salmon: hex(0xe8957a),
        }
    } else {
        Pal {
            dark,
            bg: hex(0xf7f7f8),
            sidebar: hex(0xefeff1),
            panel: hex(0xffffff),
            sunken: hex(0xf2f2f4),
            hover: Color32::from_black_alpha(10),
            active: Color32::from_black_alpha(18),
            border: hex(0xe4e4e7),
            border_strong: hex(0xd4d4d8),
            text: hex(0x111113),
            text2: hex(0x52525b),
            text3: hex(0x8e8e96),
            accent,
            accent_hover,
            on_accent,
            ok: hex(0x15935a),
            warn: hex(0xb7791f),
            danger: hex(0xd6455d),
            orange: hex(0xd9600c),
            info: hex(0x2563eb),
            violet: hex(0x7c3aed),
            salmon: hex(0xc2542d),
        }
    }
}

pub fn medium() -> FontFamily {
    FontFamily::Name("medium".into())
}

pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

pub fn icons() -> FontFamily {
    FontFamily::Name("icons".into())
}

/// Once per window: the chooser and the working screen both ask for it.
pub fn install_fonts(ctx: &Context) {
    let done = Id::new("otpuska-fonts");
    if ctx.data(|d| d.get_temp::<bool>(done)).unwrap_or(false) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(done, true));
    let mut fonts = FontDefinitions::default();
    let font = |bytes: &'static [u8]| Arc::new(FontData::from_static(bytes));
    fonts.font_data.insert("inter".into(), font(include_bytes!("../assets/fonts/Inter-Regular.ttf")));
    fonts.font_data.insert("inter-medium".into(), font(include_bytes!("../assets/fonts/Inter-Medium.ttf")));
    fonts.font_data.insert("inter-semibold".into(), font(include_bytes!("../assets/fonts/Inter-SemiBold.ttf")));

    let fallbacks: Vec<String> = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    // Icons live in their own family: Phosphor shares Private Use Area codepoints with
    // Inter, and maps Latin letters to empty glyphs, so it must never be a text fallback.
    egui_phosphor::add_font_bytes_as_family(&mut fonts, "icons", egui_phosphor::Variant::Regular.font_bytes());
    for (family, face) in [
        (FontFamily::Proportional, "inter"),
        (FontFamily::Name("medium".into()), "inter-medium"),
        (FontFamily::Name("semibold".into()), "inter-semibold"),
    ] {
        let mut list = vec![face.to_string()];
        list.extend(fallbacks.iter().cloned());
        fonts.families.insert(family, list);
    }
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &Context, pal: &Pal, reduce_motion: bool) {
    ctx.set_theme(if pal.dark { egui::Theme::Dark } else { egui::Theme::Light });
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::new(11.5, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(13.5, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(13.0, medium())),
            (TextStyle::Heading, FontId::new(22.0, semibold())),
            (TextStyle::Monospace, FontId::new(12.5, FontFamily::Monospace)),
        ]
        .into();
        style.animation_time = if reduce_motion { 0.0 } else { 0.14 };
        style.spacing.item_spacing = Vec2::new(8.0, 8.0);
        style.spacing.button_padding = Vec2::new(12.0, 6.0);
        style.spacing.interact_size = Vec2::new(24.0, 30.0);
        style.spacing.combo_height = 320.0;
        style.spacing.menu_margin = egui::Margin::same(6);

        let v = &mut style.visuals;
        *v = if pal.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        v.panel_fill = pal.bg;
        v.window_fill = pal.panel;
        v.window_stroke = Stroke::new(1.0, pal.border);
        v.window_corner_radius = CornerRadius::same(12);
        v.menu_corner_radius = CornerRadius::same(10);
        v.extreme_bg_color = pal.sunken;
        v.text_edit_bg_color = Some(pal.panel);
        v.faint_bg_color = pal.sunken;
        v.code_bg_color = pal.sunken;
        v.hyperlink_color = pal.accent;
        v.warn_fg_color = pal.warn;
        v.error_fg_color = pal.danger;
        v.selection.bg_fill = pal.accent.gamma_multiply(0.32);
        v.selection.stroke = Stroke::new(1.0, pal.accent);
        let shadow_alpha = if pal.dark { 140 } else { 40 };
        v.window_shadow =
            Shadow { offset: [0, 12], blur: 32, spread: 0, color: Color32::from_black_alpha(shadow_alpha) };
        v.popup_shadow = Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(shadow_alpha) };

        let radius = CornerRadius::same(7);
        let w = &mut v.widgets;
        w.noninteractive.bg_fill = pal.panel;
        w.noninteractive.weak_bg_fill = pal.panel;
        w.noninteractive.bg_stroke = Stroke::new(1.0, pal.border);
        w.noninteractive.fg_stroke = Stroke::new(1.0, pal.text2);
        w.noninteractive.corner_radius = radius;
        w.inactive.bg_fill = pal.panel;
        w.inactive.weak_bg_fill = pal.panel;
        w.inactive.bg_stroke = Stroke::new(1.0, pal.border_strong);
        w.inactive.fg_stroke = Stroke::new(1.0, pal.text);
        w.inactive.corner_radius = radius;
        w.inactive.expansion = 0.0;
        w.hovered.bg_fill = pal.sunken;
        w.hovered.weak_bg_fill = pal.sunken;
        w.hovered.bg_stroke = Stroke::new(1.0, pal.text3);
        w.hovered.fg_stroke = Stroke::new(1.0, pal.text);
        w.hovered.corner_radius = radius;
        w.hovered.expansion = 0.0;
        w.active.bg_fill = pal.active;
        w.active.weak_bg_fill = pal.active;
        w.active.bg_stroke = Stroke::new(1.0, pal.accent);
        w.active.fg_stroke = Stroke::new(1.0, pal.text);
        w.active.corner_radius = radius;
        w.active.expansion = 0.0;
        w.open = w.hovered;
    });
}
