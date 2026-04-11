#![recursion_limit = "1024"]

mod api_key_prefs;
mod api_server;
mod chat_client;
mod chat_composer;
mod chat_markdown;
mod chat_page;
mod chat_session;
mod colors;
mod discover_page;
mod editor;
mod hf_discover;
pub mod i18n;
mod llama_cpp_chat;
mod llama_cpp_runtime;
mod model_prefs;
#[cfg(any(test, feature = "test-support"))]
mod native_chat;
mod project_explorer;
mod reasoning;
mod session_title_editor;
mod settings_api;
mod settings_general;
mod settings_hf;
mod settings_llama;
mod settings_model;
mod settings_ui;
mod workspace_prefs;

use gpui::prelude::FluentBuilder;
use gpui::*;
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use project_explorer::{
    absolute_path, default_expanded_set, default_sample_tree, expanded_first_level,
    path_to_segments, prune_expanded, read_tree_from_disk, unique_child_name, TreeNode,
};

use colors::*;

pub(crate) fn human_readable_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let bytes_f = bytes as f64;
    if bytes_f >= GB {
        format!("{:.1} GB", bytes_f / GB)
    } else if bytes_f >= MB {
        format!("{:.1} MB", bytes_f / MB)
    } else if bytes_f >= KB {
        format!("{:.1} KB", bytes_f / KB)
    } else {
        format!("{bytes} B")
    }
}

fn chat_history_for_api(messages: &[ChatMsg]) -> Vec<(String, String)> {
    let first_user_index = messages
        .iter()
        .position(|message| message.role == "user")
        .unwrap_or(messages.len());

    messages[first_user_index..]
        .iter()
        .map(|message| (message.role.clone(), message.content.clone()))
        .collect()
}

fn chat_runtime_identity_instruction(
    chat_prefs: &model_prefs::ChatPrefs,
    settings_model_paths: &[PathBuf],
) -> String {
    let runtime = match chat_prefs.source {
        model_prefs::ChatInferenceSource::Api => {
            let model = chat_prefs.api_model.trim();
            if model.is_empty() {
                "クラウド API のプロバイダ既定モデル".to_string()
            } else {
                format!("クラウド API の `{model}`")
            }
        }
        model_prefs::ChatInferenceSource::Local => {
            let model = chat_prefs.ollama_model.trim();
            format!("Ollama の `{model}`")
        }
        model_prefs::ChatInferenceSource::LocalWeights => {
            if settings_model_paths.is_empty() {
                "ローカル GGUF/ONNX（モデル未登録）".to_string()
            } else {
                let index = chat_prefs
                    .local_model_index
                    .min(settings_model_paths.len().saturating_sub(1));
                let name = settings_model_paths[index]
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("ローカルモデル");
                format!("ローカル GGUF/ONNX の `{name}`")
            }
        }
    };

    format!(
        "現在このチャットで実際に使っている推論先は {runtime} です。存在しない別モデル、隠れたマルチモデル切替、未設定のオーケストレーションを名乗ってはいけません。モデル名を聞かれたら、この設定に基づいて簡潔に答えてください。"
    )
}

#[cfg(test)]
mod tests {
    use super::{
        apply_local_stream_completion, chat_history_for_api, chat_runtime_identity_instruction,
        human_readable_size, ChatMsg, ChatMsgMetrics, ModelFormat,
    };
    use std::path::{Path, PathBuf};

    #[test]
    fn detects_model_format_from_extension_case_insensitively() {
        assert_eq!(
            ModelFormat::from_path(Path::new("model.gguf")),
            ModelFormat::Gguf
        );
        assert_eq!(
            ModelFormat::from_path(Path::new("model.ONNX")),
            ModelFormat::Onnx
        );
    }

    #[test]
    fn unknown_model_format_when_extension_is_not_supported() {
        assert_eq!(
            ModelFormat::from_path(Path::new("model.bin")),
            ModelFormat::Unknown
        );
        assert_eq!(
            ModelFormat::from_path(Path::new("model")),
            ModelFormat::Unknown
        );
    }

    #[test]
    fn formats_human_readable_sizes() {
        assert_eq!(human_readable_size(999), "999 B");
        assert_eq!(human_readable_size(1024), "1.0 KB");
        assert_eq!(human_readable_size(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn runtime_identity_instruction_mentions_selected_api_model() {
        let chat = crate::model_prefs::ChatPrefs {
            source: crate::model_prefs::ChatInferenceSource::Api,
            api_model: "grok-4.20-0309-reasoning".into(),
            ..Default::default()
        };

        let text = chat_runtime_identity_instruction(&chat, &[]);
        assert!(text.contains("grok-4.20-0309-reasoning"));
        assert!(text.contains("別モデル"));
    }

    #[test]
    fn runtime_identity_instruction_mentions_selected_local_weights_model() {
        let chat = crate::model_prefs::ChatPrefs {
            source: crate::model_prefs::ChatInferenceSource::LocalWeights,
            local_model_index: 0,
            ..Default::default()
        };
        let models = vec![PathBuf::from("D:/models/gemma-4-q4.gguf")];

        let text = chat_runtime_identity_instruction(&chat, &models);
        assert!(text.contains("gemma-4-q4.gguf"));
        assert!(text.contains("実際に使っている推論先"));
    }

    #[test]
    fn api_history_skips_leading_bootstrap_assistant_messages() {
        let messages = vec![
            ChatMsg {
                role: "assistant".into(),
                content: "こんにちは！Open Agents AIコーディングアシスタントです。".into(),
                thinking: None,
                metrics: None,
            },
            ChatMsg {
                role: "user".into(),
                content: "こんにちは".into(),
                thinking: None,
                metrics: None,
            },
            ChatMsg {
                role: "assistant".into(),
                content: "こんにちは！".into(),
                thinking: None,
                metrics: None,
            },
        ];

        let api_history = chat_history_for_api(&messages);
        assert_eq!(api_history.len(), 2);
        assert_eq!(api_history[0].0, "user");
        assert_eq!(api_history[0].1, "こんにちは");
    }

    #[test]
    fn stream_completion_keeps_streamed_text_and_merges_metrics() {
        let mut msg = ChatMsg {
            role: "assistant".into(),
            content: "生成済み本文".into(),
            thinking: Some("生成済み思考".into()),
            metrics: Some(ChatMsgMetrics {
                model_label: Some("gemma-4-e4b-it".into()),
                ..Default::default()
            }),
        };

        apply_local_stream_completion(
            &mut msg,
            crate::llama_cpp_chat::LlamaCppChatResponse {
                content: "最終本文".into(),
                thinking: Some("最終思考".into()),
                metrics: Some(ChatMsgMetrics {
                    completion_tokens: Some(66),
                    tokens_per_second: Some(77.24),
                    elapsed_ms: Some(430),
                    stop_reason: Some("EOSトークン検出".into()),
                    ..Default::default()
                }),
            },
            true,
            true,
        );

        assert_eq!(msg.content, "生成済み本文");
        assert_eq!(msg.thinking.as_deref(), Some("生成済み思考"));
        let metrics = msg.metrics.expect("metrics should be merged");
        assert_eq!(metrics.model_label.as_deref(), Some("gemma-4-e4b-it"));
        assert_eq!(metrics.completion_tokens, Some(66));
        assert_eq!(metrics.stop_reason.as_deref(), Some("EOSトークン検出"));
        assert_eq!(metrics.elapsed_ms, Some(430));
        assert_eq!(metrics.tokens_per_second, Some(77.24));
    }

    #[test]
    fn stream_completion_falls_back_to_final_response_when_no_deltas_arrived() {
        let mut msg = ChatMsg {
            role: "assistant".into(),
            content: String::new(),
            thinking: None,
            metrics: None,
        };

        apply_local_stream_completion(
            &mut msg,
            crate::llama_cpp_chat::LlamaCppChatResponse {
                content: "最終本文".into(),
                thinking: Some("最終思考".into()),
                metrics: Some(ChatMsgMetrics {
                    completion_tokens: Some(12),
                    elapsed_ms: Some(250),
                    ..Default::default()
                }),
            },
            false,
            false,
        );

        assert_eq!(msg.content, "最終本文");
        assert_eq!(msg.thinking.as_deref(), Some("最終思考"));
        let metrics = msg.metrics.expect("metrics should be present");
        assert_eq!(metrics.completion_tokens, Some(12));
        assert_eq!(metrics.elapsed_ms, Some(250));
    }

    #[cfg(feature = "test-support")]
    mod chat_submit_tests {
        use super::super::{chat_composer, install_chat_submit_fallback};
        use gpui::{
            div, AppContext, Context, Entity, IntoElement, KeyDownEvent, Keystroke, ParentElement,
            Render, Styled, TestAppContext, Window,
        };

        struct ChatSubmitHarness {
            composer: Entity<chat_composer::ChatComposer>,
            submit_count: usize,
        }

        impl ChatSubmitHarness {
            fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
                let composer =
                    cx.new(|ecx| chat_composer::ChatComposer::new(ecx, i18n::placeholder_message()));
                composer.read(cx).focus(window);
                cx.subscribe(
                    &composer,
                    |this: &mut Self, _, _: &chat_composer::SubmitChat, cx| {
                        this.submit_count += 1;
                        cx.notify();
                    },
                )
                .detach();

                Self {
                    composer,
                    submit_count: 0,
                }
            }
        }

        impl Render for ChatSubmitHarness {
            fn render(
                &mut self,
                _window: &mut Window,
                _cx: &mut Context<Self>,
            ) -> impl IntoElement {
                div().size_full().child(self.composer.clone())
            }
        }

        #[gpui::test]
        async fn enter_submit_fallback_remains_active(cx: &mut TestAppContext) {
            cx.update(|cx| install_chat_submit_fallback(cx));
            let (view, cx) = cx.add_window_view(ChatSubmitHarness::new);
            cx.update(|window, cx| {
                view.read(cx).composer.read(cx).focus(window);
                window.activate_window();
            });

            cx.simulate_input("hello");
            cx.simulate_keystrokes("enter");

            let submit_count = cx.update(|_, cx| view.read(cx).submit_count);
            assert_eq!(submit_count, 1);
        }

        #[gpui::test]
        async fn enter_keydown_submits_without_global_fallback(cx: &mut TestAppContext) {
            let (view, cx) = cx.add_window_view(ChatSubmitHarness::new);
            cx.update(|window, cx| {
                view.read(cx).composer.read(cx).focus(window);
                window.activate_window();
            });

            cx.simulate_input("hello");
            cx.simulate_event(KeyDownEvent {
                keystroke: Keystroke::parse("enter").unwrap(),
                is_held: false,
            });

            let submit_count = cx.update(|_, cx| view.read(cx).submit_count);
            assert_eq!(submit_count, 1);
        }

        #[gpui::test]
        async fn shift_enter_keydown_inserts_newline_without_submit(cx: &mut TestAppContext) {
            let (view, cx) = cx.add_window_view(ChatSubmitHarness::new);
            cx.update(|window, cx| {
                view.read(cx).composer.read(cx).focus(window);
                window.activate_window();
            });

            cx.simulate_input("hello");
            cx.simulate_event(KeyDownEvent {
                keystroke: Keystroke::parse("shift-enter").unwrap(),
                is_held: false,
            });

            let (submit_count, text) = cx.update(|_, cx| {
                (
                    view.read(cx).submit_count,
                    view.read(cx).composer.read(cx).text().to_string(),
                )
            });
            assert_eq!(submit_count, 0);
            assert_eq!(text, "hello\n");
        }
    }
}

// ============================================================
// State
// ============================================================

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Page {
    Chat,
    Settings,
    Terminal,
    Discover,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ModelFormat {
    Gguf,
    Onnx,
    Unknown,
}

impl ModelFormat {
    fn from_path(path: &Path) -> Self {
        match path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref()
        {
            Some("gguf") => Self::Gguf,
            Some("onnx") => Self::Onnx,
            Some(_) => Self::Unknown,
            None => Self::Unknown,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Gguf => "GGUF",
            Self::Onnx => "ONNX",
            Self::Unknown => "不明",
        }
    }
}

use chat_session::{ChatMsg, ChatMsgMetrics};

pub(crate) enum ChatStreamEvent {
    ContentDelta(String),
    ThinkingDelta(String),
    Complete(llama_cpp_chat::LlamaCppChatResponse),
    Error(String),
}


use reasoning::*;


/// 設定画面のモデルパラメータ行（± で調整、`model_prefs` に保存）
#[derive(Clone, Copy)]
pub(crate) enum ModelParamAdjustKind {
    Temperature,
    MaxOutputTokens,
    ContextLength,
}

#[derive(Clone, Copy)]
pub(crate) enum HardwareParamAdjustKind {
    GpuLayers,
    NThreads,
    BatchSize,
}

#[derive(Clone, Copy)]
pub(crate) enum AiToggleKind {
    AutoComplete,
    CodeSuggestions,
    StreamingResponses,
}

pub(crate) struct AppView {
    pub(crate) page: Page,
    /// チャットセッション管理（マルチセッション + 永続化）
    pub(crate) session_store: chat_session::SessionStore,
    /// Figma Chat ヘッダー「思考を表示」トグル
    pub(crate) chat_show_thinking: bool,
    /// 設定で読み込んだローカル LLM（下に追加・永続化）
    pub(crate) settings_model_paths: Vec<PathBuf>,
    /// 永続化済みローカル LLM 推論パラメータ（Temperature / max tokens / context）
    pub(crate) model_params: model_prefs::ModelParams,
    /// GPU スレッド・バッチ等（`model_params.json` の `hardware` と同期）
    pub(crate) hardware_params: model_prefs::HardwareParams,
    /// エディタのテーマ・フォント・行番号（`appearance` と同期）
    pub(crate) appearance_prefs: model_prefs::AppearancePrefs,
    /// AI 補助機能の ON/OFF（`ai` と同期）
    pub(crate) ai_prefs: model_prefs::AiPrefs,
    /// Chat の推論先・モデル ID（`model_params.json` の `chat`）
    pub(crate) chat_prefs: model_prefs::ChatPrefs,
    /// 外部 API キー（`api_keys.json`）
    pub(crate) api_keys: api_key_prefs::ApiKeyPrefs,
    /// 設定画面での各カタログ行のプレーン表示（永続化しない、`PROVIDER_CATALOG` と同順）
    pub(crate) api_key_reveal: Vec<bool>,
    /// プロバイダから取得したモデルID一覧（キャッシュ）: (provider_id, label, models)
    pub(crate) fetched_models: Vec<(String, String, Vec<String>)>,
    /// モデル取得中フラグ
    pub(crate) fetching_models: bool,
    /// 開いているワークスペースのルート（Zed worktree root）
    pub(crate) workspace_root: PathBuf,
    /// 仮想ファイルツリー
    pub(crate) file_tree: TreeNode,
    /// 展開中ディレクトリ（パスごと）— Zed `expanded_dir_ids` 相当
    pub(crate) explorer_expanded: HashSet<Vec<String>>,
    /// フォーカス/選択行
    pub(crate) explorer_selection: Option<Vec<String>>,
    pub(crate) chat_composer: Entity<chat_composer::ChatComposer>,
    /// Chat メッセージスクロールハンドル
    pub(crate) chat_scroll: ScrollHandle,
    /// Chat API リクエスト送信中（再送信ガード）
    pub(crate) chat_pending: bool,
    /// 三点メニューを開いているセッション
    pub(crate) open_session_menu_id: Option<u64>,
    /// セッション名変更中の対象セッション
    pub(crate) renaming_session_id: Option<u64>,
    /// セッション名変更用エディタ
    pub(crate) session_title_editor: Option<Entity<session_title_editor::SessionTitleEditor>>,
    /// backend ごとの同梱 llama.cpp runtime 状態
    pub(crate) llama_cpp_runtime_statuses: Vec<llama_cpp_runtime::BundledLlamaRuntimeStatus>,
    /// GitHub Releases の更新通知
    pub(crate) llama_cpp_update_notice: Option<llama_cpp_runtime::LlamaCppUpdateNotice>,
    /// GPU 自動検出結果のサマリ（設定画面で表示）
    pub(crate) gpu_profile_summary: String,
    /// Hugging Face モデル検索・ダウンロード状態（Discover ページ専用 — Box で AppView のスタックサイズ削減）
    pub(crate) hf_state: Box<hf_discover::HuggingFaceSearchState>,
    /// Discover ページの検索バー（ChatComposer を流用）
    pub(crate) hf_search_composer: Entity<chat_composer::ChatComposer>,
    /// ダウンロードマネージャ
    pub(crate) hf_downloads: hf_discover::DownloadManager,
    /// 内蔵 API サーバー設定（永続化）
    pub(crate) api_server_prefs: model_prefs::ApiServerPrefs,
    /// 稼働中の API サーバーインスタンス
    pub(crate) api_server: Option<api_server::ApiServer>,
    /// API サーバー起動エラーメッセージ
    pub(crate) api_server_error: Option<String>,
}

impl AppView {
    /// エクスプローラ: 新規ファイル・フォルダの親ディレクトリ（相対セグメント）
    fn explorer_target_parent_segments(&self) -> Vec<String> {
        let Some(sel) = &self.explorer_selection else {
            return Vec::new();
        };
        let abs = absolute_path(&self.workspace_root, sel);
        if abs.is_dir() {
            return sel.clone();
        }
        if sel.is_empty() {
            return Vec::new();
        }
        sel[..sel.len().saturating_sub(1)].to_vec()
    }

    fn explorer_reload_from_disk(&mut self) {
        match read_tree_from_disk(&self.workspace_root) {
            Ok(tree) => {
                self.file_tree = tree;
                prune_expanded(&self.workspace_root, &mut self.explorer_expanded);
            }
            Err(e) => eprintln!("explorer: ツリー更新に失敗 {e}"),
        }
    }

    fn explorer_new_file(&mut self, cx: &mut Context<Self>) {
        let parent_rel = self.explorer_target_parent_segments();
        let parent_abs = absolute_path(&self.workspace_root, &parent_rel);
        if !parent_abs.is_dir() {
            eprintln!("explorer: 親フォルダが無効です");
            return;
        }
        let name = unique_child_name(&parent_abs, "New File");
        let path = parent_abs.join(&name);
        match fs::File::create(&path) {
            Ok(mut f) => {
                let _ = f.write_all(b"\n");
                self.explorer_reload_from_disk();
                self.explorer_expanded.insert(parent_rel.clone());
                let segs = path_to_segments(&self.workspace_root, &path);
                self.explorer_selection = Some(segs);
                cx.notify();
            }
            Err(e) => eprintln!("explorer: 新規ファイル {e}"),
        }
    }

    fn explorer_new_folder(&mut self, cx: &mut Context<Self>) {
        let parent_rel = self.explorer_target_parent_segments();
        let parent_abs = absolute_path(&self.workspace_root, &parent_rel);
        if !parent_abs.is_dir() {
            eprintln!("explorer: 親フォルダが無効です");
            return;
        }
        let name = unique_child_name(&parent_abs, "New Folder");
        let path = parent_abs.join(&name);
        match fs::create_dir(&path) {
            Ok(()) => {
                self.explorer_reload_from_disk();
                self.explorer_expanded.insert(parent_rel.clone());
                let segs = path_to_segments(&self.workspace_root, &path);
                self.explorer_expanded.insert(segs.clone());
                self.explorer_selection = Some(segs);
                cx.notify();
            }
            Err(e) => eprintln!("explorer: 新規フォルダ {e}"),
        }
    }

    /// ワークスペースルートを差し替え、ツリー再読込と `last_workspace.txt` へ保存する。
    fn apply_workspace_root(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let path = path.canonicalize().unwrap_or(path);
        if !path.is_dir() {
            eprintln!("workspace: 無効なパス {}", path.display());
            return;
        }
        self.workspace_root = path;
        workspace_prefs::save_last_workspace(&self.workspace_root);
        match read_tree_from_disk(&self.workspace_root) {
            Ok(tree) => {
                self.file_tree = tree;
                self.explorer_expanded.clear();
            }
            Err(e) => {
                eprintln!("explorer: フォルダ読込 {e}");
                self.file_tree = TreeNode::dir("", vec![]);
                self.explorer_expanded.clear();
            }
        }
        self.explorer_selection = None;
        cx.notify();
    }

    /// EXPLORER 右端: フォルダを開く（ワークスペースルートを差し替え）
    fn explorer_open_folder_dialog(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |app: WeakEntity<AppView>, cx: &mut AsyncApp| {
            if let Ok(Ok(Some(paths))) = receiver.await {
                if let Some(path) = paths.first() {
                    let path = path.clone();
                    let _ = cx.update(|ecx| {
                        let _ = app.update(ecx, |this: &mut AppView, ecx| {
                            this.apply_workspace_root(path, ecx);
                        });
                    });
                }
            }
        })
        .detach();
    }

    fn settings_open_model_file_dialog(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |app: WeakEntity<AppView>, cx: &mut AsyncApp| {
            if let Ok(Ok(Some(paths))) = receiver.await {
                if let Some(path) = paths.first() {
                    let path = path.clone();
                    let _ = cx.update(|ecx| {
                        let _ = app.update(ecx, |this: &mut AppView, cx| {
                            this.settings_add_model_file(path, cx);
                        });
                    });
                }
            }
        })
        .detach();
    }

    /// 一覧末尾へ追加（同一パスは無視）。`model_params.json` に保存する。
    fn settings_add_model_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let path = path.canonicalize().unwrap_or(path);
        if !self.settings_model_paths.iter().any(|p| p == &path) {
            self.settings_model_paths.push(path);
            self.persist_local_llm_prefs();
        }
        cx.notify();
    }

    // ── llama-server プリウォーム（Chat画面切替時にバックグラウンド起動）──

    fn prewarm_llama_server(&mut self, cx: &mut Context<Self>) {
        // LocalWeights でなければ不要
        if self.chat_prefs.source != model_prefs::ChatInferenceSource::LocalWeights {
            return;
        }
        let paths = self.settings_model_paths.clone();
        let idx = self
            .chat_prefs
            .local_model_index
            .min(paths.len().saturating_sub(1));
        let Some(path) = paths.get(idx).cloned() else {
            return;
        };
        let ctx = self.model_params.context_length;
        let hw = self.hardware_params.clone();
        // 既にwarmならスキップ
        if llama_cpp_chat::server_ready_for(&path, ctx, &hw) {
            return;
        }
        eprintln!("llama.cpp: prewarm — バックグラウンドでサーバを事前起動します");
        cx.spawn(async move |_this, _cx| {
            let _ = smol::unblock(
                move || match llama_cpp_chat::ensure_server(&path, ctx, &hw) {
                    Ok((url, model)) => eprintln!("llama.cpp: prewarm 完了 — {url} ({model})"),
                    Err(e) => eprintln!("llama.cpp: prewarm 失敗 — {e}"),
                },
            )
            .await;
        })
        .detach();
    }

    // ── Chat セッション操作 ──

    /// ローカルモデルを次のインデックスに切替
    fn cycle_local_model(&mut self, cx: &mut Context<Self>) {
        if self.settings_model_paths.is_empty() {
            return;
        }
        self.chat_prefs.local_model_index =
            (self.chat_prefs.local_model_index + 1) % self.settings_model_paths.len();
        self.persist_local_llm_prefs();
        // サーバキャッシュをクリア（次回送信時に新しいモデルで起動）
        llama_cpp_chat::cleanup_orphan_servers();
        cx.notify();
    }

    fn chat_message_model_label(&self) -> String {
        match self.chat_prefs.source {
            model_prefs::ChatInferenceSource::Api => {
                let model = self.chat_prefs.api_model.trim();
                if model.is_empty() {
                    "cloud-api".to_string()
                } else {
                    model.to_string()
                }
            }
            model_prefs::ChatInferenceSource::Local => self.chat_prefs.ollama_model.trim().into(),
            model_prefs::ChatInferenceSource::LocalWeights => {
                if self.settings_model_paths.is_empty() {
                    "local-gguf".to_string()
                } else {
                    let index = self
                        .chat_prefs
                        .local_model_index
                        .min(self.settings_model_paths.len() - 1);
                    self.settings_model_paths[index]
                        .file_stem()
                        .and_then(|value| value.to_str())
                        .unwrap_or("local-gguf")
                        .to_string()
                }
            }
        }
    }

    fn chat_new_session(&mut self, cx: &mut Context<Self>) {
        self.session_store.new_session();
        self.chat_pending = false;
        self.clear_session_sidebar_state();
        chat_session::save_sessions(&self.session_store);
        cx.notify();
    }

    fn chat_switch_session(&mut self, id: u64, cx: &mut Context<Self>) {
        self.session_store.switch_to(id);
        self.chat_pending = false;
        self.clear_session_sidebar_state();
        cx.notify();
    }

    fn chat_delete_section(&mut self, label: &'static str, cx: &mut Context<Self>) {
        self.session_store.delete_group(label);
        self.chat_pending = false;
        self.clear_session_sidebar_state();
        chat_session::save_sessions(&self.session_store);
        cx.notify();
    }

    fn clear_session_sidebar_state(&mut self) {
        self.open_session_menu_id = None;
        self.renaming_session_id = None;
        self.session_title_editor = None;
    }

    fn chat_toggle_session_menu(&mut self, id: u64, cx: &mut Context<Self>) {
        self.open_session_menu_id = match self.open_session_menu_id {
            Some(current) if current == id => None,
            _ => Some(id),
        };
        if self.renaming_session_id != Some(id) {
            self.renaming_session_id = None;
            self.session_title_editor = None;
        }
        cx.notify();
    }

    fn chat_begin_session_rename(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self
            .session_store
            .sessions
            .iter()
            .find(|session| session.id == id)
        else {
            return;
        };
        let editor =
            cx.new(|ecx| session_title_editor::SessionTitleEditor::new(ecx, session.title.clone()));
        cx.subscribe(
            &editor,
            |this: &mut AppView, _, _: &session_title_editor::SubmitRenameEvent, cx| {
                this.chat_commit_session_rename(cx);
            },
        )
        .detach();
        cx.subscribe(
            &editor,
            |this: &mut AppView, _, _: &session_title_editor::CancelRenameEvent, cx| {
                this.chat_cancel_session_rename(cx);
            },
        )
        .detach();
        self.open_session_menu_id = None;
        self.renaming_session_id = Some(id);
        self.session_title_editor = Some(editor.clone());
        editor.read(cx).focus(window);
        cx.notify();
    }

    fn chat_commit_session_rename(&mut self, cx: &mut Context<Self>) {
        let (Some(id), Some(editor)) =
            (self.renaming_session_id, self.session_title_editor.clone())
        else {
            return;
        };
        let new_title = editor.read(cx).text().to_string();
        if self.session_store.rename_session(id, &new_title) {
            chat_session::save_sessions(&self.session_store);
            self.clear_session_sidebar_state();
            cx.notify();
        }
    }

    fn chat_cancel_session_rename(&mut self, cx: &mut Context<Self>) {
        self.clear_session_sidebar_state();
        cx.notify();
    }

    fn chat_duplicate_session(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.session_store.duplicate_session(id).is_some() {
            self.chat_pending = false;
            self.clear_session_sidebar_state();
            chat_session::save_sessions(&self.session_store);
            cx.notify();
        }
    }

    fn chat_delete_session(&mut self, id: u64, cx: &mut Context<Self>) {
        self.session_store.delete_session(id);
        self.chat_pending = false;
        self.clear_session_sidebar_state();
        chat_session::save_sessions(&self.session_store);
        cx.notify();
    }

    fn chat_export_session(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(json) = self.session_store.export_session_json(id) else {
            return;
        };
        let start_dir = chat_session::sessions_file_path()
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let default_name = format!("session-{id}.json");
        let receiver = cx.prompt_for_new_path(&start_dir, Some(default_name.as_str()));
        cx.spawn(async move |_app, _cx| {
            if let Ok(Ok(Some(path))) = receiver.await {
                let _ = smol::unblock(move || fs::write(path, json)).await;
            }
        })
        .detach();
    }

    fn chat_show_session_file_in_explorer(&mut self) {
        let path = chat_session::sessions_file_path();
        #[cfg(windows)]
        let _ = Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn();
        #[cfg(not(windows))]
        let _ = Command::new("xdg-open")
            .arg(path.parent().unwrap_or_else(|| Path::new(".")))
            .spawn();
    }

    fn chat_copy_message(&mut self, content: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(content));
    }

    fn chat_delete_message(&mut self, message_index: usize, cx: &mut Context<Self>) {
        if let Some(session) = self.session_store.active_mut() {
            if message_index < session.messages.len() {
                session.messages.remove(message_index);
                session.touch();
                chat_session::save_sessions(&self.session_store);
                cx.notify();
            }
        }
    }

    fn chat_regenerate_last_reply(&mut self, cx: &mut Context<Self>) {
        if self.chat_pending {
            return;
        }
        let can_regenerate = self
            .session_store
            .active()
            .and_then(|session| {
                let last = session.messages.last()?;
                if last.role != "assistant" {
                    return None;
                }
                session
                    .messages
                    .iter()
                    .rev()
                    .skip(1)
                    .find(|message| message.role == "user")
                    .map(|_| ())
            })
            .is_some();
        if !can_regenerate {
            return;
        }
        if let Some(session) = self.session_store.active_mut() {
            let _ = session.messages.pop();
            session.touch();
        }
        self.submit_chat_request(None, cx);
    }

    /// Chat のシステムメッセージを構築（CoT 指示を含む）
    fn build_chat_system_message(&self) -> String {
        let cot = self.ai_prefs.cot_mode.system_instruction();
        format!(
            "あなたはAIコーディングアシスタントです。\n\n{}{}",
            chat_runtime_identity_instruction(&self.chat_prefs, &self.settings_model_paths),
            cot,
        )
    }

    fn submit_chat_request(&mut self, new_user_text: Option<String>, cx: &mut Context<Self>) {
        let model_label = self.chat_message_model_label();
        let system_msg = self.build_chat_system_message();

        let session_messages = self
            .session_store
            .active()
            .map(|s| &s.messages[..])
            .unwrap_or(&[]);
        let mut api_messages: Vec<(String, String)> =
            std::iter::once(("system".into(), system_msg))
                .chain(chat_history_for_api(session_messages).into_iter())
                .collect();
        if let Some(text) = &new_user_text {
            api_messages.push(("user".into(), text.clone()));
        }

        if let Some(session) = self.session_store.active_mut() {
            if let Some(text) = new_user_text {
                session.messages.push(ChatMsg {
                    role: "user".into(),
                    content: text,
                    thinking: None,
                    metrics: None,
                });
                if session.messages.iter().filter(|m| m.role == "user").count() == 1 {
                    session.auto_title();
                }
            }
            let placeholder: String = match self.chat_prefs.source {
                model_prefs::ChatInferenceSource::LocalWeights => {
                    let warm = self
                        .settings_model_paths
                        .get(
                            self.chat_prefs
                                .local_model_index
                                .min(self.settings_model_paths.len().saturating_sub(1)),
                        )
                        .map(|path| {
                            llama_cpp_chat::server_ready_for(
                                path,
                                self.model_params.context_length,
                                &self.hardware_params,
                            )
                        })
                        .unwrap_or(false);
                    if warm {
                        "GGUF 応答を生成中です… 既に起動済みの llama.cpp サーバを再利用しています。"
                            .into()
                    } else {
                        "GGUF モデルを準備中です… 内蔵 llama.cpp サーバの初回起動には時間がかかります。大型 BF16/F16 モデルでは量子化 GGUF を推奨します。".into()
                    }
                }
                _ => "応答を待っています…".into(),
            };
            session.messages.push(ChatMsg {
                role: "assistant".into(),
                content: placeholder,
                thinking: None,
                metrics: Some(ChatMsgMetrics {
                    model_label: Some(model_label),
                    ..ChatMsgMetrics::default()
                }),
            });
            session.touch();
        }
        self.chat_pending = true;
        self.chat_scroll.scroll_to_bottom();
        cx.notify();

        let api_keys = self.api_keys.clone();
        let chat_prefs = self.chat_prefs.clone();
        let local_model_paths = self.settings_model_paths.clone();
        let temperature = self.model_params.temperature;
        let max_tokens = self.model_params.max_output_tokens;
        let context_length = self.model_params.context_length;
        let hardware_params = self.hardware_params.clone();
        let streaming_enabled = self.ai_prefs.streaming_responses;
        let self_consistency = self.ai_prefs.self_consistency;
        let tot_mode = self.ai_prefs.tot_mode;
        let react_mode = self.ai_prefs.react_mode;

        match chat_client::resolve_chat_backend(&api_keys, &chat_prefs, &local_model_paths) {
            // ReAct: Reasoning + Acting ループ（ローカル GGUF 専用）
            Ok(chat_client::ChatBackend::LlamaCppLocal { path }) if react_mode.is_enabled() => {
                let max_steps = react_mode.max_steps();
                let hw = hardware_params.clone();
                let (tx, rx) = smol::channel::unbounded::<ChatStreamEvent>();
                std::thread::spawn(move || {
                    run_react_loop(
                        &path,
                        &api_messages,
                        temperature,
                        max_tokens,
                        context_length,
                        &hw,
                        max_steps,
                        &tx,
                    );
                });

                cx.spawn(async move |this, cx| {
                    let mut saw_content_delta = false;
                    while let Ok(event) = rx.recv().await {
                        let done = matches!(
                            event,
                            ChatStreamEvent::Complete(_) | ChatStreamEvent::Error(_)
                        );
                        let _ = cx.update(|app| {
                            let _ = this.update(app, |this: &mut AppView, cx| {
                                if let Some(last) = this
                                    .session_store
                                    .active_mut()
                                    .and_then(|s| s.messages.last_mut())
                                {
                                    if last.role == "assistant" {
                                        match event {
                                            ChatStreamEvent::ContentDelta(delta) => {
                                                if !saw_content_delta {
                                                    last.content.clear();
                                                    saw_content_delta = true;
                                                }
                                                last.content.push_str(&delta);
                                            }
                                            ChatStreamEvent::Complete(reply) => {
                                                this.chat_pending = false;
                                                last.content = reply.content;
                                                last.thinking = reply.thinking;
                                                last.metrics = merge_metrics(
                                                    last.metrics.take(),
                                                    reply.metrics,
                                                );
                                                chat_session::save_sessions(&this.session_store);
                                            }
                                            ChatStreamEvent::Error(err) => {
                                                this.chat_pending = false;
                                                last.content = format!("エラー: {err}");
                                                chat_session::save_sessions(&this.session_store);
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                this.chat_scroll.scroll_to_bottom();
                                cx.notify();
                            });
                        });
                        if done {
                            break;
                        }
                    }
                })
                .detach();
            }
            // Tree-of-Thoughts: 複数思考経路を探索・評価・合成（ローカル GGUF 専用）
            Ok(chat_client::ChatBackend::LlamaCppLocal { path }) if tot_mode.is_enabled() => {
                let branches = tot_mode.branch_count();
                let hw = hardware_params.clone();
                let (tx, rx) = smol::channel::unbounded::<ChatStreamEvent>();
                std::thread::spawn(move || {
                    run_tree_of_thoughts(
                        &path,
                        &api_messages,
                        temperature,
                        max_tokens,
                        context_length,
                        &hw,
                        branches,
                        &tx,
                    );
                });

                cx.spawn(async move |this, cx| {
                    let mut saw_content_delta = false;
                    while let Ok(event) = rx.recv().await {
                        let done = matches!(
                            event,
                            ChatStreamEvent::Complete(_) | ChatStreamEvent::Error(_)
                        );
                        let _ = cx.update(|app| {
                            let _ = this.update(app, |this: &mut AppView, cx| {
                                if let Some(last) = this
                                    .session_store
                                    .active_mut()
                                    .and_then(|s| s.messages.last_mut())
                                {
                                    if last.role == "assistant" {
                                        match event {
                                            ChatStreamEvent::ContentDelta(delta) => {
                                                if !saw_content_delta {
                                                    last.content.clear();
                                                    saw_content_delta = true;
                                                }
                                                last.content.push_str(&delta);
                                            }
                                            ChatStreamEvent::Complete(reply) => {
                                                this.chat_pending = false;
                                                last.content = reply.content;
                                                last.thinking = reply.thinking;
                                                last.metrics = merge_metrics(
                                                    last.metrics.take(),
                                                    reply.metrics,
                                                );
                                                chat_session::save_sessions(&this.session_store);
                                            }
                                            ChatStreamEvent::Error(err) => {
                                                this.chat_pending = false;
                                                last.content = format!("エラー: {err}");
                                                chat_session::save_sessions(&this.session_store);
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                this.chat_scroll.scroll_to_bottom();
                                cx.notify();
                            });
                        });
                        if done {
                            break;
                        }
                    }
                })
                .detach();
            }
            // Self-Consistency: 複数回推論して多数決（ストリーミング無効で実行）
            Ok(chat_client::ChatBackend::LlamaCppLocal { path })
                if self_consistency.is_enabled() =>
            {
                let vote_count = self_consistency.vote_count();
                let hw = hardware_params.clone();
                let (tx, rx) = smol::channel::unbounded::<ChatStreamEvent>();
                std::thread::spawn(move || {
                    let _ = tx.send_blocking(ChatStreamEvent::ContentDelta(format!(
                        "Self-Consistency: {vote_count} 回の推論を実行中…\n\n"
                    )));
                    let mut responses: Vec<(String, Option<ChatMsgMetrics>)> = Vec::new();
                    for i in 0..vote_count {
                        // 各試行で少し温度を変えてサンプリング多様性を確保
                        let temp = temperature + (i as f32) * 0.05;
                        let result = llama_cpp_chat::complete_llama_cpp_chat_blocking(
                            &path,
                            &api_messages,
                            temp.min(2.0),
                            max_tokens,
                            context_length,
                            &hw,
                        );
                        match result {
                            Ok(reply) => {
                                let _ = tx.send_blocking(ChatStreamEvent::ContentDelta(
                                    format!("✓ 投票 {}/{vote_count} 完了\n", i + 1),
                                ));
                                responses.push((reply.content, reply.metrics));
                            }
                            Err(e) => {
                                let _ = tx.send_blocking(ChatStreamEvent::ContentDelta(
                                    format!("✗ 投票 {}/{vote_count} エラー: {e}\n", i + 1),
                                ));
                            }
                        }
                    }
                    if responses.is_empty() {
                        let _ = tx.send_blocking(ChatStreamEvent::Error(
                            "Self-Consistency: すべての推論が失敗しました".to_string(),
                        ));
                        return;
                    }
                    // 多数決: 各回答を正規化してグループ化し、最多得票の回答を採用
                    let (best_content, best_metrics, votes, total) =
                        majority_vote_select(&responses);
                    let mut final_reply = llama_cpp_chat::LlamaCppChatResponse {
                        content: format!(
                            "{best_content}\n\n---\n*Self-Consistency: {votes}/{total} 票で採用*"
                        ),
                        thinking: None,
                        metrics: best_metrics,
                    };
                    // メトリクスに投票情報を追記
                    if let Some(ref mut m) = final_reply.metrics {
                        m.stop_reason = Some(format!(
                            "Self-Consistency {votes}/{total} 票"
                        ));
                    }
                    let _ = tx.send_blocking(ChatStreamEvent::Complete(final_reply));
                });

                cx.spawn(async move |this, cx| {
                    let mut saw_content_delta = false;
                    while let Ok(event) = rx.recv().await {
                        let done = matches!(
                            event,
                            ChatStreamEvent::Complete(_) | ChatStreamEvent::Error(_)
                        );
                        let _ = cx.update(|app| {
                            let _ = this.update(app, |this: &mut AppView, cx| {
                                if let Some(last) = this
                                    .session_store
                                    .active_mut()
                                    .and_then(|s| s.messages.last_mut())
                                {
                                    if last.role == "assistant" {
                                        match event {
                                            ChatStreamEvent::ContentDelta(delta) => {
                                                if !saw_content_delta {
                                                    last.content.clear();
                                                    saw_content_delta = true;
                                                }
                                                last.content.push_str(&delta);
                                            }
                                            ChatStreamEvent::Complete(reply) => {
                                                this.chat_pending = false;
                                                last.content = reply.content;
                                                last.thinking = reply.thinking;
                                                last.metrics = merge_metrics(
                                                    last.metrics.take(),
                                                    reply.metrics,
                                                );
                                                chat_session::save_sessions(&this.session_store);
                                            }
                                            ChatStreamEvent::Error(err) => {
                                                this.chat_pending = false;
                                                last.content = format!("エラー: {err}");
                                                chat_session::save_sessions(&this.session_store);
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                this.chat_scroll.scroll_to_bottom();
                                cx.notify();
                            });
                        });
                        if done {
                            break;
                        }
                    }
                })
                .detach();
            }
            Ok(chat_client::ChatBackend::LlamaCppLocal { path }) if streaming_enabled => {
                let (tx, rx) = smol::channel::unbounded::<ChatStreamEvent>();
                std::thread::spawn(move || {
                    let result = llama_cpp_chat::stream_llama_cpp_chat_blocking(
                        &path,
                        &api_messages,
                        temperature,
                        max_tokens,
                        context_length,
                        &hardware_params,
                        |delta| {
                            let _ =
                                tx.send_blocking(ChatStreamEvent::ContentDelta(delta.to_string()));
                        },
                        |delta| {
                            let _ =
                                tx.send_blocking(ChatStreamEvent::ThinkingDelta(delta.to_string()));
                        },
                    );
                    match result {
                        Ok(reply) => {
                            let _ = tx.send_blocking(ChatStreamEvent::Complete(reply));
                        }
                        Err(err) => {
                            let _ = tx.send_blocking(ChatStreamEvent::Error(err));
                        }
                    }
                });

                cx.spawn(async move |this, cx| {
                    let mut saw_content_delta = false;
                    let mut saw_thinking_delta = false;
                    while let Ok(event) = rx.recv().await {
                        let done = matches!(
                            event,
                            ChatStreamEvent::Complete(_) | ChatStreamEvent::Error(_)
                        );
                        let _ = cx.update(|app| {
                            let _ = this.update(app, |this: &mut AppView, cx| {
                                if let Some(last) = this
                                    .session_store
                                    .active_mut()
                                    .and_then(|s| s.messages.last_mut())
                                {
                                    if last.role == "assistant" {
                                        match event {
                                            ChatStreamEvent::ContentDelta(delta) => {
                                                if !saw_content_delta {
                                                    last.content.clear();
                                                    saw_content_delta = true;
                                                }
                                                last.content.push_str(&delta);
                                            }
                                            ChatStreamEvent::ThinkingDelta(delta) => {
                                                let thinking =
                                                    last.thinking.get_or_insert_with(String::new);
                                                thinking.push_str(&delta);
                                                saw_thinking_delta = true;
                                            }
                                            ChatStreamEvent::Complete(reply) => {
                                                this.chat_pending = false;
                                                apply_local_stream_completion(
                                                    last,
                                                    reply,
                                                    saw_content_delta,
                                                    saw_thinking_delta,
                                                );
                                                chat_session::save_sessions(&this.session_store);
                                            }
                                            ChatStreamEvent::Error(err) => {
                                                this.chat_pending = false;
                                                last.content = format!("エラー: {err}");
                                                chat_session::save_sessions(&this.session_store);
                                            }
                                        }
                                    }
                                }
                                this.chat_scroll.scroll_to_bottom();
                                cx.notify();
                            });
                        });
                        if done {
                            break;
                        }
                    }
                })
                .detach();
            }
            Ok(chat_client::ChatBackend::LlamaCppLocal { path }) => {
                let hw = hardware_params.clone();
                cx.spawn(async move |this, cx| {
                    let result = smol::unblock(move || {
                        llama_cpp_chat::complete_llama_cpp_chat_blocking(
                            &path,
                            &api_messages,
                            temperature,
                            max_tokens,
                            context_length,
                            &hw,
                        )
                    })
                    .await;

                    let _ = cx.update(|app| {
                        let _ = this.update(app, |this: &mut AppView, cx| {
                            this.chat_pending = false;
                            if let Some(last) = this
                                .session_store
                                .active_mut()
                                .and_then(|s| s.messages.last_mut())
                            {
                                if last.role == "assistant" {
                                    match result {
                                        Ok(reply) => apply_local_chat_response(last, reply),
                                        Err(e) => last.content = format!("エラー: {e}"),
                                    }
                                }
                            }
                            chat_session::save_sessions(&this.session_store);
                            this.chat_scroll.scroll_to_bottom();
                            cx.notify();
                        });
                    });
                })
                .detach();
            }
            Ok(backend) => {
                let hw = hardware_params.clone();
                cx.spawn(async move |this, cx| {
                    let result: Result<chat_client::ChatCompletionResult, String> =
                        smol::unblock(move || {
                            chat_client::complete_chat_blocking(
                                &backend,
                                &api_messages,
                                temperature,
                                max_tokens,
                                context_length,
                                &hw,
                            )
                        })
                        .await;

                    let _ = cx.update(|app| {
                        let _ = this.update(app, |this: &mut AppView, cx| {
                            this.chat_pending = false;
                            if let Some(last) = this
                                .session_store
                                .active_mut()
                                .and_then(|s| s.messages.last_mut())
                            {
                                if last.role == "assistant" {
                                    match result {
                                        Ok(reply) => apply_chat_completion_result(last, reply),
                                        Err(e) => last.content = format!("エラー: {e}"),
                                    }
                                }
                            }
                            chat_session::save_sessions(&this.session_store);
                            this.chat_scroll.scroll_to_bottom();
                            cx.notify();
                        });
                    });
                })
                .detach();
            }
            Err(err) => {
                self.chat_pending = false;
                if let Some(last) = self
                    .session_store
                    .active_mut()
                    .and_then(|s| s.messages.last_mut())
                {
                    if last.role == "assistant" {
                        last.content = format!("エラー: {err}");
                    }
                }
                chat_session::save_sessions(&self.session_store);
                cx.notify();
            }
        }
    }

    /// Chat: Enter / 送信ボタンから呼ばれ、外部 API または Ollama で応答を取得する。
    fn on_chat_submitted(&mut self, cx: &mut Context<Self>) {
        if self.chat_pending {
            return;
        }
        let text = self.chat_composer.read(cx).text().trim().to_string();
        if text.is_empty() {
            return;
        }
        self.chat_composer.update(cx, |c, ecx| c.clear(ecx));
        self.submit_chat_request(Some(text), cx);
    }
}

// ============================================================
// Render
// ============================================================

impl Render for AppView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content: AnyElement = match self.page {
            Page::Chat => {
                let model_status = self.chat_model_status_line();
                let is_local_weights =
                    self.chat_prefs.source == model_prefs::ChatInferenceSource::LocalWeights;
                chat_page::render_chat_page(
                    &self.session_store,
                    self.chat_pending,
                    self.chat_show_thinking,
                    &model_status,
                    is_local_weights,
                    self.open_session_menu_id,
                    self.renaming_session_id,
                    self.session_title_editor.clone(),
                    self.chat_composer.clone(),
                    &self.chat_scroll,
                    cx,
                )
                .into_any_element()
            }
            Page::Settings => self.render_settings(cx).into_any_element(),
            Page::Terminal => self.render_terminal().into_any_element(),
            Page::Discover => discover_page::render_discover_page(
                &self.hf_state,
                self.hf_search_composer.clone(),
                &self.hf_downloads,
                cx,
            )
            .into_any_element(),
        };

        div()
            .size_full()
            .bg(hex(BG))
            .flex()
            .flex_col()
            .rounded(px(20.))
            .overflow_hidden()
            .child(self.render_titlebar())
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .flex()
                    .overflow_hidden()
                    .child(content),
            )
    }
}


// ============================================================
// Entry Point
// ============================================================

fn install_chat_submit_fallback(cx: &mut App) {
    chat_composer::install_enter_submit_interceptor(cx).detach();
}

fn main() {
    i18n::init();
    Application::new().run(|cx: &mut App| {
        // キーバインド登録
        editor::actions::register_keybindings(cx);
        chat_composer::register_keybindings(cx);
        session_title_editor::register_keybindings(cx);
        install_chat_submit_fallback(cx);

        let bounds = Bounds::centered(None, size(px(1400.), px(900.)), cx);

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Open_Agents — AI Coding Assistant".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(16.), px(16.))),
                }),
                ..Default::default()
            },
            |window, cx| {
                window.set_client_inset(px(44.));

                cx.new(|cx| {
                    let workspace_root = workspace_prefs::resolve_workspace_at_launch();
                    let (file_tree, explorer_expanded) = match read_tree_from_disk(&workspace_root)
                    {
                        Ok(tree) => {
                            let exp = expanded_first_level(&tree);
                            (tree, exp)
                        }
                        Err(e) => {
                            eprintln!("explorer: 初期読込失敗 ({e}), デモツリーを使用します");
                            (default_sample_tree(), default_expanded_set())
                        }
                    };
                    let local_llm = model_prefs::load_local_llm_prefs();
                    let api_keys = api_key_prefs::load_api_keys();
                    let chat_composer =
                        cx.new(|ecx| chat_composer::ChatComposer::new(ecx, i18n::placeholder_message()));
                    let llama_cpp_runtime_statuses =
                        llama_cpp_runtime::probe_bundled_runtime_statuses();
                    cx.subscribe(
                        &chat_composer,
                        |this: &mut AppView, _, _: &chat_composer::SubmitChat, cx| {
                            this.on_chat_submitted(cx);
                        },
                    )
                    .detach();
                    // Discover ページ用の検索コンポーザー — Enter で検索実行
                    let hf_search_composer = cx.new(|ecx| {
                        chat_composer::ChatComposer::new(
                            ecx,
                            i18n::discover_search_placeholder(),
                        )
                    });
                    cx.subscribe(
                        &hf_search_composer,
                        |this: &mut AppView, _, _: &chat_composer::SubmitChat, cx| {
                            this.hf_execute_search(cx);
                        },
                    )
                    .detach();
                    // API サーバー設定 — キーが空なら自動生成して保存
                    let mut api_server_prefs = local_llm.api_server;
                    if api_server_prefs.api_key.is_empty() {
                        api_server_prefs.api_key = api_server::generate_api_key();
                    }
                    let mut app = AppView {
                        page: Page::Chat,
                        session_store: chat_session::load_sessions(),
                        chat_show_thinking: true,
                        settings_model_paths: local_llm.model_paths,
                        model_params: local_llm.model,
                        hardware_params: local_llm.hardware,
                        appearance_prefs: local_llm.appearance,
                        ai_prefs: local_llm.ai,
                        chat_prefs: local_llm.chat,
                        api_keys,
                        api_key_reveal: vec![false; api_key_prefs::PROVIDER_CATALOG.len()],
                        fetched_models: Vec::new(),
                        fetching_models: false,
                        workspace_root,
                        file_tree,
                        explorer_expanded,
                        explorer_selection: None,
                        chat_composer,
                        chat_scroll: ScrollHandle::new(),
                        chat_pending: false,
                        open_session_menu_id: None,
                        renaming_session_id: None,
                        session_title_editor: None,
                        llama_cpp_runtime_statuses,
                        llama_cpp_update_notice: None,
                        gpu_profile_summary: String::new(),
                        hf_state: Box::new(hf_discover::HuggingFaceSearchState::default()),
                        hf_search_composer,
                        hf_downloads: hf_discover::DownloadManager::default(),
                        api_server_prefs,
                        api_server: None,
                        api_server_error: None,
                    };
                    llama_cpp_chat::cleanup_orphan_servers();
                    // GPU 自動検出 & 最適化（Auto モード時）
                    {
                        let profile = model_prefs::detect_gpu_profile();
                        app.gpu_profile_summary = profile.summary.clone();
                        if app.hardware_params.llama_runtime_preset
                            == model_prefs::LlamaRuntimePreset::Auto
                        {
                            model_prefs::apply_gpu_profile(&mut app.hardware_params, &profile);
                            app.persist_local_llm_prefs();
                        }
                    }
                    app.start_llama_cpp_update_check(cx);
                    app.prewarm_llama_server(cx);
                    // API サーバー自動起動
                    if app.api_server_prefs.enabled {
                        app.start_api_server();
                    }
                    app.persist_local_llm_prefs();
                    app
                })
            },
        )
        .unwrap();
    });
}
