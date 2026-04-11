//! Settings UI — 共通ヘルパー、タイトルバー、設定ページ構成、ターミナルビュー。
//!
//! 個別のセクション実装は以下のモジュールに分離:
//! - settings_general: 外観・AI トグル・推論モード
//! - settings_model: モデル選択・パラメータ・チャット推論
//! - settings_llama: llama.cpp ランタイム・ハードウェア
//! - settings_api: API キー管理・API サーバー
//! - settings_hf: Hugging Face Discover ハンドラ

use gpui::prelude::FluentBuilder;
use gpui::*;
use std::path::PathBuf;

use crate::api_key_prefs;
use crate::colors::*;
use crate::i18n;
use crate::model_prefs;

use super::{AppView, AiToggleKind, HardwareParamAdjustKind, ModelFormat, ModelParamAdjustKind, Page};

impl AppView {
    // ============================================================
    // モデル形式ヘルパー
    // ============================================================

    /// 形式ドロップダウン用（末尾＝直近追加のモデル）
    pub(crate) fn settings_last_model_path(&self) -> Option<&std::path::Path> {
        self.settings_model_paths.last().map(|p| p.as_path())
    }

    pub(crate) fn settings_detected_model_format(&self) -> Option<ModelFormat> {
        self.settings_last_model_path().map(ModelFormat::from_path)
    }

    pub(crate) fn settings_model_format_label(&self) -> SharedString {
        self.settings_detected_model_format()
            .map(|format| format.label().into())
            .unwrap_or_else(|| "自動判定".into())
    }

    pub(crate) fn settings_model_format_hint(&self) -> SharedString {
        if self.settings_model_paths.is_empty() {
            return "モデルを追加すると一覧に残ります（起動後も維持）".into();
        }
        if self.settings_model_paths.len() > 1 {
            return "末尾（直近追加）のファイル形式を表示しています".into();
        }
        match self.settings_detected_model_format() {
            Some(ModelFormat::Gguf) => "GGUF を自動判定しました".into(),
            Some(ModelFormat::Onnx) => "ONNX を自動判定しました".into(),
            Some(ModelFormat::Unknown) => "GGUF / ONNX を判定できませんでした".into(),
            None => "選択したモデルファイルから自動判定します".into(),
        }
    }

    // ============================================================
    // 共通ヘルパー
    // ============================================================

    pub(crate) fn settings_figma_heading(&self, icon: &str, icon_color: u32, title: &str) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .mb(px(16.))
            .child(
                div()
                    .text_size(px(18.))
                    .text_color(hex(icon_color))
                    .child(icon.to_string()),
            )
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(hex(TEXT_PRIMARY))
                    .child(title.to_string()),
            )
    }

    pub(crate) fn settings_fake_dropdown(&self, label: &str) -> impl IntoElement {
        div()
            .px(px(12.))
            .py(px(6.))
            .min_w(px(180.))
            .bg(hex(CONTROL_BG))
            .border_1()
            .border_color(hex(CONTROL_BORDER))
            .rounded(px(6.))
            .text_size(px(12.))
            .text_color(hex(TEXT_SECONDARY))
            .child(label.to_string())
    }

    pub(crate) fn settings_labeled_block(&self, title: &str, subtitle: &str) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(hex(TEXT_PRIMARY))
                    .child(title.to_string()),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(hex(TEXT_MUTED))
                    .child(subtitle.to_string()),
            )
    }

    pub(crate) fn persist_local_llm_prefs(&self) {
        model_prefs::save_local_llm_prefs(&model_prefs::LocalLlmPrefs {
            model: self.model_params.clone(),
            hardware: self.hardware_params.clone(),
            appearance: self.appearance_prefs.clone(),
            ai: self.ai_prefs.clone(),
            model_paths: self.settings_model_paths.clone(),
            chat: self.chat_prefs.clone(),
            api_server: self.api_server_prefs.clone(),
        });
    }

    // ============================================================
    // Title Bar
    // ============================================================

    pub(crate) fn render_titlebar(&self) -> impl IntoElement {
        div()
            .h(px(44.))
            .bg(linear_gradient(
                180.0,
                linear_color_stop(hex_a(TITLEBAR_GRADIENT_TOP, 0.85), 0.0),
                linear_color_stop(hex_a(TITLEBAR_GRADIENT_BOTTOM, 0.94), 1.0),
            ))
            .border_b_1()
            .border_color(hex_a(0xffffff, 0.03))
            .child(
                canvas(
                    |bounds, window, _cx| {
                        let drag_bounds = Bounds {
                            origin: point(bounds.origin.x + px(80.), bounds.origin.y),
                            size: size(bounds.size.width - px(80.), bounds.size.height),
                        };
                        let drag_hitbox = window.insert_hitbox(drag_bounds, HitboxBehavior::Normal);

                        let close_bounds = Bounds {
                            origin: point(bounds.origin.x + px(16.), bounds.origin.y + px(16.)),
                            size: size(px(12.), px(12.)),
                        };
                        let close_hitbox =
                            window.insert_hitbox(close_bounds, HitboxBehavior::Normal);

                        let min_bounds = Bounds {
                            origin: point(bounds.origin.x + px(36.), bounds.origin.y + px(16.)),
                            size: size(px(12.), px(12.)),
                        };
                        let min_hitbox = window.insert_hitbox(min_bounds, HitboxBehavior::Normal);

                        let max_bounds = Bounds {
                            origin: point(bounds.origin.x + px(56.), bounds.origin.y + px(16.)),
                            size: size(px(12.), px(12.)),
                        };
                        let max_hitbox = window.insert_hitbox(max_bounds, HitboxBehavior::Normal);

                        (drag_hitbox, close_hitbox, min_hitbox, max_hitbox)
                    },
                    |bounds, (drag_hb, close_hb, min_hb, max_hb), window, _cx| {
                        window.insert_window_control_hitbox(WindowControlArea::Drag, drag_hb);
                        window.insert_window_control_hitbox(WindowControlArea::Close, close_hb);
                        window.insert_window_control_hitbox(WindowControlArea::Min, min_hb);
                        window.insert_window_control_hitbox(WindowControlArea::Max, max_hb);

                        let btn_y = bounds.origin.y + px(16.);
                        let btn_r = px(6.);

                        let close_center = point(bounds.origin.x + px(22.), btn_y + btn_r);
                        window.paint_quad(
                            fill(
                                Bounds::centered_at(close_center, size(px(12.), px(12.))),
                                hex(TRAFFIC_RED),
                            )
                            .corner_radii(px(6.)),
                        );

                        let min_center = point(bounds.origin.x + px(42.), btn_y + btn_r);
                        window.paint_quad(
                            fill(
                                Bounds::centered_at(min_center, size(px(12.), px(12.))),
                                hex(TRAFFIC_YELLOW),
                            )
                            .corner_radii(px(6.)),
                        );

                        let max_center = point(bounds.origin.x + px(62.), btn_y + btn_r);
                        window.paint_quad(
                            fill(
                                Bounds::centered_at(max_center, size(px(12.), px(12.))),
                                hex(TRAFFIC_GREEN),
                            )
                            .corner_radii(px(6.)),
                        );
                    },
                )
                .size_full(),
            )
            .child(
                div()
                    .absolute()
                    .top(px(0.))
                    .left(px(80.))
                    .right(px(60.))
                    .h(px(44.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(13.))
                    .text_color(hex(TEXT_SECONDARY))
                    .child("AI Coding Assistant"),
            )
    }

    // ============================================================
    // Settings View（Figma Make: SettingsView.tsx — 単一スクロール + セクションカード）
    // ============================================================

    pub(crate) fn render_settings(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let ver = env!("CARGO_PKG_VERSION");
        div()
            .flex_1()
            .flex()
            .flex_col()
            .min_h(px(0.))
            .min_w(px(0.))
            .bg(hex(BG))
            .child(
                div()
                    .flex_shrink_0()
                    .h(px(48.))
                    .bg(hex(PANEL_BG))
                    .border_b_1()
                    .border_color(hex(BORDER))
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(16.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .px(px(8.))
                                    .py(px(4.))
                                    .rounded(px(6.))
                                    .text_size(px(12.))
                                    .text_color(hex(TEXT_SECONDARY))
                                    .cursor_pointer()
                                    .hover(|d| d.bg(hex(HOVER_BG)))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                            this.page = Page::Chat;
                                            cx.notify();
                                        }),
                                    )
                                    .child("← Chat"),
                            )
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .text_color(hex(TEXT_SECONDARY))
                                    .child("⚙"),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(hex(TEXT_PRIMARY))
                                    .child(i18n::settings()),
                            ),
                    ),
            )
            .child(
                div()
                    .id("settings-figma-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .min_w(px(0.))
                    .overflow_x_hidden()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .w_full()
                            .min_w(px(0.))
                            .max_w(px(768.))
                            .mx_auto()
                            .p(px(24.))
                            .flex()
                            .flex_col()
                            .gap(px(32.))
                            // --- ローカル LLM ---
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(self.settings_figma_heading(
                                        "💾",
                                        FIGMA_ICON_ORANGE,
                                        i18n::settings_local_llm(),
                                    ))
                                    .child(
                                        div()
                                            .bg(hex(PANEL_BG))
                                            .rounded(px(8.))
                                            .p(px(16.))
                                            .flex()
                                            .flex_col()
                                            .gap(px(16.))
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .gap(px(16.))
                                                    .child(self.settings_labeled_block(
                                                        i18n::settings_model_format(),
                                                        i18n::settings_model_format_hint(),
                                                    ))
                                                    .child(self.settings_fake_dropdown(
                                                        self.settings_model_format_label().as_ref(),
                                                    )),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(8.))
                                                    .child(
                                                        div()
                                                            .text_size(px(13.))
                                                            .text_color(hex(TEXT_PRIMARY))
                                                            .child(i18n::settings_model_file()),
                                                    )
                                                    .child(
                                                        div()
                                                            .w_full()
                                                            .flex()
                                                            .items_center()
                                                            .justify_center()
                                                            .gap(px(8.))
                                                            .px(px(16.))
                                                            .py(px(12.))
                                                            .bg(hex(CONTROL_BG))
                                                            .border_1()
                                                            .border_color(hex(CONTROL_BORDER))
                                                            .rounded(px(8.))
                                                            .text_size(px(12.))
                                                            .text_color(hex(TEXT_SECONDARY))
                                                            .cursor_pointer()
                                                            .on_mouse_down(
                                                                MouseButton::Left,
                                                                cx.listener(
                                                                    |this: &mut AppView,
                                                                     _: &MouseDownEvent,
                                                                     _,
                                                                     cx: &mut Context<AppView>| {
                                                                        this.settings_open_model_file_dialog(cx);
                                                                    },
                                                                ),
                                                            )
                                                            .child("⬆")
                                                            .child(i18n::settings_load_model()),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(11.))
                                                            .text_color(hex(TEXT_DIM))
                                                            .child(self.settings_model_format_hint()),
                                                    ),
                                            )
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .flex_col()
                                                            .gap(px(8.))
                                                            .child(
                                                                div()
                                                                    .text_size(px(13.))
                                                                    .text_color(hex(TEXT_PRIMARY))
                                                                    .child(i18n::settings_loaded_models()),
                                                            )
                                                            .when(self.settings_model_paths.is_empty(), |d| {
                                                                d.child(
                                                                    div()
                                                                        .text_size(px(11.))
                                                                        .text_color(hex(TEXT_DIM))
                                                                        .child(
                                                                            i18n::settings_loaded_models_hint(),
                                                                        ),
                                                                )
                                                            })
                                                            .children({
                                                                let rows: Vec<(usize, PathBuf)> = self
                                                                    .settings_model_paths
                                                                    .iter()
                                                                    .enumerate()
                                                                    .map(|(i, p)| (i, p.clone()))
                                                                    .collect();
                                                                rows.into_iter().map(|(idx, path)| {
                                                                    self.settings_loaded_model_row(
                                                                        cx, idx, &path,
                                                                    )
                                                                })
                                                            }),
                                                    )
                                            .child(
                                                div()
                                                    .pt(px(16.))
                                                    .border_t_1()
                                                    .border_color(hex(BORDER))
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(16.))
                                                    .child(
                                                        div()
                                                            .text_size(px(13.))
                                                            .text_color(hex(TEXT_PRIMARY))
                                                            .child(i18n::settings_model_params()),
                                                    )
                                                    .child(self.settings_model_param_row(
                                                        cx,
                                                        ModelParamAdjustKind::Temperature,
                                                        "Temperature",
                                                        Some(i18n::settings_temperature_hint()),
                                                    ))
                                                    .child(self.settings_model_param_row(
                                                        cx,
                                                        ModelParamAdjustKind::MaxOutputTokens,
                                                        i18n::settings_max_tokens(),
                                                        Some(i18n::settings_max_tokens_hint()),
                                                    ))
                                                    .child(self.settings_model_param_row(
                                                        cx,
                                                        ModelParamAdjustKind::ContextLength,
                                                        i18n::settings_context_length(),
                                                        None,
                                                    )),
                                            )
                                            .child(
                                                div()
                                                    .pt(px(16.))
                                                    .border_t_1()
                                                    .border_color(hex(BORDER))
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(16.))
                                                    .child(
                                                        div()
                                                            .text_size(px(13.))
                                                            .text_color(hex(TEXT_PRIMARY))
                                                            .child(i18n::settings_hardware()),
                                                    )
                                                    .child(self.settings_runtime_preset_row(cx))
                                                    .child(self.settings_hardware_stepper_row(
                                                        cx,
                                                        HardwareParamAdjustKind::GpuLayers,
                                                        i18n::settings_gpu_layers(),
                                                        Some(
                                                            i18n::settings_gpu_layers_hint(),
                                                        ),
                                                    ))
                                                    .child(self.settings_hardware_stepper_row(
                                                        cx,
                                                        HardwareParamAdjustKind::NThreads,
                                                        i18n::settings_threads(),
                                                        None,
                                                    ))
                                                    .child(self.settings_hardware_stepper_row(
                                                        cx,
                                                        HardwareParamAdjustKind::BatchSize,
                                                        i18n::settings_batch_size(),
                                                        None,
                                                    )),
                                            ),
                                    ),
                            )
                            // --- 外観 ---
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(self.settings_figma_heading(
                                        "🎨",
                                        FIGMA_ICON_BLUE,
                                        i18n::settings_appearance(),
                                    ))
                                    .child(
                                        div()
                                            .bg(hex(PANEL_BG))
                                            .rounded(px(8.))
                                            .p(px(16.))
                                    .flex()
                                    .flex_col()
                                    .gap(px(16.))
                                    .child(self.settings_appearance_theme_row(cx))
                                    .child(self.settings_appearance_font_row(cx))
                                    .child(self.settings_appearance_line_numbers_row(cx)),
                                    ),
                            )
                            // --- AI 設定 ---
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(self.settings_figma_heading(
                                        "🧠",
                                        PURPLE,
                                        i18n::settings_ai(),
                                    ))
                                    .child(
                                        div()
                                            .bg(hex(PANEL_BG))
                                            .rounded(px(8.))
                                            .p(px(16.))
                                            .flex()
                                            .flex_col()
                                            .gap(px(16.))
                                            .child(self.settings_ai_toggle_row(
                                                cx,
                                                AiToggleKind::AutoComplete,
                                                i18n::settings_auto_complete(),
                                                i18n::settings_auto_complete_hint(),
                                            ))
                                            .child(self.settings_ai_toggle_row(
                                                cx,
                                                AiToggleKind::CodeSuggestions,
                                                i18n::settings_code_suggestions(),
                                                i18n::settings_code_suggestions_hint(),
                                            ))
                                            .child(self.settings_ai_toggle_row(
                                                cx,
                                                AiToggleKind::StreamingResponses,
                                                i18n::settings_streaming(),
                                                i18n::settings_streaming_hint(),
                                            ))
                                            .child(self.settings_cot_mode_row(cx))
                                            .child(self.settings_self_consistency_row(cx))
                                            .child(self.settings_tot_mode_row(cx))
                                            .child(self.settings_react_mode_row(cx))
                                            .child(self.settings_chat_inference_block(cx)),
                                    ),
                            )
                            // --- API キー ---
                            .child({
                                let extra_keys =
                                    api_key_prefs::extra_entry_count(&self.api_keys);
                                div()
                                    .flex()
                                    .flex_col()
                                    .min_w(px(0.))
                                    .child(self.settings_figma_heading(
                                        "🔑",
                                        FIGMA_ICON_GREEN,
                                        i18n::settings_api_keys(),
                                    ))
                                    .child(
                                        div()
                                            .min_w(px(0.))
                                            .bg(hex(PANEL_BG))
                                            .rounded(px(8.))
                                            .p(px(16.))
                                            .flex()
                                            .flex_col()
                                            .gap(px(16.))
                                            .child(
                                                div()
                                                    .min_w(px(0.))
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(6.))
                                                    .child(
                                                        div()
                                                            .min_w(px(0.))
                                                            .whitespace_normal()
                                                            .text_size(px(12.))
                                                            .text_color(hex(TEXT_MUTED))
                                                            .child(i18n::settings_api_keys_description()),
                                                    )
                                                    .when(extra_keys > 0, |d| {
                                                        d.child(
                                                            div()
                                                                .min_w(px(0.))
                                                                .whitespace_normal()
                                                                .text_size(px(11.))
                                                                .text_color(hex(ACCENT_ORANGE))
                                                                .child(i18n::settings_extra_entries(extra_keys)),
                                                        )
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .min_w(px(0.))
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(8.))
                                                    .child(
                                                        div()
                                                            .text_size(px(13.))
                                                            .text_color(hex(TEXT_PRIMARY))
                                                            .child(i18n::settings_registered_catalog()),
                                                    )
                                                    .children(
                                                        self.settings_api_keys_child_elements(cx),
                                                    )
                                            )
                                    )
                            })
                            // --- API サーバー ---
                            .child(self.settings_api_server_section(cx))
                            // --- アプリ情報 ---
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(self.settings_figma_heading(
                                        "ℹ",
                                        TEXT_SECONDARY,
                                        i18n::settings_app_info(),
                                    ))
                                    .child(
                                        div()
                                            .bg(hex(PANEL_BG))
                                            .rounded(px(8.))
                                            .p(px(16.))
                                            .flex()
                                            .flex_col()
                                            .gap(px(8.))
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .child(
                                                        div()
                                                            .text_size(px(12.))
                                                            .text_color(hex(TEXT_MUTED))
                                                            .child(i18n::settings_version()),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(12.))
                                                            .text_color(hex(TEXT_PRIMARY))
                                                            .child(ver),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .child(
                                                        div()
                                                            .text_size(px(12.))
                                                            .text_color(hex(TEXT_MUTED))
                                                            .child(i18n::settings_build()),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(px(12.))
                                                            .text_color(hex(TEXT_PRIMARY))
                                                            .child("2026.04.05"),
                                                    ),
                                            ),
                                    ),
                            ),
                    ),
            )
    }

    // ============================================================
    // Terminal View
    // ============================================================

    pub(crate) fn render_terminal(&self) -> impl IntoElement {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .bg(hex(BG))
            .child(
                div()
                    .h(px(44.))
                    .bg(hex(TITLEBAR_BG))
                    .border_b_1()
                    .border_color(hex(BORDER))
                    .flex()
                    .items_center()
                    .px(px(16.))
                    .gap(px(8.))
                    .child(div().text_size(px(13.)).child("▶"))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(hex(TEXT_SECONDARY))
                            .child("Terminal"),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .p(px(16.))
                    .font_family("Cascadia Code")
                    .text_size(px(12.))
                    .text_color(hex(TEXT_PRIMARY))
                    .overflow_hidden()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child("Open Agents Terminal v1.0.0")
                            .child("Type 'help' for available commands")
                            .child("")
                            .child(
                                div()
                                    .flex()
                                    .gap(px(8.))
                                    .child(div().text_color(hex(TRAFFIC_GREEN)).child("$"))
                                    .child("open_agents --gpus"),
                            )
                            .child(
                                div()
                                    .text_color(hex(TRAFFIC_GREEN))
                                    .child("GPU detected: NVIDIA RTX 4090 (15.7 GB)"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap(px(8.))
                                    .child(div().text_color(hex(TRAFFIC_GREEN)).child("$"))
                                    .child("_"),
                            ),
                    ),
            )
    }
}
