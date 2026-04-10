//! Settings UI, HF Discover ハンドラ、その他の AppView メソッド群。
//!
//! main.rs から分離した `impl AppView` ブロック。

use gpui::prelude::FluentBuilder;
use gpui::*;
use std::fs;
use std::path::{Path, PathBuf};

use crate::api_key_prefs;
use crate::api_server;
use crate::chat_client;
use crate::chat_composer;
use crate::colors::*;
use crate::discover_page;
use crate::hf_discover;
use crate::i18n;
use crate::llama_cpp_chat;
use crate::llama_cpp_runtime;
use crate::model_prefs;

use super::{
    human_readable_size, AppView, AiToggleKind, HardwareParamAdjustKind, ModelFormat,
    ModelParamAdjustKind, Page,
};

impl AppView {
    /// 形式ドロップダウン用（末尾＝直近追加のモデル）
    pub(crate) fn settings_last_model_path(&self) -> Option<&Path> {
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
                                    .text_size(px(12.))
                                    .text_color(hex(TEXT_PRIMARY))
                                    .child(Self::settings_model_filename_for(path)),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(hex(FIGMA_ICON_GREEN))
                                    .child("✓"),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(hex(TEXT_MUTED))
                            .child(Self::settings_model_meta_label_for(path)),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
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
                    .text_size(px(11.))
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

    pub(crate) fn sync_api_key_reveal_len(&mut self) {
        let n = api_key_prefs::PROVIDER_CATALOG.len();
        if self.api_key_reveal.len() != n {
            self.api_key_reveal.resize(n, false);
        }
    }

    pub(crate) fn settings_api_key_paste_row(&mut self, provider_id: &'static str, cx: &mut Context<Self>) {
        if let Some(item) = cx.read_from_clipboard() {
            if let Some(text) = item.text() {
                let t = text.trim();
                if !t.is_empty() {
                    self.api_keys.set_entry(provider_id, t.to_string());
                    api_key_prefs::save_api_keys(&self.api_keys);
                    cx.notify();
                }
            }
        }
    }

    pub(crate) fn settings_api_key_clear_row(
        &mut self,
        row_idx: usize,
        provider_id: &'static str,
        cx: &mut Context<Self>,
    ) {
        self.api_keys.set_entry(provider_id, String::new());
        if let Some(r) = self.api_key_reveal.get_mut(row_idx) {
            *r = false;
        }
        api_key_prefs::save_api_keys(&self.api_keys);
        cx.notify();
    }

    pub(crate) fn settings_api_key_toggle_reveal_row(&mut self, row_idx: usize, cx: &mut Context<Self>) {
        if let Some(r) = self.api_key_reveal.get_mut(row_idx) {
            *r = !*r;
        }
        cx.notify();
    }

    pub(crate) fn settings_api_key_row(
        &mut self,
        cx: &mut Context<Self>,
        row_idx: usize,
        def: api_key_prefs::ProviderDef,
    ) -> impl IntoElement {
        let provider_id = def.id;
        let effective = self.api_keys.get_value(provider_id);
        let from_env = self.api_keys.is_from_env(provider_id);
        let env_name_opt = self.api_keys.active_env_var_name(provider_id);
        let has_key = !effective.is_empty();
        let reveal = self.api_key_reveal.get(row_idx).copied().unwrap_or(false);
        let masked: SharedString = api_key_prefs::masked_line(&effective, reveal).into();
        let title = def.title;
        let tag = def.env_hint;
        let kind_line: &'static str = match def.kind {
            api_key_prefs::CredentialKind::BaseUrl => "種別: ベース URL",
            api_key_prefs::CredentialKind::SecretToken => "種別: API キー / トークン",
        };
        let reveal_btn: &'static str = if reveal { "隠す" } else { "表示" };
        let id_paste = provider_id;
        let row_reveal = row_idx;
        let row_clear = row_idx;
        let id_clear = provider_id;

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
                    .gap(px(6.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(hex(TEXT_PRIMARY))
                                    .child(title),
                            )
                            .when(has_key, |d| {
                                d.child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(hex(FIGMA_ICON_GREEN))
                                        .child("✓"),
                                )
                            })
                            .when(from_env, |d| {
                                d.child(
                                    div()
                                        .px(px(6.))
                                        .py(px(1.))
                                        .bg(hex_a(TRAFFIC_GREEN, 0.2))
                                        .rounded(px(3.))
                                        .text_size(px(10.))
                                        .text_color(hex(TRAFFIC_GREEN))
                                        .child("ENV"),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(hex(TEXT_MUTED))
                                    .child(tag),
                            )
                            .when_some(env_name_opt, |d, name| {
                                let label: SharedString =
                                    format!("← ${name}").into();
                                d.child(
                                    div()
                                        .text_size(px(10.))
                                        .text_color(hex(TEXT_DIM))
                                        .child(label),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(hex(TEXT_DIM))
                            .child(kind_line),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_family("Cascadia Code")
                            .text_color(hex(TEXT_SECONDARY))
                            .child(masked),
                    ),
            )
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
                            .bg(hex(CONTROL_BG))
                            .text_size(px(11.))
                            .text_color(hex(TEXT_SECONDARY))
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    this.settings_api_key_paste_row(id_paste, cx);
                                }),
                            )
                            .child("貼り付け"),
                    )
                    .child(
                        div()
                            .px(px(8.))
                            .py(px(4.))
                            .rounded(px(6.))
                            .bg(hex(CONTROL_BG))
                            .text_size(px(11.))
                            .text_color(hex(TEXT_SECONDARY))
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    this.settings_api_key_toggle_reveal_row(row_reveal, cx);
                                }),
                            )
                            .child(reveal_btn),
                    )
                    .when(has_key, |d| {
                        d.child(
                            div()
                                .px(px(8.))
                                .py(px(4.))
                                .rounded(px(6.))
                                .bg(hex_a(TRAFFIC_RED, 0.2))
                                .text_size(px(11.))
                                .text_color(hex(TRAFFIC_RED))
                                .cursor_pointer()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                        this.settings_api_key_clear_row(row_clear, id_clear, cx);
                                    }),
                                )
                                .child("クリア"),
                        )
                    }),
            )
    }

    /// API キー一覧（グループ見出し付き）を `AnyElement` の列で返す
    pub(crate) fn settings_api_keys_child_elements(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        self.sync_api_key_reveal_len();
        let mut out = Vec::new();
        let mut last_group: Option<&'static str> = None;
        for (idx, def) in api_key_prefs::PROVIDER_CATALOG.iter().enumerate() {
            if last_group != Some(def.group) {
                last_group = Some(def.group);
                out.push(
                    div()
                        .pt(px(8.))
                        .pb(px(4.))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(hex(TEXT_MUTED))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(api_key_prefs::translate_group(def.group)),
                        )
                        .into_any_element(),
                );
            }
            out.push(self.settings_api_key_row(cx, idx, *def).into_any_element());
        }
        out
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
    // ============================================================
    // Settings View（Figma Make: SettingsView.tsx — 単一スクロール + セクションカード）
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

    // ── API サーバー管理 ──

    pub(crate) fn resolve_proxy_target(&self) -> (String, String) {
        match chat_client::resolve_chat_backend(
            &self.api_keys,
            &self.chat_prefs,
            &self.settings_model_paths,
        ) {
            Ok(backend) => backend.proxy_target(),
            Err(_) => (String::new(), String::new()),
        }
    }

    pub(crate) fn start_api_server(&mut self) {
        // 既に起動中なら停止
        if let Some(mut s) = self.api_server.take() {
            s.stop();
        }
        let (proxy_url, proxy_key) = self.resolve_proxy_target();
        let config = api_server::ApiServerConfig {
            port: self.api_server_prefs.port,
            api_key: self.api_server_prefs.api_key.clone(),
            proxy_target_url: proxy_url,
            proxy_target_key: proxy_key,
        };
        match api_server::ApiServer::start(config) {
            Ok(server) => {
                self.api_server = Some(server);
                self.api_server_error = None;
            }
            Err(e) => {
                self.api_server_error = Some(e);
            }
        }
    }

    pub(crate) fn stop_api_server(&mut self) {
        if let Some(mut s) = self.api_server.take() {
            s.stop();
        }
        self.api_server_error = None;
    }

    pub(crate) fn api_server_is_running(&self) -> bool {
        self.api_server.as_ref().map_or(false, |s| s.is_running())
    }

    pub(crate) fn settings_api_server_section(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let running = self.api_server_is_running();
        let base_url: SharedString =
            format!("http://localhost:{}", self.api_server_prefs.port).into();
        let masked_key: SharedString = if self.api_server_prefs.api_key.len() > 8 {
            format!("{}...", &self.api_server_prefs.api_key[..8]).into()
        } else {
            self.api_server_prefs.api_key.clone().into()
        };
        let full_key = self.api_server_prefs.api_key.clone();
        let full_base_url = base_url.clone();
        let port_label: SharedString = format!("{}", self.api_server_prefs.port).into();
        let status_label: SharedString = if running {
            "稼働中".into()
        } else if let Some(ref e) = self.api_server_error {
            SharedString::from(format!("エラー: {}", e))
        } else {
            "停止中".into()
        };
        let status_color = if running {
            FIGMA_ICON_GREEN
        } else if self.api_server_error.is_some() {
            TRAFFIC_RED
        } else {
            TEXT_MUTED
        };

        div()
            .flex()
            .flex_col()
            .child(self.settings_figma_heading(
                "🌐",
                FIGMA_ICON_BLUE,
                "API サーバー（外部アプリ連携）",
            ))
            .child(
                div()
                    .bg(hex(PANEL_BG))
                    .rounded(px(8.))
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    // ON/OFF トグル
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                self.settings_labeled_block(
                                    "サーバー",
                                    "外部アプリから OpenAI 互換 API として利用",
                                ),
                            )
                            .child(
                                div()
                                    .px(px(10.))
                                    .py(px(4.))
                                    .rounded(px(9999.))
                                    .cursor_pointer()
                                    .bg(if running {
                                        hex_a(ACCENT_BLUE, 0.35)
                                    } else {
                                        hex(CONTROL_BG)
                                    })
                                    .text_size(px(11.))
                                    .text_color(if running {
                                        hex(TEXT_PRIMARY)
                                    } else {
                                        hex(TEXT_MUTED)
                                    })
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                            if this.api_server_is_running() {
                                                this.stop_api_server();
                                                this.api_server_prefs.enabled = false;
                                            } else {
                                                this.api_server_prefs.enabled = true;
                                                this.start_api_server();
                                            }
                                            this.persist_local_llm_prefs();
                                            cx.notify();
                                        }),
                                    )
                                    .child(if running { "ON" } else { "OFF" }),
                            ),
                    )
                    // ポート
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(hex(TEXT_PRIMARY))
                                    .child("ポート"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(hex(TEXT_SECONDARY))
                                    .child(port_label),
                            ),
                    )
                    // ベース URL + コピー
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(hex(TEXT_PRIMARY))
                                    .child("ベース URL"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(hex(TEXT_SECONDARY))
                                            .child(full_base_url.clone()),
                                    )
                                    .child(
                                        div()
                                            .px(px(6.))
                                            .py(px(2.))
                                            .rounded(px(4.))
                                            .bg(hex(CONTROL_BG))
                                            .text_size(px(10.))
                                            .text_color(hex(TEXT_MUTED))
                                            .cursor_pointer()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(move |_, _: &MouseDownEvent, _window, cx| {
                                                    cx.write_to_clipboard(
                                                        ClipboardItem::new_string(
                                                            full_base_url.to_string(),
                                                        ),
                                                    );
                                                }),
                                            )
                                            .child("コピー"),
                                    ),
                            ),
                    )
                    // API キー + コピー + 再生成
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(hex(TEXT_PRIMARY))
                                    .child("API キー"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(hex(TEXT_SECONDARY))
                                            .child(masked_key),
                                    )
                                    .child({
                                        let key_for_copy = full_key.clone();
                                        div()
                                            .px(px(6.))
                                            .py(px(2.))
                                            .rounded(px(4.))
                                            .bg(hex(CONTROL_BG))
                                            .text_size(px(10.))
                                            .text_color(hex(TEXT_MUTED))
                                            .cursor_pointer()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(
                                                    move |_, _: &MouseDownEvent, _window, cx| {
                                                        cx.write_to_clipboard(
                                                            ClipboardItem::new_string(
                                                                key_for_copy.clone(),
                                                            ),
                                                        );
                                                    },
                                                ),
                                            )
                                            .child("コピー")
                                    })
                                    .child(
                                        div()
                                            .px(px(6.))
                                            .py(px(2.))
                                            .rounded(px(4.))
                                            .bg(hex(CONTROL_BG))
                                            .text_size(px(10.))
                                            .text_color(hex(TEXT_MUTED))
                                            .cursor_pointer()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                                    this.api_server_prefs.api_key =
                                                        api_server::generate_api_key();
                                                    this.persist_local_llm_prefs();
                                                    // 稼働中なら再起動して新キーを反映
                                                    if this.api_server_is_running() {
                                                        this.start_api_server();
                                                    }
                                                    cx.notify();
                                                }),
                                            )
                                            .child("再生成"),
                                    ),
                            ),
                    )
                    // ステータス
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(hex(TEXT_PRIMARY))
                                    .child("ステータス"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(hex(status_color))
                                    .child(status_label),
                            ),
                    ),
            )
    }

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
                            .text_size(px(11.))
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
                        d.child(div().text_size(px(10.)).text_color(hex(TEXT_MUTED)).child(
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
                        .text_size(px(11.))
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
                            .text_size(px(10.))
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
                                    .text_size(px(10.))
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

    pub(crate) fn settings_chat_paste_api_model(&mut self, cx: &mut Context<Self>) {
        if let Some(item) = cx.read_from_clipboard() {
            if let Some(text) = item.text() {
                let t = text.trim();
                if !t.is_empty() {
                    self.chat_prefs.api_model = t.to_string();
                    self.chat_prefs = self.chat_prefs.clone().sanitize();
                    self.persist_local_llm_prefs();
                    cx.notify();
                }
            }
        }
    }

    pub(crate) fn settings_chat_clear_api_model(&mut self, cx: &mut Context<Self>) {
        self.chat_prefs.api_model.clear();
        self.chat_prefs = self.chat_prefs.clone().sanitize();
        self.persist_local_llm_prefs();
        cx.notify();
    }

    pub(crate) fn settings_chat_paste_ollama_model(&mut self, cx: &mut Context<Self>) {
        if let Some(item) = cx.read_from_clipboard() {
            if let Some(text) = item.text() {
                let t = text.trim();
                if !t.is_empty() {
                    self.chat_prefs.ollama_model = t.to_string();
                    self.chat_prefs = self.chat_prefs.clone().sanitize();
                    self.persist_local_llm_prefs();
                    cx.notify();
                }
            }
        }
    }

    pub(crate) fn settings_chat_clear_ollama_model(&mut self, cx: &mut Context<Self>) {
        self.chat_prefs.ollama_model = model_prefs::ChatPrefs::default().ollama_model;
        self.chat_prefs = self.chat_prefs.clone().sanitize();
        self.persist_local_llm_prefs();
        cx.notify();
    }

    pub(crate) fn chat_local_weights_summary(&self) -> String {
        if self.settings_model_paths.is_empty() {
            return "モデルファイルを「ローカルLLM設定」で追加すると、ここで番号を選べます。"
                .into();
        }
        let i = self
            .chat_prefs
            .local_model_index
            .min(self.settings_model_paths.len() - 1);
        let path = &self.settings_model_paths[i];
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("-");
        format!(
            "選択中 [{}/{}]: {}",
            i + 1,
            self.settings_model_paths.len(),
            name
        )
    }

    /// Chat ヘッダー用: 現在の推論先とモデル
    pub(crate) fn chat_model_status_line(&self) -> String {
        match self.chat_prefs.source {
            model_prefs::ChatInferenceSource::Api => {
                let m = self.chat_prefs.api_model.trim();
                if m.is_empty() {
                    "Chat: クラウド API（モデルはプロバイダ既定）".to_string()
                } else {
                    format!("Chat: クラウド API（{m}）")
                }
            }
            model_prefs::ChatInferenceSource::Local => {
                let m = self.chat_prefs.ollama_model.trim();
                format!("Chat: Ollama（{m}）")
            }
            model_prefs::ChatInferenceSource::LocalWeights => {
                if self.settings_model_paths.is_empty() {
                    "Chat: ローカル GGUF/ONNX（モデル未登録）".to_string()
                } else {
                    let i = self
                        .chat_prefs
                        .local_model_index
                        .min(self.settings_model_paths.len() - 1);
                    let name = self.settings_model_paths[i]
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("-");
                    format!(
                        "Chat: GGUF/ONNX [{}/{}] {name}",
                        i + 1,
                        self.settings_model_paths.len()
                    )
                }
            }
        }
    }

    pub(crate) fn selected_runtime_backend(&self) -> llama_cpp_runtime::BundledLlamaBackend {
        match self.hardware_params.llama_runtime_preset {
            model_prefs::LlamaRuntimePreset::Auto => {
                // Auto: GPU プロファイルの resolved_preset に従う
                let profile = model_prefs::detect_gpu_profile();
                match profile.resolved_preset {
                    model_prefs::LlamaRuntimePreset::NvidiaCuda => {
                        llama_cpp_runtime::BundledLlamaBackend::Cuda
                    }
                    model_prefs::LlamaRuntimePreset::VulkanHybrid => {
                        llama_cpp_runtime::BundledLlamaBackend::Vulkan
                    }
                    _ => llama_cpp_runtime::BundledLlamaBackend::Cpu,
                }
            }
            model_prefs::LlamaRuntimePreset::NvidiaCuda => {
                llama_cpp_runtime::BundledLlamaBackend::Cuda
            }
            model_prefs::LlamaRuntimePreset::VulkanHybrid => {
                llama_cpp_runtime::BundledLlamaBackend::Vulkan
            }
            model_prefs::LlamaRuntimePreset::CpuOnly => {
                llama_cpp_runtime::BundledLlamaBackend::Cpu
            }
        }
    }

    pub(crate) fn runtime_status_for_backend(
        &self,
        backend: llama_cpp_runtime::BundledLlamaBackend,
    ) -> Option<&llama_cpp_runtime::BundledLlamaRuntimeStatus> {
        self.llama_cpp_runtime_statuses
            .iter()
            .find(|status| status.backend == backend)
    }

    pub(crate) fn selected_runtime_manifest(&self) -> Option<&llama_cpp_runtime::BundledLlamaManifest> {
        self.runtime_status_for_backend(self.selected_runtime_backend())
            .and_then(|status| status.manifest.as_ref())
    }

    pub(crate) fn selected_runtime_error(&self) -> Option<String> {
        self.runtime_status_for_backend(self.selected_runtime_backend())
            .and_then(|status| status.error.clone())
    }

    pub(crate) fn runtime_preset_is_available(&self, preset: model_prefs::LlamaRuntimePreset) -> bool {
        match preset {
            model_prefs::LlamaRuntimePreset::Auto => true, // Auto は常に利用可能
            model_prefs::LlamaRuntimePreset::NvidiaCuda => self
                .runtime_status_for_backend(llama_cpp_runtime::BundledLlamaBackend::Cuda)
                .and_then(|status| status.manifest.as_ref())
                .is_some(),
            model_prefs::LlamaRuntimePreset::VulkanHybrid => self
                .runtime_status_for_backend(llama_cpp_runtime::BundledLlamaBackend::Vulkan)
                .and_then(|status| status.manifest.as_ref())
                .is_some(),
            model_prefs::LlamaRuntimePreset::CpuOnly => self
                .runtime_status_for_backend(llama_cpp_runtime::BundledLlamaBackend::Cpu)
                .and_then(|status| status.manifest.as_ref())
                .is_some(),
        }
    }

    pub(crate) fn llama_cpp_bundle_status_line(&self) -> String {
        let backend = self.selected_runtime_backend();
        if let Some(manifest) = self.selected_runtime_manifest() {
            return format!(
                "内蔵 llama-server [{}]: {} ({})",
                backend.label(),
                manifest.llama_server_version,
                manifest.platform
            );
        }
        format!("内蔵 llama-server [{}]: 未同梱", backend.label())
    }

    pub(crate) fn copy_llama_cpp_release_url(&mut self, cx: &mut Context<Self>) {
        if let Some(notice) = &self.llama_cpp_update_notice {
            cx.write_to_clipboard(ClipboardItem::new_string(notice.release_url.clone()));
        }
    }

    pub(crate) fn start_llama_cpp_update_check(&mut self, cx: &mut Context<Self>) {
        self.llama_cpp_update_notice = None;
        let Some(manifest) = self.selected_runtime_manifest().cloned() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let notice = smol::unblock(move || {
                let latest = llama_cpp_runtime::fetch_latest_release().ok()?;
                llama_cpp_runtime::compute_update_notice(&manifest, &latest)
            })
            .await;

            let Some(notice) = notice else {
                return;
            };

            let _ = cx.update(|app| {
                let _ = this.update(app, |this: &mut AppView, cx| {
                    this.llama_cpp_update_notice = Some(notice);
                    cx.notify();
                });
            });
        })
        .detach();
    }

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
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(hex(TEXT_PRIMARY))
                            .child("Chain-of-Thought (CoT)"),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
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
                                            .text_size(px(11.))
                                            .text_color(hex(TEXT_PRIMARY))
                                            .child(mode.label()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(9.))
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
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(hex(TEXT_PRIMARY))
                            .child("ReAct（検索・計算ツール）"),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
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
                                            .text_size(px(11.))
                                            .text_color(hex(TEXT_PRIMARY))
                                            .child(mode.label()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(9.))
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
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(hex(TEXT_PRIMARY))
                            .child("Tree-of-Thoughts (ToT)"),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
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
                                            .text_size(px(11.))
                                            .text_color(hex(TEXT_PRIMARY))
                                            .child(mode.label()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(9.))
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
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(hex(TEXT_PRIMARY))
                            .child("Self-Consistency（多数決）"),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
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
                                            .text_size(px(11.))
                                            .text_color(hex(TEXT_PRIMARY))
                                            .child(mode.label()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(9.))
                                            .text_color(hex(TEXT_MUTED))
                                            .child(mode.subtitle()),
                                    ),
                            )
                    })),
            )
    }

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
                    .text_size(px(13.))
                    .text_color(hex(TEXT_PRIMARY))
                    .child("Chat での推論"),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(hex(TEXT_DIM))
                    .child(
                        "チャット送信時の推論先。「Ollama」は HTTP サーバ、「GGUF/ONNX」は設定に追加したファイルを内蔵 llama.cpp runtime 経由で実行します。",
                    ),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(hex(TEXT_MUTED))
                    .child(bundle_status),
            )
            .when(bundle_error.is_some(), |d| {
                d.child(
                    div()
                        .text_size(px(11.))
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
                                .text_size(px(11.))
                                .text_color(hex(ACCENT_ORANGE))
                                .child(format!(
                                    "llama-server 更新あり: {} → {}",
                                    notice.current_tag, notice.latest_tag
                                )),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(hex(TEXT_MUTED))
                                .whitespace_normal()
                                .child("比較先: ggml-org/llama.cpp（同梱 runtime は Prism + upstream フォールバック構成）"),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
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
                                .text_size(px(11.))
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
                                    .text_size(px(12.))
                                    .text_color(hex(TEXT_SECONDARY))
                                    .child("推論先"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
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
                            .text_size(px(11.))
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
                            .text_size(px(12.))
                            .text_color(hex(TEXT_SECONDARY))
                            .child("ネイティブ GGUF / ONNX（読み込み済み一覧から選択）"),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
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
                                    .text_size(px(11.))
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
                                    .text_size(px(11.))
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
                            .text_size(px(12.))
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
                            .text_size(px(11.))
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
                                    .text_size(px(11.))
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
                                    .text_size(px(11.))
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
                            .text_size(px(12.))
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
                            .text_size(px(11.))
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
                                    .text_size(px(11.))
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
                                    .text_size(px(11.))
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
                    .text_size(px(12.))
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
                            .text_size(px(13.))
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
                                            .text_size(px(14.))
                                            .text_color(hex(TEXT_SECONDARY))
                                            .child("−"),
                                    ),
                            )
                            .child(
                                div()
                                    .min_w(px(52.))
                                    .text_size(px(12.))
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
                                            .text_size(px(14.))
                                            .text_color(hex(TEXT_SECONDARY))
                                            .child("+"),
                                    ),
                            ),
                    ),
            )
            .child(
                div()
                    .text_size(px(11.))
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
                    .text_size(px(11.))
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
                    .text_size(px(11.))
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

    pub(crate) fn adjust_hardware_param(
        &mut self,
        kind: HardwareParamAdjustKind,
        steps: i32,
        cx: &mut Context<Self>,
    ) {
        match kind {
            HardwareParamAdjustKind::GpuLayers => {
                let v = self.hardware_params.gpu_layers + steps;
                self.hardware_params.gpu_layers = v.clamp(0, 80);
            }
            HardwareParamAdjustKind::NThreads => {
                let v = self.hardware_params.n_threads + steps;
                self.hardware_params.n_threads = v.clamp(1, 32);
            }
            HardwareParamAdjustKind::BatchSize => {
                let v = self.hardware_params.batch_size + steps * 128;
                self.hardware_params.batch_size = v.clamp(128, 2048);
            }
        }
        self.hardware_params.clamp();
        self.persist_local_llm_prefs();
        cx.notify();
    }

    pub(crate) fn settings_runtime_preset_row(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.hardware_params.llama_runtime_preset;
        let options = model_prefs::LlamaRuntimePreset::ALL;
        let gpu_summary: SharedString = if self.gpu_profile_summary.is_empty() {
            "GPU 検出中…".into()
        } else {
            format!("検出: {}", self.gpu_profile_summary).into()
        };
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(self.settings_labeled_block(
                "実行モード",
                "起動時に GPU を自動検出し、最適な runtime を選択します。手動で固定することもできます。",
            ))
            .child(
                div()
                    .px(px(10.))
                    .py(px(6.))
                    .rounded(px(6.))
                    .bg(hex(PANEL_BG))
                    .border_1()
                    .border_color(hex(BORDER))
                    .text_size(px(11.))
                    .text_color(hex(TEXT_SECONDARY))
                    .child(gpu_summary),
            )
            .child(
                div().flex().flex_col().gap(px(8.)).children(options.into_iter().map(|preset| {
                    let is_selected = preset == selected;
                    let is_available = self.runtime_preset_is_available(preset);
                    let badge = if preset.is_experimental() {
                        "Experimental"
                    } else if is_available {
                        "Available"
                    } else {
                        "Unavailable"
                    };
                    div()
                        .p(px(10.))
                        .rounded(px(8.))
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
                        .when(!is_available, |d| d.opacity(0.55))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                if !this.runtime_preset_is_available(preset) {
                                    return;
                                }
                                this.hardware_params.llama_runtime_preset = preset;
                                this.hardware_params.gpu_acceleration = true;
                                this.persist_local_llm_prefs();
                                this.start_llama_cpp_update_check(cx);
                                cx.notify();
                            }),
                        )
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
                                                .text_size(px(12.))
                                                .text_color(hex(TEXT_PRIMARY))
                                                .child(preset.label()),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(11.))
                                                .text_color(hex(TEXT_MUTED))
                                                .whitespace_normal()
                                                .child(preset.subtitle()),
                                        ),
                                )
                                .child(
                                    div()
                                        .px(px(10.))
                                        .py(px(4.))
                                        .rounded(px(9999.))
                                        .bg(if is_selected {
                                            hex_a(ACCENT_BLUE, 0.35)
                                        } else if preset.is_experimental() {
                                            hex_a(ACCENT_ORANGE, 0.2)
                                        } else {
                                            hex(0x2c2c2c)
                                        })
                                        .text_size(px(10.))
                                        .text_color(if is_selected {
                                            hex(TEXT_PRIMARY)
                                        } else if is_available {
                                            hex(TEXT_SECONDARY)
                                        } else {
                                            hex(TEXT_MUTED)
                                        })
                                        .child(badge),
                                ),
                        )
                })),
            )
    }

    pub(crate) fn settings_hardware_stepper_row(
        &mut self,
        cx: &mut Context<Self>,
        kind: HardwareParamAdjustKind,
        label: &'static str,
        hint: Option<&'static str>,
    ) -> impl IntoElement {
        let value_str: SharedString = match kind {
            HardwareParamAdjustKind::GpuLayers => {
                format!("{}", self.hardware_params.gpu_layers).into()
            }
            HardwareParamAdjustKind::NThreads => {
                format!("{}", self.hardware_params.n_threads).into()
            }
            HardwareParamAdjustKind::BatchSize => {
                format!("{}", self.hardware_params.batch_size).into()
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
                            .text_size(px(13.))
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
                                            this.adjust_hardware_param(k_minus, -1, cx);
                                        }),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .text_color(hex(TEXT_SECONDARY))
                                            .child("−"),
                                    ),
                            )
                            .child(
                                div()
                                    .min_w(px(52.))
                                    .text_size(px(12.))
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
                                            this.adjust_hardware_param(k_plus, 1, cx);
                                        }),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .text_color(hex(TEXT_SECONDARY))
                                            .child("+"),
                                    ),
                            ),
                    ),
            )
            .child(div().h(px(4.)).w_full().rounded(px(2.)).bg(hex(CONTROL_BG)));
        if let Some(h) = hint {
            col = col.child(div().text_size(px(11.)).text_color(hex(TEXT_DIM)).child(h));
        }
        col
    }

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
                            .text_size(px(13.))
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
                                            .text_size(px(14.))
                                            .text_color(hex(TEXT_SECONDARY))
                                            .child("−"),
                                    ),
                            )
                            .child(
                                div()
                                    .min_w(px(52.))
                                    .text_size(px(12.))
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
                                            .text_size(px(14.))
                                            .text_color(hex(TEXT_SECONDARY))
                                            .child("+"),
                                    ),
                            ),
                    ),
            )
            .child(div().h(px(4.)).w_full().rounded(px(2.)).bg(hex(CONTROL_BG)));
        if let Some(h) = hint {
            col = col.child(div().text_size(px(11.)).text_color(hex(TEXT_DIM)).child(h));
        }
        col
    }

    // ============================================================
    // Hugging Face Discover ハンドラ
    // ============================================================

    pub(crate) fn hf_open_discover(&mut self, cx: &mut Context<Self>) {
        self.page = Page::Discover;
        cx.notify();
    }

    pub(crate) fn hf_cycle_sort(&mut self, cx: &mut Context<Self>) {
        self.hf_state.sort = match self.hf_state.sort {
            hf_discover::SortOrder::Trending => hf_discover::SortOrder::Downloads,
            hf_discover::SortOrder::Downloads => hf_discover::SortOrder::Likes,
            hf_discover::SortOrder::Likes => hf_discover::SortOrder::LastModified,
            hf_discover::SortOrder::LastModified => hf_discover::SortOrder::Trending,
        };
        cx.notify();
        // 既に検索済みなら同じクエリで再検索
        if !self.hf_state.results.is_empty() || !self.hf_state.query.is_empty() {
            self.hf_execute_search(cx);
        }
    }

    pub(crate) fn hf_toggle_downloads_panel(&mut self, cx: &mut Context<Self>) {
        self.hf_downloads.panel_open = !self.hf_downloads.panel_open;
        cx.notify();
    }

    pub(crate) fn hf_execute_search(&mut self, cx: &mut Context<Self>) {
        // 検索中は再入禁止（連打・Enter 連打対策）
        if self.hf_state.loading {
            return;
        }
        // 検索バーから現在のテキストを取得
        let query = self.hf_search_composer.read(cx).text().trim().to_string();
        self.hf_state.query = query.clone();
        self.hf_state.loading = true;
        self.hf_state.error = None;
        self.hf_state.request_gen = self.hf_state.request_gen.wrapping_add(1);
        let gen = self.hf_state.request_gen;
        let sort = self.hf_state.sort;
        let token = self.api_keys.get_value("huggingface");
        cx.notify();

        cx.spawn(async move |app: WeakEntity<AppView>, cx: &mut AsyncApp| {
            let token_opt = if token.is_empty() { None } else { Some(token) };
            let result = smol::unblock(move || {
                hf_discover::search_hf_models(&query, sort, token_opt.as_deref())
            })
            .await;
            let _ = cx.update(|ecx| {
                let _ = app.update(ecx, |this: &mut AppView, cx| {
                    // 古いレスポンスは破棄
                    if this.hf_state.request_gen != gen {
                        return;
                    }
                    this.hf_state.loading = false;
                    match result {
                        Ok(models) => {
                            this.hf_state.results = models;
                        }
                        Err(e) => {
                            this.hf_state.error = Some(e);
                            this.hf_state.results.clear();
                        }
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    pub(crate) fn hf_select_model(&mut self, id: String, cx: &mut Context<Self>) {
        // 既に同じモデルが選択済みなら再取得しない
        if self.hf_state.selected_id.as_deref() == Some(id.as_str()) {
            return;
        }
        self.hf_state.selected_id = Some(id.clone());
        self.hf_state.detail = None;
        self.hf_state.detail_loading = true;
        self.hf_state.detail_error = None;
        let token = self.api_keys.get_value("huggingface");
        cx.notify();

        cx.spawn(async move |app: WeakEntity<AppView>, cx: &mut AsyncApp| {
            let token_opt = if token.is_empty() { None } else { Some(token) };
            let id_c = id.clone();
            let result = smol::unblock(move || {
                hf_discover::fetch_model_detail(&id_c, token_opt.as_deref())
            })
            .await;
            let _ = cx.update(|ecx| {
                let _ = app.update(ecx, |this: &mut AppView, cx| {
                    // 選択が変わっていたら破棄
                    if this.hf_state.selected_id.as_deref() != Some(&id) {
                        return;
                    }
                    this.hf_state.detail_loading = false;
                    match result {
                        Ok(detail) => this.hf_state.detail = Some(detail),
                        Err(e) => this.hf_state.detail_error = Some(e),
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    pub(crate) fn hf_start_download(
        &mut self,
        model_id: String,
        file: hf_discover::GgufFile,
        cx: &mut Context<Self>,
    ) {
        // 1. キューにタスクを積む
        let token = self.api_keys.get_value("huggingface");
        let token_opt = if token.is_empty() { None } else { Some(token) };
        self.hf_downloads.enqueue(model_id, &file, token_opt);
        self.hf_downloads.panel_open = true;
        cx.notify();

        // 2. 同時ダウンロード数を最大 3 に制限
        const MAX_CONCURRENT_DOWNLOADS: u32 = 3;
        let prev = self
            .hf_downloads
            .worker_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if prev >= MAX_CONCURRENT_DOWNLOADS {
            self.hf_downloads
                .worker_count
                .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            return;
        }

        // 3. ワーカー + 進捗ポーリングを起動（最大 3 並列）
        let worker_counter = self.hf_downloads.worker_count.clone();
        let tx = self.hf_downloads.tx.clone();
        cx.spawn(async move |app: WeakEntity<AppView>, cx: &mut AsyncApp| {
            loop {
                // 次のキュー済みタスクを取り出す
                let next_task = cx
                    .update(|ecx| {
                        app.update(ecx, |this: &mut AppView, _| {
                            this.hf_downloads.next_queued_task()
                        })
                        .ok()
                        .flatten()
                    })
                    .ok()
                    .flatten();

                let Some(task) = next_task else {
                    // キュー空 → ワーカー終了
                    break;
                };

                // ダウンロード実行を別スレッドに投げ、同スレッドで進捗 drain
                let tx_c = tx.clone();
                let task_c = task.clone();
                let token_inner = task.hf_token.clone();
                cx.background_executor()
                    .spawn(async move {
                        hf_discover::run_download(task_c, token_inner, tx_c);
                    })
                    .detach();

                // このタスクが完了するまで 300ms ごとに drain
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(300))
                        .await;
                    let finished = cx
                        .update(|ecx| {
                            app.update(ecx, |this: &mut AppView, cx| {
                                let events = this.hf_downloads.drain_progress();
                                let mut any = false;
                                for ev in events {
                                    any = true;
                                    if let hf_discover::DownloadProgress::Completed {
                                        final_path,
                                        ..
                                    } = ev
                                    {
                                        if !this
                                            .settings_model_paths
                                            .iter()
                                            .any(|p| p == &final_path)
                                        {
                                            this.settings_model_paths.push(final_path);
                                            this.persist_local_llm_prefs();
                                        }
                                    }
                                }
                                if any {
                                    cx.notify();
                                }
                                // この特定タスクが終わったか
                                this.hf_downloads
                                    .tasks
                                    .iter()
                                    .find(|t| t.id == task.id)
                                    .map(|t| {
                                        !matches!(
                                            t.status,
                                            hf_discover::DownloadStatus::Queued
                                                | hf_discover::DownloadStatus::InProgress
                                        )
                                    })
                                    .unwrap_or(true)
                            })
                            .ok()
                            .unwrap_or(true)
                        })
                        .ok()
                        .unwrap_or(true);
                    if finished {
                        break;
                    }
                }
            }
            // ワーカー終了 — カウンタをデクリメント
            worker_counter.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        })
        .detach();
    }

    pub(crate) fn hf_cancel_download(&mut self, id: u64, cx: &mut Context<Self>) {
        self.hf_downloads.cancel(id);
        cx.notify();
    }

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
