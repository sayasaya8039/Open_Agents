//! モデル選択・形式判定・推論パラメータ・チャット推論先・モデルピッカー。

use gpui::prelude::FluentBuilder;
use gpui::*;
use std::fs;
use std::path::{Path, PathBuf};

use crate::chat_client;
use crate::colors::*;
use crate::model_prefs;

use super::{human_readable_size, AppView, ModelFormat, ModelParamAdjustKind};

impl AppView {
    pub(crate) fn settings_model_filename_for(path: &Path) -> SharedString {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.to_string().into())
            .unwrap_or_else(|| "（無名）".into())
    }

    pub(crate) fn settings_model_path_label_for(path: &Path) -> SharedString {
        path.to_string_lossy().into_owned().into()
    }

    pub(crate) fn settings_model_meta_label_for(path: &Path) -> SharedString {
        let format = ModelFormat::from_path(path).label();
        let size = fs::metadata(path)
            .ok()
            .map(|meta| human_readable_size(meta.len()))
            .unwrap_or_else(|| "サイズ不明".to_string());
        format!("{format} • {size}").into()
    }

    // ============================================================
    // ロード済みモデル行
    // ============================================================

    pub(crate) fn settings_loaded_model_row(
        &mut self,
        cx: &mut Context<Self>,
        index: usize,
        path: &PathBuf,
    ) -> impl IntoElement {
        let idx = index;
        div()
            .flex()
            .items_center()
            .justify_between()
            .p(px(12.))
            .bg(hex(BG))
            .border_1()
            .border_color(hex(BORDER))
            .rounded(px(8.))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(TYPE_CAPTION1))
                                    .text_color(hex(TEXT_PRIMARY))
                                    .child(Self::settings_model_filename_for(path)),
                            )
                            .child(
                                div()
                                    .text_size(px(TYPE_CAPTION1))
                                    .text_color(hex(FIGMA_ICON_GREEN))
                                    .child("✓"),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(TYPE_CAPTION2))
                            .text_color(hex(TEXT_MUTED))
                            .child(Self::settings_model_meta_label_for(path)),
                    )
                    .child(
                        div()
                            .text_size(px(TYPE_CAPTION2))
                            .text_color(hex(TEXT_DIM))
                            .child(Self::settings_model_path_label_for(path)),
                    ),
            )
            .child(
                div()
                    .px(px(10.))
                    .py(px(4.))
                    .rounded(px(4.))
                    .bg(hex(TITLEBAR_BG))
                    .text_size(px(TYPE_CAPTION2))
                    .text_color(hex(TEXT_MUTED))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            if idx < this.settings_model_paths.len() {
                                this.settings_model_paths.remove(idx);
                                if this.settings_model_paths.is_empty() {
                                    this.chat_prefs.local_model_index = 0;
                                } else {
                                    this.chat_prefs.local_model_index = this
                                        .chat_prefs
                                        .local_model_index
                                        .min(this.settings_model_paths.len() - 1);
                                }
                                this.chat_prefs = this.chat_prefs.clone().sanitize();
                                this.persist_local_llm_prefs();
                                cx.notify();
                            }
                        }),
                    )
                    .child("🗑"),
            )
    }

    // ============================================================
    // モデルパラメータ調整
    // ============================================================

    pub(crate) fn adjust_model_param(
        &mut self,
        kind: ModelParamAdjustKind,
        steps: i32,
        cx: &mut Context<Self>,
    ) {
        const TEMP_STEP: f32 = 0.1;
        match kind {
            ModelParamAdjustKind::Temperature => {
                let d = steps as f32 * TEMP_STEP;
                self.model_params.temperature = (self.model_params.temperature + d).clamp(0.0, 2.0);
            }
            ModelParamAdjustKind::MaxOutputTokens => {
                let v = self.model_params.max_output_tokens + steps * 256;
                self.model_params.max_output_tokens = v.clamp(256, 8192);
            }
            ModelParamAdjustKind::ContextLength => {
                let v = self.model_params.context_length + steps * 512;
                self.model_params.context_length = v.clamp(512, model_prefs::LOCAL_CONTEXT_LENGTH_CAP);
            }
        }
        self.model_params.clamp();
        self.normalize_model_params_for_chat_source();
        self.persist_local_llm_prefs();
        cx.notify();
    }

    pub(crate) fn normalize_model_params_for_chat_source(&mut self) {
        if self.chat_prefs.source == model_prefs::ChatInferenceSource::LocalWeights {
            self.model_params.max_output_tokens =
                model_prefs::effective_local_max_output_tokens(self.model_params.max_output_tokens);
        }
    }

    // ============================================================
    // チャット推論先・モデル選択
    // ============================================================

    pub(crate) fn cycle_chat_inference_source(&mut self, cx: &mut Context<Self>) {
        self.chat_prefs.source = self.chat_prefs.source.cycle();
        self.chat_prefs = self.chat_prefs.clone().sanitize();
        self.normalize_model_params_for_chat_source();
        self.persist_local_llm_prefs();
        // LocalWeights に切り替わった場合、サーバをプリウォーム
        self.prewarm_llama_server(cx);
        cx.notify();
    }

    pub(crate) fn adjust_chat_local_model_index(&mut self, delta: i32, cx: &mut Context<Self>) {
        if self.settings_model_paths.is_empty() {
            return;
        }
        let n = self.settings_model_paths.len() as i32;
        let cur = self.chat_prefs.local_model_index as i32;
        let next = (cur + delta).rem_euclid(n) as usize;
        self.chat_prefs.local_model_index = next;
        self.chat_prefs = self.chat_prefs.clone().sanitize();
        self.persist_local_llm_prefs();
        cx.notify();
    }

    /// モデル一覧をAPIから取得
    pub(crate) fn fetch_models(&mut self, cx: &mut Context<Self>) {
        if self.fetching_models {
            return;
        }
        self.fetching_models = true;
        cx.notify();
        let api_keys = self.api_keys.clone();
        cx.spawn(async move |this, cx| {
            let results =
                smol::unblock(move || chat_client::fetch_provider_models(&api_keys)).await;
            let _ = cx.update(|app| {
                let _ = this.update(app, |this: &mut AppView, cx| {
                    this.fetched_models = results;
                    this.fetching_models = false;
                    cx.notify();
                });
            });
        })
        .detach();
    }

    /// プロバイダ別モデルIDピッカー（API取得 + フォールバック）
    pub(crate) fn render_model_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.chat_prefs.api_model.clone();
        let fetching = self.fetching_models;

        // 取得済みモデル: (provider_id, label, models)
        let sections = self.fetched_models.clone();

        let has_any_key = chat_client::PROVIDER_ENDPOINTS
            .iter()
            .any(|(id, _, _)| !self.api_keys.get_value(id).is_empty());

        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            // 取得ボタン
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .px(px(10.))
                            .py(px(5.))
                            .bg(if fetching {
                                hex(BORDER)
                            } else {
                                hex(ACCENT_BLUE)
                            })
                            .rounded(px(6.))
                            .text_size(px(TYPE_CAPTION2))
                            .text_color(hex(0xFFFFFF))
                            .cursor(if fetching {
                                CursorStyle::OperationNotAllowed
                            } else {
                                CursorStyle::PointingHand
                            })
                            .when(!fetching, |d| {
                                d.on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                        this.fetch_models(cx);
                                    }),
                                )
                            })
                            .child(if fetching {
                                "取得中…"
                            } else {
                                "🔄 最新モデル一覧を取得"
                            }),
                    )
                    .when(!sections.is_empty(), |d| {
                        d.child(div().text_size(px(TYPE_CAPTION2)).text_color(hex(TEXT_MUTED)).child(
                            format!(
                                "{}プロバイダ / {}モデル",
                                sections.len(),
                                sections.iter().map(|(_, _, m)| m.len()).sum::<usize>()
                            ),
                        ))
                    }),
            )
            .when(!has_any_key && sections.is_empty(), |d| {
                d.child(
                    div()
                        .text_size(px(TYPE_CAPTION2))
                        .text_color(hex(TEXT_MUTED))
                        .child("上の「API キー管理」でプロバイダのキーを登録してください"),
                )
            })
            // モデルリスト
            .children(sections.into_iter().map(|(provider_id, label, models)| {
                let current = current.clone();
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .text_size(px(TYPE_CAPTION2))
                            .text_color(hex(TEXT_DIM))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(label),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(4.))
                            .children(models.into_iter().map(|model_id| {
                                let is_current = current == model_id;
                                let id_string = model_id.clone();
                                let pid = provider_id.clone();
                                div()
                                    .px(px(8.))
                                    .py(px(3.))
                                    .rounded(px(4.))
                                    .text_size(px(TYPE_CAPTION2))
                                    .bg(if is_current {
                                        hex(ACCENT_BLUE)
                                    } else {
                                        hex(BORDER)
                                    })
                                    .text_color(if is_current {
                                        hex(0xFFFFFF)
                                    } else {
                                        hex(TEXT_SECONDARY)
                                    })
                                    .cursor_pointer()
                                    .hover(|d| d.bg(hex(HOVER_BG)))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                            this.chat_prefs.api_model = id_string.clone();
                                            this.chat_prefs.api_provider = pid.clone();
                                            this.persist_local_llm_prefs();
                                            cx.notify();
                                        }),
                                    )
                                    .child(model_id)
                                    .into_any_element()
                            })),
                    )
                    .into_any_element()
            }))
    }

    // ============================================================
    // チャット推論設定ブロック UI
    // ============================================================

    pub(crate) fn settings_chat_inference_block(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let source_label = self.chat_prefs.source.label();
        let api_disp: SharedString = if self.chat_prefs.api_model.is_empty() {
            "（空＝OpenRouter / OpenAI / 汎用それぞれの既定モデル）".into()
        } else {
            self.chat_prefs.api_model.clone().into()
        };
        let ollama_disp: SharedString = self.chat_prefs.ollama_model.clone().into();
        let bundle_status: SharedString = self.llama_cpp_bundle_status_line().into();
        let bundle_error = self.selected_runtime_error();
        let update_notice = self.llama_cpp_update_notice.clone();

        div()
            .pt(px(16.))
            .border_t_1()
            .border_color(hex(BORDER))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .text_size(px(TYPE_BODY))
                    .text_color(hex(TEXT_PRIMARY))
                    .child("Chat での推論"),
            )
            .child(
                div()
                    .text_size(px(TYPE_CAPTION2))
                    .text_color(hex(TEXT_DIM))
                    .child(
                        "チャット送信時の推論先。「Ollama」は HTTP サーバ、「GGUF/ONNX」は設定に追加したファイルを内蔵 llama.cpp runtime 経由で実行します。",
                    ),
            )
            .child(
                div()
                    .text_size(px(TYPE_CAPTION2))
                    .text_color(hex(TEXT_MUTED))
                    .child(bundle_status),
            )
            .when(bundle_error.is_some(), |d| {
                d.child(
                    div()
                        .text_size(px(TYPE_CAPTION2))
                        .text_color(hex(ACCENT_ORANGE))
                        .whitespace_normal()
                        .child(bundle_error.clone().unwrap_or_default()),
                )
            })
            .when(update_notice.is_some(), |d| {
                let notice = update_notice.clone().unwrap();
                d.child(
                    div()
                        .mt(px(4.))
                        .p(px(10.))
                        .bg(hex(0x3b2b1c))
                        .border_1()
                        .border_color(hex(ACCENT_ORANGE))
                        .rounded(px(8.))
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            div()
                                .text_size(px(TYPE_CAPTION2))
                                .text_color(hex(ACCENT_ORANGE))
                                .child(format!(
                                    "llama-server 更新あり: {} → {}",
                                    notice.current_tag, notice.latest_tag
                                )),
                        )
                        .child(
                            div()
                                .text_size(px(TYPE_CAPTION2))
                                .text_color(hex(TEXT_MUTED))
                                .whitespace_normal()
                                .child("比較先: ggml-org/llama.cpp（同梱 runtime は Prism + upstream フォールバック構成）"),
                        )
                        .child(
                            div()
                                .text_size(px(TYPE_CAPTION2))
                                .text_color(hex(TEXT_MUTED))
                                .whitespace_normal()
                                .child(notice.release_url.clone()),
                        )
                        .child(
                            div()
                                .px(px(10.))
                                .py(px(5.))
                                .bg(hex(CONTROL_BG))
                                .border_1()
                                .border_color(hex(CONTROL_BORDER))
                                .rounded(px(6.))
                                .text_size(px(TYPE_CAPTION2))
                                .text_color(hex(TEXT_SECONDARY))
                                .cursor_pointer()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                        this.copy_llama_cpp_release_url(cx);
                                    }),
                                )
                                .child("リリース URL をコピー"),
                        ),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(px(TYPE_CAPTION1))
                                    .text_color(hex(TEXT_SECONDARY))
                                    .child("推論先"),
                            )
                            .child(
                                div()
                                    .text_size(px(TYPE_CAPTION1))
                                    .text_color(hex(TEXT_PRIMARY))
                                    .child(source_label),
                            ),
                    )
                    .child(
                        div()
                            .px(px(12.))
                            .py(px(6.))
                            .bg(hex(CONTROL_BG))
                            .border_1()
                            .border_color(hex(CONTROL_BORDER))
                            .rounded(px(6.))
                            .text_size(px(TYPE_CAPTION2))
                            .text_color(hex(TEXT_SECONDARY))
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.cycle_chat_inference_source(cx);
                                }),
                            )
                            .child("推論先を切替（API → Ollama → GGUF/ONNX）"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(TYPE_CAPTION1))
                            .text_color(hex(TEXT_SECONDARY))
                            .child("ネイティブ GGUF / ONNX（読み込み済み一覧から選択）"),
                    )
                    .child(
                        div()
                            .text_size(px(TYPE_CAPTION2))
                            .text_color(hex(TEXT_MUTED))
                            .child(self.chat_local_weights_summary()),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(
                                div()
                                    .px(px(10.))
                                    .py(px(5.))
                                    .bg(hex(CONTROL_BG))
                                    .border_1()
                                    .border_color(hex(CONTROL_BORDER))
                                    .rounded(px(6.))
                                    .text_size(px(TYPE_CAPTION2))
                                    .text_color(hex(TEXT_SECONDARY))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| {
                                            this.adjust_chat_local_model_index(-1, cx);
                                        }),
                                    )
                                    .child("← 前のモデル"),
                            )
                            .child(
                                div()
                                    .px(px(10.))
                                    .py(px(5.))
                                    .bg(hex(CONTROL_BG))
                                    .border_1()
                                    .border_color(hex(CONTROL_BORDER))
                                    .rounded(px(6.))
                                    .text_size(px(TYPE_CAPTION2))
                                    .text_color(hex(TEXT_SECONDARY))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| {
                                            this.adjust_chat_local_model_index(1, cx);
                                        }),
                                    )
                                    .child("次のモデル →"),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(TYPE_CAPTION1))
                            .text_color(hex(TEXT_SECONDARY))
                            .child("クラウド API モデル ID（クリックで選択）"),
                    )
                    .child(
                        div()
                            .p(px(10.))
                            .bg(hex(BG))
                            .border_1()
                            .border_color(hex(BORDER))
                            .rounded(px(6.))
                            .text_size(px(TYPE_CAPTION2))
                            .text_color(hex(TEXT_MUTED))
                            .child(api_disp),
                    )
                    // モデルIDリスト（クリックで選択）
                    .child(self.render_model_picker(cx))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(
                                div()
                                    .px(px(10.))
                                    .py(px(5.))
                                    .bg(hex(ACCENT_BLUE))
                                    .rounded(px(6.))
                                    .text_size(px(TYPE_CAPTION2))
                                    .text_color(hex(0xFFFFFF))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| {
                                            this.settings_chat_paste_api_model(cx);
                                        }),
                                    )
                                    .child("クリップボードから貼付"),
                            )
                            .child(
                                div()
                                    .px(px(10.))
                                    .py(px(5.))
                                    .bg(hex(BORDER))
                                    .rounded(px(6.))
                                    .text_size(px(TYPE_CAPTION2))
                                    .text_color(hex(TEXT_PRIMARY))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| {
                                            this.settings_chat_clear_api_model(cx);
                                        }),
                                    )
                                    .child("空に戻す"),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(TYPE_CAPTION1))
                            .text_color(hex(TEXT_SECONDARY))
                            .child("Ollama モデル名"),
                    )
                    .child(
                        div()
                            .p(px(10.))
                            .bg(hex(BG))
                            .border_1()
                            .border_color(hex(BORDER))
                            .rounded(px(6.))
                            .text_size(px(TYPE_CAPTION2))
                            .text_color(hex(TEXT_MUTED))
                            .child(ollama_disp),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(
                                div()
                                    .px(px(10.))
                                    .py(px(5.))
                                    .bg(hex(ACCENT_BLUE))
                                    .rounded(px(6.))
                                    .text_size(px(TYPE_CAPTION2))
                                    .text_color(hex(0xFFFFFF))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| {
                                            this.settings_chat_paste_ollama_model(cx);
                                        }),
                                    )
                                    .child("クリップボードから貼付"),
                            )
                            .child(
                                div()
                                    .px(px(10.))
                                    .py(px(5.))
                                    .bg(hex(BORDER))
                                    .rounded(px(6.))
                                    .text_size(px(TYPE_CAPTION2))
                                    .text_color(hex(TEXT_PRIMARY))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| {
                                            this.settings_chat_clear_ollama_model(cx);
                                        }),
                                    )
                                    .child("既定に戻す"),
                            ),
                    ),
            )
    }

    // ============================================================
    // モデルパラメータ行 UI（Temperature / MaxTokens / ContextLength）
    // ============================================================

    /// Temperature / 最大トークン / コンテキスト長 — 値は `model_prefs` と同期
    pub(crate) fn settings_model_param_row(
        &mut self,
        cx: &mut Context<Self>,
        kind: ModelParamAdjustKind,
        label: &'static str,
        hint: Option<&'static str>,
    ) -> impl IntoElement {
        let value_str: SharedString = match kind {
            ModelParamAdjustKind::Temperature => {
                format!("{:.1}", self.model_params.temperature).into()
            }
            ModelParamAdjustKind::MaxOutputTokens => {
                format!("{}", self.model_params.max_output_tokens).into()
            }
            ModelParamAdjustKind::ContextLength => {
                format!("{}", self.model_params.context_length).into()
            }
        };

        let k_minus = kind;
        let k_plus = kind;

        let mut col = div()
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
                            .child(label),
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
                                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                            this.adjust_model_param(k_minus, -1, cx);
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
                                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                            this.adjust_model_param(k_plus, 1, cx);
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
            .child(div().h(px(4.)).w_full().rounded(px(2.)).bg(hex(CONTROL_BG)));
        if let Some(h) = hint {
            col = col.child(div().text_size(px(TYPE_CAPTION2)).text_color(hex(TEXT_DIM)).child(h));
        }
        col
    }
}
