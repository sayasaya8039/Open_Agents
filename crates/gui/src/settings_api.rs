//! API キー管理（プロバイダーカタログ・貼り付け・クリア・表示切替）と API サーバー設定。

use gpui::prelude::FluentBuilder;
use gpui::*;

use crate::api_key_prefs;
use crate::api_server;
use crate::chat_client;
use crate::colors::*;
use crate::model_prefs;

use super::AppView;

impl AppView {
    // ============================================================
    // API キー管理
    // ============================================================

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
                                    .text_size(px(TYPE_CAPTION1))
                                    .text_color(hex(TEXT_PRIMARY))
                                    .child(title),
                            )
                            .when(has_key, |d| {
                                d.child(
                                    div()
                                        .text_size(px(TYPE_CAPTION1))
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
                                        .text_size(px(TYPE_CAPTION2))
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
                                    .text_size(px(TYPE_CAPTION2))
                                    .text_color(hex(TEXT_MUTED))
                                    .child(tag),
                            )
                            .when_some(env_name_opt, |d, name| {
                                let label: SharedString =
                                    format!("← ${name}").into();
                                d.child(
                                    div()
                                        .text_size(px(TYPE_CAPTION2))
                                        .text_color(hex(TEXT_DIM))
                                        .child(label),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(TYPE_CAPTION2))
                            .text_color(hex(TEXT_DIM))
                            .child(kind_line),
                    )
                    .child(
                        div()
                            .text_size(px(TYPE_CAPTION2))
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
                            .text_size(px(TYPE_CAPTION2))
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
                            .text_size(px(TYPE_CAPTION2))
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
                                .text_size(px(TYPE_CAPTION2))
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
                                .text_size(px(TYPE_CAPTION2))
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
    // API サーバー管理
    // ============================================================

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
                                    .text_size(px(TYPE_CAPTION2))
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
                                    .text_size(px(TYPE_BODY))
                                    .text_color(hex(TEXT_PRIMARY))
                                    .child("ポート"),
                            )
                            .child(
                                div()
                                    .text_size(px(TYPE_CAPTION1))
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
                                    .text_size(px(TYPE_BODY))
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
                                            .text_size(px(TYPE_CAPTION2))
                                            .text_color(hex(TEXT_SECONDARY))
                                            .child(full_base_url.clone()),
                                    )
                                    .child(
                                        div()
                                            .px(px(6.))
                                            .py(px(2.))
                                            .rounded(px(4.))
                                            .bg(hex(CONTROL_BG))
                                            .text_size(px(TYPE_CAPTION2))
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
                                    .text_size(px(TYPE_BODY))
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
                                            .text_size(px(TYPE_CAPTION2))
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
                                            .text_size(px(TYPE_CAPTION2))
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
                                            .text_size(px(TYPE_CAPTION2))
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
                                    .text_size(px(TYPE_BODY))
                                    .text_color(hex(TEXT_PRIMARY))
                                    .child("ステータス"),
                            )
                            .child(
                                div()
                                    .text_size(px(TYPE_CAPTION1))
                                    .text_color(hex(status_color))
                                    .child(status_label),
                            ),
                    ),
            )
    }

    // ============================================================
    // チャットモデル Paste / Clear
    // ============================================================

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

    // ============================================================
    // チャットモデル ステータス
    // ============================================================

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
}
