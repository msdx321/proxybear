use std::rc::Rc;

use gpui::{App, SharedString};
use gpui_component::Theme;

pub fn init(cx: &mut App) {
    let theme = Theme::global_mut(cx);
    for config in [&mut theme.light_theme, &mut theme.dark_theme] {
        let config = Rc::make_mut(config);
        let color = |dark: &'static str, light: &'static str| {
            Some(SharedString::from(if config.mode.is_dark() {
                dark
            } else {
                light
            }))
        };
        let colors = &mut config.colors;
        colors.background = color("#1B1B1F", "#F8F9FB");
        colors.foreground = color("#F4F4F5", "#18181B");
        colors.muted = color("#141416", "#F0F1F4");
        colors.muted_foreground = color("#A1A1AA", "#71717A");
        colors.secondary = color("#27272C", "#FFFFFF");
        colors.secondary_foreground = color("#F4F4F5", "#18181B");
        colors.secondary_hover = color("#34343B", "#F4F4F5");
        colors.border = color("#36363E", "#E4E4E7");
        colors.input = color("#45454F", "#D4D4D8");
        colors.button = color("#27272C", "#E8E9ED");
        colors.button_hover = color("#34343B", "#DDDFE4");
        colors.button_active = color("#3F3F47", "#D1D4DB");
        colors.primary = color("#2563EB", "#2563EB");
        colors.primary_foreground = color("#FFFFFF", "#FFFFFF");
        colors.primary_hover = color("#3B82F6", "#1D4ED8");
        colors.primary_active = color("#1D4ED8", "#1E40AF");
        colors.ring = color("#60A5FA", "#3B82F6");
        colors.caret = color("#93C5FD", "#2563EB");
        colors.accent = color("#263550", "#EFF6FF");
        colors.accent_foreground = color("#BFDBFE", "#1D4ED8");
        colors.success = color("#4ADE80", "#15803D");
        colors.warning = color("#FBBF24", "#B45309");
    }
    Theme::sync_system_appearance(None, cx);
}
