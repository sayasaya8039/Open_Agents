//! アプリ全体の色定数と色ユーティリティ（Figma Design Colors / VS Code Dark / macOS style）

use gpui::*;

pub const BG: u32 = 0x1c1c1e; // macOS systemBackground
pub const SIDEBAR_BG: u32 = 0x2c2c2e; // macOS secondarySystemBackground
pub const TITLEBAR_BG: u32 = 0x2d2d2d;
pub const TITLEBAR_GRADIENT_TOP: u32 = 0x4a4a4c;
pub const TITLEBAR_GRADIENT_BOTTOM: u32 = 0x2f2f31;
pub const BORDER: u32 = 0x48484a; // macOS separator
pub const HOVER_BG: u32 = 0x363638; // adjusted for Sonoma palette
pub const PANEL_BG: u32 = 0x3a3a3c; // macOS tertiarySystemBackground
pub const TEXT_PRIMARY: u32 = 0xffffff; // macOS label
pub const TEXT_SECONDARY: u32 = 0xebebf5; // macOS secondaryLabel
pub const TEXT_MUTED: u32 = 0x6e6e73; // macOS tertiaryLabel
pub const TEXT_DIM: u32 = 0x545458; // macOS quaternaryLabel
pub const ACCENT_BLUE: u32 = 0x0a84ff; // macOS systemBlue
pub const ACCENT_ORANGE: u32 = 0xfb923c;
#[allow(dead_code)]
pub const ACCENT_PINK: u32 = 0xdb2777;
pub const STATUSBAR_BG: u32 = 0x007acc;
/// エクスプローラで選択中の行（Zed の list selection に近い色）
pub const EXPLORER_SELECTION_BG: u32 = 0x2a2d2e;
pub const TRAFFIC_RED: u32 = 0xff5f57;
pub const TRAFFIC_YELLOW: u32 = 0xfebc2e;
pub const TRAFFIC_GREEN: u32 = 0x28c840;
#[allow(dead_code)]
pub const PURPLE: u32 = 0xa855f7;
/// Figma Make（MacOS-style UI）セクション見出し用アイコン色
pub const FIGMA_ICON_ORANGE: u32 = 0xf97316;
pub const FIGMA_ICON_BLUE: u32 = 0x3b82f6;
pub const FIGMA_ICON_GREEN: u32 = 0x22c55e;
/// コントロール背景（Figma `bg-[#3d3d3d]` と揃え、既存 BORDER と同値）
pub const CONTROL_BG: u32 = 0x48484a; // aligned with new BORDER
pub const CONTROL_BORDER: u32 = 0x545458; // aligned with new TEXT_DIM

// ── macOS Sonoma Type Scale ──────────────────────────────
pub const TYPE_CAPTION2: f32 = 11.0;
pub const TYPE_CAPTION1: f32 = 12.0;
pub const TYPE_BODY: f32 = 13.0;
pub const TYPE_HEADLINE: f32 = 15.0;
pub const TYPE_TITLE3: f32 = 17.0;

pub fn hex(c: u32) -> Hsla {
    let r = ((c >> 16) & 0xFF) as f32 / 255.0;
    let g = ((c >> 8) & 0xFF) as f32 / 255.0;
    let b = (c & 0xFF) as f32 / 255.0;
    Rgba { r, g, b, a: 1.0 }.into()
}

pub fn hex_a(c: u32, a: f32) -> Hsla {
    let r = ((c >> 16) & 0xFF) as f32 / 255.0;
    let g = ((c >> 8) & 0xFF) as f32 / 255.0;
    let b = (c & 0xFF) as f32 / 255.0;
    Rgba { r, g, b, a }.into()
}
