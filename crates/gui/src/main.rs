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

fn human_readable_size(bytes: u64) -> String {
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
enum Page {
    Chat,
    Settings,
    Terminal,
    Discover,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ModelFormat {
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
enum ModelParamAdjustKind {
    Temperature,
    MaxOutputTokens,
    ContextLength,
}

#[derive(Clone, Copy)]
enum HardwareParamAdjustKind {
    GpuLayers,
    NThreads,
    BatchSize,
}

#[derive(Clone, Copy)]
enum AiToggleKind {
    AutoComplete,
    CodeSuggestions,
    StreamingResponses,
}

struct AppView {
    page: Page,
    /// チャットセッション管理（マルチセッション + 永続化）
    session_store: chat_session::SessionStore,
    /// Figma Chat ヘッダー「思考を表示」トグル
    chat_show_thinking: bool,
    /// 設定で読み込んだローカル LLM（下に追加・永続化）
    settings_model_paths: Vec<PathBuf>,
    /// 永続化済みローカル LLM 推論パラメータ（Temperature / max tokens / context）
    model_params: model_prefs::ModelParams,
    /// GPU スレッド・バッチ等（`model_params.json` の `hardware` と同期）
    hardware_params: model_prefs::HardwareParams,
    /// エディタのテーマ・フォント・行番号（`appearance` と同期）
    appearance_prefs: model_prefs::AppearancePrefs,
    /// AI 補助機能の ON/OFF（`ai` と同期）
    ai_prefs: model_prefs::AiPrefs,
    /// Chat の推論先・モデル ID（`model_params.json` の `chat`）
    chat_prefs: model_prefs::ChatPrefs,
    /// 外部 API キー（`api_keys.json`）
    api_keys: api_key_prefs::ApiKeyPrefs,
    /// 設定画面での各カタログ行のプレーン表示（永続化しない、`PROVIDER_CATALOG` と同順）
    api_key_reveal: Vec<bool>,
    /// プロバイダから取得したモデルID一覧（キャッシュ）: (provider_id, label, models)
    fetched_models: Vec<(String, String, Vec<String>)>,
    /// モデル取得中フラグ
    fetching_models: bool,
    /// 開いているワークスペースのルート（Zed worktree root）
    workspace_root: PathBuf,
    /// 仮想ファイルツリー
    file_tree: TreeNode,
    /// 展開中ディレクトリ（パスごと）— Zed `expanded_dir_ids` 相当
    explorer_expanded: HashSet<Vec<String>>,
    /// フォーカス/選択行
    explorer_selection: Option<Vec<String>>,
    chat_composer: Entity<chat_composer::ChatComposer>,
    /// Chat メッセージスクロールハンドル
    chat_scroll: ScrollHandle,
    /// Chat API リクエスト送信中（再送信ガード）
    chat_pending: bool,
    /// 三点メニューを開いているセッション
    open_session_menu_id: Option<u64>,
    /// セッション名変更中の対象セッション
    renaming_session_id: Option<u64>,
    /// セッション名変更用エディタ
    session_title_editor: Option<Entity<session_title_editor::SessionTitleEditor>>,
    /// backend ごとの同梱 llama.cpp runtime 状態
    llama_cpp_runtime_statuses: Vec<llama_cpp_runtime::BundledLlamaRuntimeStatus>,
    /// GitHub Releases の更新通知
    llama_cpp_update_notice: Option<llama_cpp_runtime::LlamaCppUpdateNotice>,
    /// GPU 自動検出結果のサマリ（設定画面で表示）
    gpu_profile_summary: String,
    /// Hugging Face モデル検索・ダウンロード状態（Discover ページ専用 — Box で AppView のスタックサイズ削減）
    hf_state: Box<hf_discover::HuggingFaceSearchState>,
    /// Discover ページの検索バー（ChatComposer を流用）
    hf_search_composer: Entity<chat_composer::ChatComposer>,
    /// ダウンロードマネージャ
    hf_downloads: hf_discover::DownloadManager,
    /// 内蔵 API サーバー設定（永続化）
    api_server_prefs: model_prefs::ApiServerPrefs,
    /// 稼働中の API サーバーインスタンス
    api_server: Option<api_server::ApiServer>,
    /// API サーバー起動エラーメッセージ
    api_server_error: Option<String>,
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

impl AppView {
    /// 形式ドロップダウン用（末尾＝直近追加のモデル）
    fn settings_last_model_path(&self) -> Option<&Path> {
        self.settings_model_paths.last().map(|p| p.as_path())
    }

    fn settings_detected_model_format(&self) -> Option<ModelFormat> {
        self.settings_last_model_path().map(ModelFormat::from_path)
    }

    fn settings_model_format_label(&self) -> SharedString {
        self.settings_detected_model_format()
            .map(|format| format.label().into())
            .unwrap_or_else(|| "自動判定".into())
    }

    fn settings_model_format_hint(&self) -> SharedString {
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

    fn settings_model_filename_for(path: &Path) -> SharedString {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.to_string().into())
            .unwrap_or_else(|| "（無名）".into())
    }

    fn settings_model_path_label_for(path: &Path) -> SharedString {
        path.to_string_lossy().into_owned().into()
    }

    fn settings_model_meta_label_for(path: &Path) -> SharedString {
        let format = ModelFormat::from_path(path).label();
        let size = fs::metadata(path)
            .ok()
            .map(|meta| human_readable_size(meta.len()))
            .unwrap_or_else(|| "サイズ不明".to_string());
        format!("{format} • {size}").into()
    }

    fn settings_loaded_model_row(
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
                                    .text_color(hex(0x22c55e))
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
                    .bg(hex(0x2d2d2d))
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

    fn sync_api_key_reveal_len(&mut self) {
        let n = api_key_prefs::PROVIDER_CATALOG.len();
        if self.api_key_reveal.len() != n {
            self.api_key_reveal.resize(n, false);
        }
    }

    fn settings_api_key_paste_row(&mut self, provider_id: &'static str, cx: &mut Context<Self>) {
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

    fn settings_api_key_clear_row(
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

    fn settings_api_key_toggle_reveal_row(&mut self, row_idx: usize, cx: &mut Context<Self>) {
        if let Some(r) = self.api_key_reveal.get_mut(row_idx) {
            *r = !*r;
        }
        cx.notify();
    }

    fn settings_api_key_row(
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
                                        .text_color(hex(0x22c55e))
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
    fn settings_api_keys_child_elements(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
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

    fn render_titlebar(&self) -> impl IntoElement {
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

    fn settings_figma_heading(&self, icon: &str, icon_color: u32, title: &str) -> impl IntoElement {
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

    fn settings_fake_dropdown(&self, label: &str) -> impl IntoElement {
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

    fn settings_labeled_block(&self, title: &str, subtitle: &str) -> impl IntoElement {
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

    fn adjust_model_param(
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

    fn normalize_model_params_for_chat_source(&mut self) {
        if self.chat_prefs.source == model_prefs::ChatInferenceSource::LocalWeights {
            self.model_params.max_output_tokens =
                model_prefs::effective_local_max_output_tokens(self.model_params.max_output_tokens);
        }
    }

    fn persist_local_llm_prefs(&self) {
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

    fn resolve_proxy_target(&self) -> (String, String) {
        match chat_client::resolve_chat_backend(
            &self.api_keys,
            &self.chat_prefs,
            &self.settings_model_paths,
        ) {
            Ok(backend) => backend.proxy_target(),
            Err(_) => (String::new(), String::new()),
        }
    }

    fn start_api_server(&mut self) {
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

    fn stop_api_server(&mut self) {
        if let Some(mut s) = self.api_server.take() {
            s.stop();
        }
        self.api_server_error = None;
    }

    fn api_server_is_running(&self) -> bool {
        self.api_server.as_ref().map_or(false, |s| s.is_running())
    }

    fn settings_api_server_section(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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

    fn cycle_chat_inference_source(&mut self, cx: &mut Context<Self>) {
        self.chat_prefs.source = self.chat_prefs.source.cycle();
        self.chat_prefs = self.chat_prefs.clone().sanitize();
        self.normalize_model_params_for_chat_source();
        self.persist_local_llm_prefs();
        // LocalWeights に切り替わった場合、サーバをプリウォーム
        self.prewarm_llama_server(cx);
        cx.notify();
    }

    fn adjust_chat_local_model_index(&mut self, delta: i32, cx: &mut Context<Self>) {
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
    fn fetch_models(&mut self, cx: &mut Context<Self>) {
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
    fn render_model_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
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

    fn settings_chat_paste_api_model(&mut self, cx: &mut Context<Self>) {
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

    fn settings_chat_clear_api_model(&mut self, cx: &mut Context<Self>) {
        self.chat_prefs.api_model.clear();
        self.chat_prefs = self.chat_prefs.clone().sanitize();
        self.persist_local_llm_prefs();
        cx.notify();
    }

    fn settings_chat_paste_ollama_model(&mut self, cx: &mut Context<Self>) {
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

    fn settings_chat_clear_ollama_model(&mut self, cx: &mut Context<Self>) {
        self.chat_prefs.ollama_model = model_prefs::ChatPrefs::default().ollama_model;
        self.chat_prefs = self.chat_prefs.clone().sanitize();
        self.persist_local_llm_prefs();
        cx.notify();
    }

    fn chat_local_weights_summary(&self) -> String {
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
    fn chat_model_status_line(&self) -> String {
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

    fn selected_runtime_backend(&self) -> llama_cpp_runtime::BundledLlamaBackend {
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

    fn runtime_status_for_backend(
        &self,
        backend: llama_cpp_runtime::BundledLlamaBackend,
    ) -> Option<&llama_cpp_runtime::BundledLlamaRuntimeStatus> {
        self.llama_cpp_runtime_statuses
            .iter()
            .find(|status| status.backend == backend)
    }

    fn selected_runtime_manifest(&self) -> Option<&llama_cpp_runtime::BundledLlamaManifest> {
        self.runtime_status_for_backend(self.selected_runtime_backend())
            .and_then(|status| status.manifest.as_ref())
    }

    fn selected_runtime_error(&self) -> Option<String> {
        self.runtime_status_for_backend(self.selected_runtime_backend())
            .and_then(|status| status.error.clone())
    }

    fn runtime_preset_is_available(&self, preset: model_prefs::LlamaRuntimePreset) -> bool {
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

    fn llama_cpp_bundle_status_line(&self) -> String {
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

    fn copy_llama_cpp_release_url(&mut self, cx: &mut Context<Self>) {
        if let Some(notice) = &self.llama_cpp_update_notice {
            cx.write_to_clipboard(ClipboardItem::new_string(notice.release_url.clone()));
        }
    }

    fn start_llama_cpp_update_check(&mut self, cx: &mut Context<Self>) {
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

    fn settings_cot_mode_row(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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

    fn settings_react_mode_row(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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

    fn settings_tot_mode_row(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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

    fn settings_self_consistency_row(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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

    fn settings_chat_inference_block(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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

    fn sync_editor_appearance(&self, _cx: &mut Context<Self>) {
        // Editor は削除済み — 外観同期は不要
    }

    fn cycle_appearance_theme(&mut self, cx: &mut Context<Self>) {
        self.appearance_prefs.theme =
            model_prefs::AppearancePrefs::cycle_theme(self.appearance_prefs.theme);
        self.appearance_prefs.clamp();
        self.persist_local_llm_prefs();
        self.sync_editor_appearance(cx);
        cx.notify();
    }

    fn adjust_appearance_font(&mut self, delta: i32, cx: &mut Context<Self>) {
        self.appearance_prefs.font_size_px =
            model_prefs::AppearancePrefs::step_font_size(self.appearance_prefs.font_size_px, delta);
        self.appearance_prefs.clamp();
        self.persist_local_llm_prefs();
        self.sync_editor_appearance(cx);
        cx.notify();
    }

    fn toggle_appearance_line_numbers(&mut self, cx: &mut Context<Self>) {
        self.appearance_prefs.show_line_numbers = !self.appearance_prefs.show_line_numbers;
        self.persist_local_llm_prefs();
        self.sync_editor_appearance(cx);
        cx.notify();
    }

    fn settings_appearance_theme_row(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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

    fn settings_appearance_font_row(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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

    fn settings_appearance_line_numbers_row(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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

    fn ai_toggle_value(&self, kind: AiToggleKind) -> bool {
        match kind {
            AiToggleKind::AutoComplete => self.ai_prefs.auto_complete,
            AiToggleKind::CodeSuggestions => self.ai_prefs.code_suggestions,
            AiToggleKind::StreamingResponses => self.ai_prefs.streaming_responses,
        }
    }

    fn toggle_ai_setting(&mut self, kind: AiToggleKind, cx: &mut Context<Self>) {
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

    fn settings_ai_toggle_row(
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

    fn adjust_hardware_param(
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

    fn settings_runtime_preset_row(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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

    fn settings_hardware_stepper_row(
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
    fn settings_model_param_row(
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

    fn hf_open_discover(&mut self, cx: &mut Context<Self>) {
        self.page = Page::Discover;
        cx.notify();
    }

    fn hf_cycle_sort(&mut self, cx: &mut Context<Self>) {
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

    fn hf_toggle_downloads_panel(&mut self, cx: &mut Context<Self>) {
        self.hf_downloads.panel_open = !self.hf_downloads.panel_open;
        cx.notify();
    }

    fn hf_execute_search(&mut self, cx: &mut Context<Self>) {
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

    fn hf_select_model(&mut self, id: String, cx: &mut Context<Self>) {
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

    fn hf_start_download(
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

    fn hf_cancel_download(&mut self, id: u64, cx: &mut Context<Self>) {
        self.hf_downloads.cancel(id);
        cx.notify();
    }

    // ============================================================

    fn render_settings(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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

    fn render_terminal(&self) -> impl IntoElement {
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
