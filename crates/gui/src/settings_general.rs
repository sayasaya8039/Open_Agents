//! 一般設定: 外観（テーマ・フォント・行番号）、AI トグル、推論モード（CoT / ReAct / ToT / Self-Consistency）。

use gpui::*;

use crate::colors::*;
use crate::model_prefs;

use super::{AppView, AiToggleKind};

impl AppView {
    // ============================================================
    // 外観設定
    // ============================================================

    pub(crate) fn sync_editor_appearance(&self, _cx: &mut Context<Self>) {
        // Editor は削除済み — 外観同期は不要
    }

    pub(crate) fn cycle_appearance_theme(&mut self, cx: &mut Context<Self>) {
        self.appearance_prefs.theme =
            model_prefs::AppearancePrefs::cycle_theme(self.appearance_prefs.theme);
        self.appearance_prefs.clamp();
        self.persist_local_llm_prefs();
        self.sync_editor_appearance(cx);
        cx.notify();
    }

    pub(crate) fn adjust_appearance_font(&mut self, delta: i32, cx: &mut Context<Self>) {
        self.appearance_prefs.font_size_px =
            model_prefs::AppearancePrefs::step_font_size(self.appearance_prefs.font_size_px, delta);
        self.appearance_prefs.clamp();
        self.persist_local_llm_prefs();
        self.sync_editor_appearance(cx);
        cx.notify();
    }

    pub(crate) fn toggle_appearance_line_numbers(&mut self, cx: &mut Context<Self>) {
        self.appearance_prefs.show_line_numbers = !self.appearance_prefs.show_line_numbers;
        self.persist_local_llm_prefs();
        self.sync_editor_appearance(cx);
        cx.notify();
    }

    pub(crate) fn settings_appearance_theme_row(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme_label: SharedString = self.appearance_prefs.theme.label().into();
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .child(self.settings_labeled_block(
                "テーマ",
                "エディタの背景・テキスト配色（自動は当面ダーク相当）",
            ))
            .child(
                div()
                    .px(px(12.))
                    .py(px(6.))
                    .min_w(px(160.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .bg(hex(CONTROL_BG))
                    .border_1()
                    .border_color(hex(CONTROL_BORDER))
                    .text_size(px(TYPE_CAPTION1))
                    .text_color(hex(TEXT_SECONDARY))
                    .text_center()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.cycle_appearance_theme(cx);
                        }),
                    )
                    .child(theme_label),
            )
    }

    pub(crate) fn settings_appearance_font_row(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let value_str: SharedString = format!("{} px", self.appearance_prefs.font_size_px).into();
        div()
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
                            .text_size(px(TYPE_BODY))
                            .text_color(hex(TEXT_PRIMARY))
                            .child("フォントサイズ"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .px(px(10.))
                                    .py(px(4.))
                                    .rounded(px(6.))
                                    .bg(hex(CONTROL_BG))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                            this.adjust_appearance_font(-1, cx);
                                        }),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(TYPE_BODY))
                                            .text_color(hex(TEXT_SECONDARY))
                                            .child("−"),
                                    ),
                            )
                            .child(
                                div()
                                    .min_w(px(52.))
                                    .text_size(px(TYPE_CAPTION1))
                                    .text_color(hex(TEXT_MUTED))
                                    .text_center()
                                    .child(value_str),
                            )
                            .child(
                                div()
                                    .px(px(10.))
                                    .py(px(4.))
                                    .rounded(px(6.))
                                    .bg(hex(CONTROL_BG))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                            this.adjust_appearance_font(1, cx);
                                        }),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(TYPE_BODY))
                                            .text_color(hex(TEXT_SECONDARY))
                                            .child("+"),
                                    ),
                            ),
                    ),
            )
            .child(
                div()
                    .text_size(px(TYPE_CAPTION2))
                    .text_color(hex(TEXT_DIM))
                    .child("12 / 14 / 16 / 18 px から選択"),
            )
    }

    pub(crate) fn settings_appearance_line_numbers_row(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let on = self.appearance_prefs.show_line_numbers;
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .child(self.settings_labeled_block("行番号を表示", "エディタ左端に行番号を表示"))
            .child(
                div()
                    .px(px(10.))
                    .py(px(4.))
                    .rounded(px(9999.))
                    .cursor_pointer()
                    .bg(if on {
                        hex_a(ACCENT_BLUE, 0.35)
                    } else {
                        hex(CONTROL_BG)
                    })
                    .text_size(px(TYPE_CAPTION2))
                    .text_color(if on {
                        hex(TEXT_PRIMARY)
                    } else {
                        hex(TEXT_MUTED)
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.toggle_appearance_line_numbers(cx);
                        }),
                    )
                    .child(if on { "オン" } else { "オフ" }),
            )
    }

    // ============================================================
    // AI トグル設定
    // ============================================================

    pub(crate) fn ai_toggle_value(&self, kind: AiToggleKind) -> bool {
        match kind {
            AiToggleKind::AutoComplete => self.ai_prefs.auto_complete,
            AiToggleKind::CodeSuggestions => self.ai_prefs.code_suggestions,
            AiToggleKind::StreamingResponses => self.ai_prefs.streaming_responses,
        }
    }

    pub(crate) fn toggle_ai_setting(&mut self, kind: AiToggleKind, cx: &mut Context<Self>) {
        match kind {
            AiToggleKind::AutoComplete => {
                self.ai_prefs.auto_complete = !self.ai_prefs.auto_complete;
            }
            AiToggleKind::CodeSuggestions => {
                self.ai_prefs.code_suggestions = !self.ai_prefs.code_suggestions;
            }
            AiToggleKind::StreamingResponses => {
                self.ai_prefs.streaming_responses = !self.ai_prefs.streaming_responses;
            }
        }
        self.persist_local_llm_prefs();
        cx.notify();
    }

    pub(crate) fn settings_ai_toggle_row(
        &mut self,
        cx: &mut Context<Self>,
        kind: AiToggleKind,
        title: &'static str,
        subtitle: &'static str,
    ) -> impl IntoElement {
        let on = self.ai_toggle_value(kind);
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .child(self.settings_labeled_block(title, subtitle))
            .child(
                div()
                    .px(px(10.))
                    .py(px(4.))
                    .rounded(px(9999.))
                    .cursor_pointer()
                    .bg(if on {
                        hex_a(ACCENT_BLUE, 0.35)
                    } else {
                        hex(CONTROL_BG)
                    })
                    .text_size(px(TYPE_CAPTION2))
                    .text_color(if on {
                        hex(TEXT_PRIMARY)
                    } else {
                        hex(TEXT_MUTED)
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.toggle_ai_setting(kind, cx);
                        }),
                    )
                    .child(if on { "オン" } else { "オフ" }),
            )
    }

    // ============================================================
    // 推論モード設定（CoT / ReAct / ToT / Self-Consistency）
    // ============================================================

    pub(crate) fn settings_cot_mode_row(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.ai_prefs.cot_mode;
        let modes = [
            model_prefs::CoTMode::Off,
            model_prefs::CoTMode::Basic,
            model_prefs::CoTMode::Detailed,
        ];
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(TYPE_BODY))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(hex(TEXT_PRIMARY))
                            .child("Chain-of-Thought (CoT)"),
                    )
                    .child(
                        div()
                            .text_size(px(TYPE_CAPTION2))
                            .text_color(hex(TEXT_MUTED))
                            .whitespace_normal()
                            .child(
                                "ローカル LLM の推論品質を向上させるステップバイステップ思考。Basic 推奨。",
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .children(modes.into_iter().map(|mode| {
                        let is_selected = mode == current;
                        div()
                            .flex_1()
                            .px(px(8.))
                            .py(px(6.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(if is_selected {
                                hex(ACCENT_BLUE)
                            } else {
                                hex(CONTROL_BORDER)
                            })
                            .bg(if is_selected {
                                hex_a(ACCENT_BLUE, 0.18)
                            } else {
                                hex(CONTROL_BG)
                            })
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    this.ai_prefs.cot_mode = mode;
                                    this.persist_local_llm_prefs();
                                    cx.notify();
                                }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(TYPE_CAPTION2))
                                            .text_color(hex(TEXT_PRIMARY))
                                            .child(mode.label()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(TYPE_CAPTION2))
                                            .text_color(hex(TEXT_MUTED))
                                            .child(mode.subtitle()),
                                    ),
                            )
                    })),
            )
    }

    pub(crate) fn settings_react_mode_row(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.ai_prefs.react_mode;
        let modes = [
            model_prefs::ReActMode::Off,
            model_prefs::ReActMode::Steps3,
            model_prefs::ReActMode::Steps5,
        ];
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(TYPE_BODY))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(hex(TEXT_PRIMARY))
                            .child("ReAct（検索・計算ツール）"),
                    )
                    .child(
                        div()
                            .text_size(px(TYPE_CAPTION2))
                            .text_color(hex(TEXT_MUTED))
                            .whitespace_normal()
                            .child(
                                "LLM が自律的にウェブ検索・計算・日時取得を使いながら推論。ローカル GGUF 専用。",
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .children(modes.into_iter().map(|mode| {
                        let is_selected = mode == current;
                        div()
                            .flex_1()
                            .px(px(8.))
                            .py(px(6.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(if is_selected {
                                hex(ACCENT_BLUE)
                            } else {
                                hex(CONTROL_BORDER)
                            })
                            .bg(if is_selected {
                                hex_a(ACCENT_BLUE, 0.18)
                            } else {
                                hex(CONTROL_BG)
                            })
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    this.ai_prefs.react_mode = mode;
                                    this.persist_local_llm_prefs();
                                    cx.notify();
                                }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(TYPE_CAPTION2))
                                            .text_color(hex(TEXT_PRIMARY))
                                            .child(mode.label()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(TYPE_CAPTION2))
                                            .text_color(hex(TEXT_MUTED))
                                            .child(mode.subtitle()),
                                    ),
                            )
                    })),
            )
    }

    pub(crate) fn settings_tot_mode_row(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.ai_prefs.tot_mode;
        let modes = [
            model_prefs::ToTMode::Off,
            model_prefs::ToTMode::Branch2,
            model_prefs::ToTMode::Branch3,
        ];
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(TYPE_BODY))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(hex(TEXT_PRIMARY))
                            .child("Tree-of-Thoughts (ToT)"),
                    )
                    .child(
                        div()
                            .text_size(px(TYPE_CAPTION2))
                            .text_color(hex(TEXT_MUTED))
                            .whitespace_normal()
                            .child(
                                "複数の思考経路を生成→評価→合成する3フェーズ推論。ローカル GGUF 専用。推論時間は3倍+。",
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .children(modes.into_iter().map(|mode| {
                        let is_selected = mode == current;
                        div()
                            .flex_1()
                            .px(px(8.))
                            .py(px(6.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(if is_selected {
                                hex(ACCENT_BLUE)
                            } else {
                                hex(CONTROL_BORDER)
                            })
                            .bg(if is_selected {
                                hex_a(ACCENT_BLUE, 0.18)
                            } else {
                                hex(CONTROL_BG)
                            })
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    this.ai_prefs.tot_mode = mode;
                                    this.persist_local_llm_prefs();
                                    cx.notify();
                                }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(TYPE_CAPTION2))
                                            .text_color(hex(TEXT_PRIMARY))
                                            .child(mode.label()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(TYPE_CAPTION2))
                                            .text_color(hex(TEXT_MUTED))
                                            .child(mode.subtitle()),
                                    ),
                            )
                    })),
            )
    }

    pub(crate) fn settings_self_consistency_row(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.ai_prefs.self_consistency;
        let modes = [
            model_prefs::SelfConsistencyMode::Off,
            model_prefs::SelfConsistencyMode::Vote3,
            model_prefs::SelfConsistencyMode::Vote5,
        ];
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(TYPE_BODY))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(hex(TEXT_PRIMARY))
                            .child("Self-Consistency（多数決）"),
                    )
                    .child(
                        div()
                            .text_size(px(TYPE_CAPTION2))
                            .text_color(hex(TEXT_MUTED))
                            .whitespace_normal()
                            .child(
                                "同じ質問を複数回推論し、最も一致する回答を採用。ローカル GGUF 専用。推論時間は投票数倍になります。",
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .children(modes.into_iter().map(|mode| {
                        let is_selected = mode == current;
                        div()
                            .flex_1()
                            .px(px(8.))
                            .py(px(6.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(if is_selected {
                                hex(ACCENT_BLUE)
                            } else {
                                hex(CONTROL_BORDER)
                            })
                            .bg(if is_selected {
                                hex_a(ACCENT_BLUE, 0.18)
                            } else {
                                hex(CONTROL_BG)
                            })
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    this.ai_prefs.self_consistency = mode;
                                    this.persist_local_llm_prefs();
                                    cx.notify();
                                }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(px(TYPE_CAPTION2))
                                            .text_color(hex(TEXT_PRIMARY))
                                            .child(mode.label()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(TYPE_CAPTION2))
                                            .text_color(hex(TEXT_MUTED))
                                            .child(mode.subtitle()),
                                    ),
                            )
                    })),
            )
    }
}
