mod api_key_prefs;
mod native_chat;
mod chat_client;
mod chat_composer;
mod editor;
mod model_prefs;
mod project_explorer;
mod workspace_prefs;

use editor::EditorView;
use gpui::*;
use gpui::prelude::FluentBuilder;
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use project_explorer::{
    TreeNode, absolute_path, default_expanded_set, default_sample_tree, expanded_first_level,
    flatten_visible, path_to_segments, prune_expanded, read_tree_from_disk, unique_child_name,
};

// ============================================================
// Figma Design Colors (VS Code Dark / macOS style)
// ============================================================

pub const BG: u32 = 0x1e1e1e;
pub const SIDEBAR_BG: u32 = 0x252526;
pub const TITLEBAR_BG: u32 = 0x2d2d2d;
pub const TITLEBAR_GRADIENT_TOP: u32 = 0x4a4a4c;
pub const TITLEBAR_GRADIENT_BOTTOM: u32 = 0x2f2f31;
pub const BORDER: u32 = 0x3d3d3d;
pub const HOVER_BG: u32 = 0x37373d;
pub const PANEL_BG: u32 = 0x252526;
pub const TEXT_PRIMARY: u32 = 0xe5e5e5;
pub const TEXT_SECONDARY: u32 = 0x9ca3af;
pub const TEXT_MUTED: u32 = 0x6b7280;
pub const TEXT_DIM: u32 = 0x4b5563;
pub const ACCENT_BLUE: u32 = 0x2563eb;
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
pub const CONTROL_BG: u32 = 0x3d3d3d;
pub const CONTROL_BORDER: u32 = 0x4d4d4d;

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

#[cfg(test)]
mod tests {
    use super::{ModelFormat, human_readable_size};
    use std::path::Path;

    #[test]
    fn detects_model_format_from_extension_case_insensitively() {
        assert_eq!(ModelFormat::from_path(Path::new("model.gguf")), ModelFormat::Gguf);
        assert_eq!(ModelFormat::from_path(Path::new("model.ONNX")), ModelFormat::Onnx);
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
}

// ============================================================
// State
// ============================================================

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Editor,
    Chat,
    Settings,
    Terminal,
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

struct ChatMsg {
    role: String,
    content: String,
    thinking: Option<String>,
}

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
    chat_messages: Vec<ChatMsg>,
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
    /// 開いているワークスペースのルート（Zed worktree root）
    workspace_root: PathBuf,
    /// 仮想ファイルツリー
    file_tree: TreeNode,
    /// 展開中ディレクトリ（パスごと）— Zed `expanded_dir_ids` 相当
    explorer_expanded: HashSet<Vec<String>>,
    /// フォーカス/選択行
    explorer_selection: Option<Vec<String>>,
    editor_view: Entity<EditorView>,
    chat_composer: Entity<chat_composer::ChatComposer>,
    /// Chat API リクエスト送信中（再送信ガード）
    chat_pending: bool,
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
                self.explorer_selection = Some(segs.clone());
                let wr = self.workspace_root.clone();
                self.editor_view.update(cx, |ed, ecx| {
                    ed.open_project_path(&wr, &segs, ecx);
                });
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

    /// Chat の作業ディレクトリ（エクスプローラ選択を最優先、なければエディタの開いているファイル、最後にワークスペースルート）
    fn chat_working_directory(&self, cx: &mut Context<Self>) -> PathBuf {
        let root = self
            .workspace_root
            .canonicalize()
            .unwrap_or_else(|_| self.workspace_root.clone());

        if let Some(segs) = &self.explorer_selection {
            let abs = absolute_path(&self.workspace_root, segs);
            if abs.is_dir() {
                return abs.canonicalize().unwrap_or(abs);
            }
            if abs.is_file() {
                return abs
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| root.clone());
            }
            if let Some(parent) = abs.parent() {
                if !parent.as_os_str().is_empty() {
                    return parent.to_path_buf();
                }
            }
        }

        self.editor_view.read(cx).chat_working_directory(&self.workspace_root)
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

        let work_dir = self.chat_working_directory(cx);
        let work_dir_msg = format!(
            "作業ディレクトリ（Editor / エクスプローラで選んだ場所。相対パス・ターミナルコマンドの cwd はここを基準に解釈してください）: {}",
            work_dir.display()
        );
        let api_messages: Vec<(String, String)> = std::iter::once(("system".into(), work_dir_msg))
            .chain(
                self.chat_messages
                    .iter()
                    .map(|m| (m.role.clone(), m.content.clone())),
            )
            .chain(std::iter::once(("user".into(), text.clone())))
            .collect();

        self.chat_composer.update(cx, |c, ecx| c.clear(ecx));

        self.chat_messages.push(ChatMsg {
            role: "user".into(),
            content: text,
            thinking: None,
        });
        self.chat_messages.push(ChatMsg {
            role: "assistant".into(),
            content: "応答を待っています…".into(),
            thinking: None,
        });
        self.chat_pending = true;
        cx.notify();

        let api_keys = self.api_keys.clone();
        let chat_prefs = self.chat_prefs.clone();
        let local_model_paths = self.settings_model_paths.clone();
        let temperature = self.model_params.temperature;
        let max_tokens = self.model_params.max_output_tokens;

        cx.spawn(async move |this, cx| {
            let result: Result<String, String> = smol::unblock(move || {
                let backend = chat_client::resolve_chat_backend(
                    &api_keys,
                    &chat_prefs,
                    &local_model_paths,
                )?;
                chat_client::complete_chat_blocking(
                    &backend,
                    &api_messages,
                    temperature,
                    max_tokens,
                )
            })
            .await;

            let _ = cx.update(|app| {
                let _ = this.update(app, |this: &mut AppView, cx| {
                    this.chat_pending = false;
                    if let Some(last) = this.chat_messages.last_mut() {
                        if last.role == "assistant" {
                            last.content = match result {
                                Ok(reply) => reply,
                                Err(e) => format!("エラー: {e}"),
                            };
                        }
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }
}

// ============================================================
// Render
// ============================================================

impl Render for AppView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content: AnyElement = match self.page {
            Page::Editor => self.render_editor(cx).into_any_element(),
            Page::Chat => self.render_chat_page(cx).into_any_element(),
            Page::Settings => self.render_settings(cx).into_any_element(),
            Page::Terminal => self.render_terminal().into_any_element(),
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
                    .child(self.render_sidebar(cx))
                    .child(div().w(px(1.)).h_full().bg(hex(BORDER)))
                    .child(
                        div()
                            .flex_1()
                            .min_h(px(0.))
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(content),
                    ),
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
        let key_ref = self.api_keys.get_str(provider_id);
        let has_key = !key_ref.is_empty();
        let reveal = self
            .api_key_reveal
            .get(row_idx)
            .copied()
            .unwrap_or(false);
        let masked: SharedString = api_key_prefs::masked_line(key_ref, reveal).into();
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
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(hex(TEXT_MUTED))
                            .child(tag),
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
                                .child(def.group),
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
                        let drag_hitbox =
                            window.insert_hitbox(drag_bounds, HitboxBehavior::Normal);

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
                        let min_hitbox =
                            window.insert_hitbox(min_bounds, HitboxBehavior::Normal);

                        let max_bounds = Bounds {
                            origin: point(bounds.origin.x + px(56.), bounds.origin.y + px(16.)),
                            size: size(px(12.), px(12.)),
                        };
                        let max_hitbox =
                            window.insert_hitbox(max_bounds, HitboxBehavior::Normal);

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
    // Sidebar
    // ============================================================

    fn render_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut visible = Vec::new();
        flatten_visible(&self.file_tree, &self.explorer_expanded, &mut visible);

        div()
            .w(px(220.))
            .h_full()
            .bg(hex(SIDEBAR_BG))
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(
                div()
                    .p(px(12.))
                    .pt(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(self.nav_item("Editor", Page::Editor, cx))
                    .child(self.nav_item("Chat", Page::Chat, cx))
                    .child(self.nav_item("Settings", Page::Settings, cx)),
            )
            .child(
                div()
                    .flex_1()
                    .border_t_1()
                    .border_color(hex(BORDER))
                    .flex()
                    .flex_col()
                    .min_h(px(0.))
                    .child(
                        div()
                            .p(px(12.))
                            .pb(px(4.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .mb(px(4.))
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(hex(TEXT_SECONDARY))
                                            .child("📂"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(10.))
                                            .text_color(hex(TEXT_SECONDARY))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child("EXPLORER"),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(2.))
                                    .mb(px(4.))
                                    .px(px(8.))
                                    .child(
                                        div()
                                            .p(px(4.))
                                            .rounded(px(4.))
                                            .text_size(px(12.))
                                            .text_color(hex(TEXT_SECONDARY))
                                            .cursor_pointer()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                                    this.explorer_new_file(cx);
                                                }),
                                            )
                                            .child("📄+"),
                                    )
                                    .child(
                                        div()
                                            .p(px(4.))
                                            .rounded(px(4.))
                                            .text_size(px(12.))
                                            .text_color(hex(TEXT_SECONDARY))
                                            .cursor_pointer()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                                    this.explorer_new_folder(cx);
                                                }),
                                            )
                                            .child("📁+"),
                                    )
                                    .child(
                                        div()
                                            .p(px(4.))
                                            .rounded(px(4.))
                                            .text_size(px(12.))
                                            .text_color(hex(TEXT_SECONDARY))
                                            .cursor_pointer()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                                    this.explorer_reload_from_disk();
                                                    cx.notify();
                                                }),
                                            )
                                            .child("🔄"),
                                    )
                                    .child(
                                        div()
                                            .p(px(4.))
                                            .rounded(px(4.))
                                            .text_size(px(12.))
                                            .text_color(hex(TEXT_SECONDARY))
                                            .cursor_pointer()
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                                    this.explorer_open_folder_dialog(cx);
                                                }),
                                            )
                                            .child("📂"),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .min_h(px(0.))
                            .pb(px(8.))
                            .children(visible.into_iter().map(|row| {
                                let path = row.path.clone();
                                let is_dir = row.is_dir;
                                let is_expanded = row.is_expanded;
                                let depth = row.depth;
                                let label = path
                                    .last()
                                    .cloned()
                                    .unwrap_or_default();
                                let is_selected =
                                    self.explorer_selection.as_ref() == Some(&path);
                                let chevron = if is_dir {
                                    if is_expanded { "▼" } else { "▶" }
                                } else {
                                    " "
                                };
                                let icon = if is_dir { "📁" } else { "📄" };
                                let bg = if is_selected {
                                    hex(EXPLORER_SELECTION_BG)
                                } else {
                                    hex_a(0x000000, 0.0)
                                };
                                let indent = px(10. * depth as f32);

                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .ml(indent)
                                    .mr(px(8.))
                                    .px(px(6.))
                                    .py(px(3.))
                                    .rounded(px(3.))
                                    .bg(bg)
                                    .cursor_pointer()
                                    .text_size(px(13.))
                                    .text_color(hex(TEXT_SECONDARY))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(
                                            move |this, _ev: &MouseDownEvent, window, cx| {
                                                this.explorer_selection = Some(path.clone());
                                                if is_dir {
                                                    if this.explorer_expanded.contains(&path) {
                                                        this.explorer_expanded.remove(&path);
                                                    } else {
                                                        this.explorer_expanded.insert(path.clone());
                                                    }
                                                } else {
                                                    // Chat 等のページだとエディタが描画されないため必ず Editor へ
                                                    this.page = Page::Editor;
                                                    let wr = this.workspace_root.clone();
                                                    let segs = path.clone();
                                                    this.editor_view.update(cx, |ed, ecx| {
                                                        ed.open_project_path(&wr, &segs, ecx);
                                                    });
                                                    this.editor_view.read(cx).focus_editor(window);
                                                }
                                                cx.notify();
                                            },
                                        ),
                                    )
                                    .child(
                                        div()
                                            .w(px(14.))
                                            .flex_shrink_0()
                                            .text_size(px(10.))
                                            .text_color(hex(TEXT_MUTED))
                                            .text_center()
                                            .child(chevron),
                                    )
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .text_size(px(12.))
                                            .child(icon),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .overflow_hidden()
                                            .child(label),
                                    )
                            })),
                    ),
            )
            .child(
                div()
                    .border_t_1()
                    .border_color(hex(BORDER))
                    .p(px(12.))
                    .child(self.nav_item("Terminal", Page::Terminal, cx)),
            )
    }

    fn nav_item(&mut self, label: &str, page: Page, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.page == page;
        let bg = if active {
            hex(HOVER_BG)
        } else {
            hex_a(0x000000, 0.0)
        };
        let fg = if active {
            hex(TEXT_PRIMARY)
        } else {
            hex(TEXT_SECONDARY)
        };

        let icon = match page {
            Page::Editor => "💻",
            Page::Chat => "💬",
            Page::Settings => "⚙",
            Page::Terminal => "▶",
        };

        let target = page;
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(12.))
            .py(px(8.))
            .rounded(px(6.))
            .bg(bg)
            .text_color(fg)
            .text_size(px(13.))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.page = target;
                    cx.notify();
                }),
            )
            .child(icon)
            .child(label.to_string())
    }

    // ============================================================
    // Editor View — EditorView Entity を組み込む
    // ============================================================

    fn render_editor(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let tab_title = self.editor_view.read(cx).tab_title();

        div()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .bg(hex(BG))
            // タブヘッダー
            .child(
                div()
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
                            .gap(px(4.))
                            .child(
                                div()
                                    .px(px(12.))
                                    .py(px(6.))
                                    .bg(hex(BG))
                                    .border_1()
                                    .border_color(hex(BORDER))
                                    .rounded_t(px(6.))
                                    .text_size(px(12.))
                                    .text_color(hex(TEXT_PRIMARY))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(tab_title)
                                    .child(
                                        div()
                                            .text_size(px(10.))
                                            .text_color(hex(TEXT_MUTED))
                                            .child("×"),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .p(px(6.))
                                    .rounded(px(4.))
                                    .text_size(px(14.))
                                    .text_color(hex(TEXT_SECONDARY))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                            let wr = this.workspace_root.clone();
                                            this.editor_view.update(cx, |ed, ecx| {
                                                ed.perform_save(ecx, Some(wr));
                                            });
                                            cx.notify();
                                        }),
                                    )
                                    .child("💾"),
                            ),
                    ),
            )
            // エディタ本体
            .child(self.editor_view.clone())
            // ステータスバー
            .child(
                div()
                    .h(px(32.))
                    .bg(hex(STATUSBAR_BG))
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(16.))
                    .text_size(px(11.))
                    .text_color(hex(0xFFFFFF))
                    .child(
                        div()
                            .flex()
                            .gap(px(16.))
                            .child("UTF-8")
                            .child("LF"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(16.))
                            .child("Spaces: 4"),
                    ),
            )
    }

    // ============================================================
    // Chat View（Figma Make: ChatView.tsx に合わせたレイアウト）
    // ============================================================

    fn render_chat_page(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let show_suggestions = self.chat_messages.len() == 1;
        let thinking_toggle = self.chat_show_thinking;
        let send_disabled = self.chat_pending;

        div()
            .flex_1()
            .flex()
            .flex_col()
            .min_h(px(0.))
            .bg(hex(BG))
            .child(
                div()
                    .flex_shrink_0()
                    .bg(hex(PANEL_BG))
                    .border_b_1()
                    .border_color(hex(BORDER))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(48.))
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
                                            .w(px(24.))
                                            .h(px(24.))
                                            .rounded(px(6.))
                                            .bg(hex(ACCENT_ORANGE))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(12.))
                                            .text_color(hex(0xFFFFFF))
                                            .child("✦"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .text_color(hex(TEXT_PRIMARY))
                                            .child("Open Agents"),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(hex(TEXT_SECONDARY))
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                            this.chat_show_thinking = !this.chat_show_thinking;
                                            cx.notify();
                                        }),
                                    )
                                    .child(if thinking_toggle {
                                        "思考を非表示"
                                    } else {
                                        "思考を表示"
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .px(px(16.))
                            .pb(px(8.))
                            .text_size(px(10.))
                            .text_color(hex(TEXT_MUTED))
                            .child(self.chat_model_status_line()),
                    ),
            )
            .child(
                div()
                    .id("chat-messages-scroll")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .child(
                        div()
                            .max_w(px(896.))
                            .mx_auto()
                            .px(px(24.))
                            .py(px(32.))
                            .flex()
                            .flex_col()
                            .when(show_suggestions, |d| {
                                d.child(
                                    div()
                                        .mb(px(32.))
                                        .flex()
                                        .flex_col()
                                        .child(
                                            div()
                                                .text_size(px(10.))
                                                .text_color(hex(TEXT_MUTED))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .mb(px(12.))
                                                .child("提案"),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .gap(px(8.))
                                                .child(
                                                    div()
                                                        .flex()
                                                        .gap(px(8.))
                                                        .child(self.suggestion_chip("Reactコンポーネントを作成"))
                                                        .child(self.suggestion_chip("バグを修正")),
                                                )
                                                .child(
                                                    div()
                                                        .flex()
                                                        .gap(px(8.))
                                                        .child(self.suggestion_chip("コードをリファクタリング"))
                                                        .child(self.suggestion_chip("テストを追加")),
                                                ),
                                        ),
                                )
                            })
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(32.))
                                    .children(self.chat_messages.iter().map(|msg| {
                                        let is_user = msg.role == "user";
                                        let mut block = div()
                                            .flex()
                                            .flex_col()
                                            .gap(px(12.))
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(8.))
                                                    .child(if is_user {
                                                        div()
                                                            .w(px(24.))
                                                            .h(px(24.))
                                                            .rounded(px(6.))
                                                            .bg(hex(ACCENT_BLUE))
                                                            .flex()
                                                            .items_center()
                                                            .justify_center()
                                                            .text_size(px(11.))
                                                            .text_color(hex(0xFFFFFF))
                                                            .child("U")
                                                            .into_any_element()
                                                    } else {
                                                        div()
                                                            .w(px(24.))
                                                            .h(px(24.))
                                                            .rounded(px(6.))
                                                            .bg(hex(ACCENT_ORANGE))
                                                            .flex()
                                                            .items_center()
                                                            .justify_center()
                                                            .text_size(px(11.))
                                                            .text_color(hex(0xFFFFFF))
                                                            .child("✦")
                                                            .into_any_element()
                                                    })
                                                    .child(
                                                        div()
                                                            .text_size(px(12.))
                                                            .text_color(hex(TEXT_SECONDARY))
                                                            .child(if is_user {
                                                                "You"
                                                            } else {
                                                                "Agent"
                                                            }),
                                                    ),
                                            );
                                        if let Some(th) = &msg.thinking {
                                            if self.chat_show_thinking {
                                                block = block.child(
                                                    div()
                                                        .ml(px(32.))
                                                        .flex()
                                                        .gap(px(0.))
                                                        .child(
                                                            div()
                                                                .w(px(2.))
                                                                .flex_shrink_0()
                                                                .bg(hex(PURPLE)),
                                                        )
                                                        .child(
                                                            div()
                                                                .flex_1()
                                                                .p(px(12.))
                                                                .bg(hex(PANEL_BG))
                                                                .rounded(px(4.))
                                                                .text_size(px(12.))
                                                                .text_color(hex(TEXT_SECONDARY))
                                                                .child(th.clone()),
                                                        ),
                                                );
                                            }
                                        }
                                        block.child(
                                            div()
                                                .ml(px(32.))
                                                .text_size(px(13.))
                                                .text_color(hex(TEXT_PRIMARY))
                                                .child(msg.content.clone()),
                                        )
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .border_t_1()
                    .border_color(hex(BORDER))
                    .bg(hex(PANEL_BG))
                    .p(px(16.))
                    .child(
                        div()
                            .max_w(px(896.))
                            .mx_auto()
                            .child(
                                div()
                                    .w_full()
                                    .min_h(px(72.))
                                    .flex()
                                    .gap(px(8.))
                                    .items_end()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_h(px(72.))
                                            .bg(hex(BG))
                                            .border_1()
                                            .border_color(hex(BORDER))
                                            .rounded(px(12.))
                                            .px(px(12.))
                                            .py(px(8.))
                                            .flex()
                                            .items_center()
                                            .child(self.chat_composer.clone()),
                                    )
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .mb(px(4.))
                                            .mr(px(4.))
                                            .p(px(8.))
                                            .rounded(px(8.))
                                            .bg(if send_disabled {
                                                hex_a(ACCENT_BLUE, 0.45)
                                            } else {
                                                hex(ACCENT_BLUE)
                                            })
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(px(12.))
                                            .text_color(hex(0xFFFFFF))
                                            .cursor(if send_disabled {
                                                CursorStyle::OperationNotAllowed
                                            } else {
                                                CursorStyle::PointingHand
                                            })
                                            .when(!send_disabled, |d| {
                                                d.on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.on_chat_submitted(cx);
                                                    }),
                                                )
                                            })
                                            .child("➤"),
                                    ),
                            )
                            .child(
                                div()
                                    .mt(px(8.))
                                    .text_size(px(11.))
                                    .text_color(hex(TEXT_MUTED))
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .child("⌨".to_string())
                                    .child(
                                        div()
                                            .px(px(6.))
                                            .py(px(2.))
                                            .bg(hex(BORDER))
                                            .rounded(px(4.))
                                            .text_size(px(10.))
                                            .child("Enter"),
                                    )
                                    .child("で送信".to_string()),
                            ),
                    ),
            )
    }

    fn suggestion_chip(&self, label: &str) -> impl IntoElement {
        div()
            .flex_1()
            .min_w(px(0.))
            .px(px(16.))
            .py(px(12.))
            .bg(hex(PANEL_BG))
            .border_1()
            .border_color(hex(BORDER))
            .rounded(px(8.))
            .text_size(px(12.))
            .text_color(hex(TEXT_SECONDARY))
            .cursor_pointer()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(14.))
                    .text_color(hex(ACCENT_BLUE))
                    .child("⚡"),
            )
            .child(label.to_string())
    }

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

    fn adjust_model_param(&mut self, kind: ModelParamAdjustKind, steps: i32, cx: &mut Context<Self>) {
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
                self.model_params.context_length = v.clamp(512, 32768);
            }
        }
        self.model_params.clamp();
        self.persist_local_llm_prefs();
        cx.notify();
    }

    fn persist_local_llm_prefs(&self) {
        model_prefs::save_local_llm_prefs(&model_prefs::LocalLlmPrefs {
            model: self.model_params.clone(),
            hardware: self.hardware_params.clone(),
            appearance: self.appearance_prefs.clone(),
            ai: self.ai_prefs.clone(),
            model_paths: self.settings_model_paths.clone(),
            chat: self.chat_prefs.clone(),
        });
    }

    fn cycle_chat_inference_source(&mut self, cx: &mut Context<Self>) {
        self.chat_prefs.source = self.chat_prefs.source.cycle();
        self.chat_prefs = self.chat_prefs.clone().sanitize();
        self.persist_local_llm_prefs();
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
            return "モデルファイルを「ローカルLLM設定」で追加すると、ここで番号を選べます。".into();
        }
        let i = self
            .chat_prefs
            .local_model_index
            .min(self.settings_model_paths.len() - 1);
        let path = &self.settings_model_paths[i];
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("-");
        format!("選択中 [{}/{}]: {}", i + 1, self.settings_model_paths.len(), name)
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

    fn settings_chat_inference_block(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let source_label = self.chat_prefs.source.label();
        let api_disp: SharedString = if self.chat_prefs.api_model.is_empty() {
            "（空＝OpenRouter / OpenAI / 汎用それぞれの既定モデル）".into()
        } else {
            self.chat_prefs.api_model.clone().into()
        };
        let ollama_disp: SharedString = self.chat_prefs.ollama_model.clone().into();

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
                                "チャット送信時の推論先。「Ollama」は HTTP サーバ、「GGUF/ONNX」は設定に追加したファイルをネイティブ実行します。",
                            ),
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
                            .child("クラウド API モデル ID"),
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

    fn sync_editor_appearance(&self, cx: &mut Context<Self>) {
        let ap = self.appearance_prefs.clone();
        self.editor_view.update(cx, |ed, ecx| {
            ed.apply_appearance(&ap, ecx);
        });
    }

    fn cycle_appearance_theme(&mut self, cx: &mut Context<Self>) {
        self.appearance_prefs.theme = model_prefs::AppearancePrefs::cycle_theme(
            self.appearance_prefs.theme,
        );
        self.appearance_prefs.clamp();
        self.persist_local_llm_prefs();
        self.sync_editor_appearance(cx);
        cx.notify();
    }

    fn adjust_appearance_font(&mut self, delta: i32, cx: &mut Context<Self>) {
        self.appearance_prefs.font_size_px = model_prefs::AppearancePrefs::step_font_size(
            self.appearance_prefs.font_size_px,
            delta,
        );
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
            .child(self.settings_labeled_block(
                "行番号を表示",
                "エディタ左端に行番号を表示",
            ))
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

    fn settings_gpu_acceleration_row(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let on = self.hardware_params.gpu_acceleration;
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(16.))
            .child(self.settings_labeled_block(
                "GPU アクセラレーション",
                "利用可能な場合、GPUを使用",
            ))
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
                            this.hardware_params.gpu_acceleration =
                                !this.hardware_params.gpu_acceleration;
                            this.persist_local_llm_prefs();
                            cx.notify();
                        }),
                    )
                    .child(if on { "オン" } else { "オフ" }),
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
            .child(
                div()
                    .h(px(4.))
                    .w_full()
                    .rounded(px(2.))
                    .bg(hex(CONTROL_BG)),
            );
        if let Some(h) = hint {
            col = col.child(
                div()
                    .text_size(px(11.))
                    .text_color(hex(TEXT_DIM))
                    .child(h),
            );
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
            .child(
                div()
                    .h(px(4.))
                    .w_full()
                    .rounded(px(2.))
                    .bg(hex(CONTROL_BG)),
            );
        if let Some(h) = hint {
            col = col.child(
                div()
                    .text_size(px(11.))
                    .text_color(hex(TEXT_DIM))
                    .child(h),
            );
        }
        col
    }

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
                    .px(px(16.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
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
                                    .child("Settings"),
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
                                        "ローカルLLM設定",
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
                                                        "モデル形式",
                                                        "選択したモデルファイルから自動判定",
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
                                                            .child("モデルファイル"),
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
                                                            .child("モデルファイルを読み込む"),
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
                                                                    .child("読み込み済みモデル"),
                                                            )
                                                            .when(self.settings_model_paths.is_empty(), |d| {
                                                                d.child(
                                                                    div()
                                                                        .text_size(px(11.))
                                                                        .text_color(hex(TEXT_DIM))
                                                                        .child(
                                                                            "上のボタンで追加するとここに並びます（次回起動後も保持）",
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
                                                            .child("モデルパラメータ"),
                                                    )
                                                    .child(self.settings_model_param_row(
                                                        cx,
                                                        ModelParamAdjustKind::Temperature,
                                                        "Temperature",
                                                        Some("低い値ほど決定論的、高い値ほど創造的"),
                                                    ))
                                                    .child(self.settings_model_param_row(
                                                        cx,
                                                        ModelParamAdjustKind::MaxOutputTokens,
                                                        "最大トークン数",
                                                        None,
                                                    ))
                                                    .child(self.settings_model_param_row(
                                                        cx,
                                                        ModelParamAdjustKind::ContextLength,
                                                        "コンテキスト長",
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
                                                            .child("ハードウェア設定"),
                                                    )
                                                    .child(self.settings_gpu_acceleration_row(cx))
                                                    .child(self.settings_hardware_stepper_row(
                                                        cx,
                                                        HardwareParamAdjustKind::GpuLayers,
                                                        "GPU レイヤー数",
                                                        Some("GPUにオフロードするレイヤー数"),
                                                    ))
                                                    .child(self.settings_hardware_stepper_row(
                                                        cx,
                                                        HardwareParamAdjustKind::NThreads,
                                                        "スレッド数",
                                                        None,
                                                    ))
                                                    .child(self.settings_hardware_stepper_row(
                                                        cx,
                                                        HardwareParamAdjustKind::BatchSize,
                                                        "バッチサイズ",
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
                                        "外観",
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
                                        "AI設定",
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
                                                "自動補完",
                                                "AI による自動補完を有効化",
                                            ))
                                            .child(self.settings_ai_toggle_row(
                                                cx,
                                                AiToggleKind::CodeSuggestions,
                                                "コード提案",
                                                "リアルタイムのコード提案を表示",
                                            ))
                                            .child(self.settings_ai_toggle_row(
                                                cx,
                                                AiToggleKind::StreamingResponses,
                                                "ストリーミング応答",
                                                "応答をリアルタイムで表示",
                                            ))
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
                                        "APIキー管理",
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
                                                            .child("外部 API・ローカル推論（Ollama / llama.cpp 等）のキーと URL をローカルに保存します（api_keys.json の entries）。キーをコピーして「貼り付け」で取り込めます。カタログにないマイナー API は同ファイルの entries に手動で ID を追加してください。"),
                                                    )
                                                    .when(extra_keys > 0, |d| {
                                                        d.child(
                                                            div()
                                                                .min_w(px(0.))
                                                                .whitespace_normal()
                                                                .text_size(px(11.))
                                                                .text_color(hex(ACCENT_ORANGE))
                                                                .child(format!(
                                                                    "カタログ外のエントリが {extra_keys} 件あります（api_keys.json を参照）。",
                                                                )),
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
                                                            .child("登録済み（カタログ順）"),
                                                    )
                                                    .children(
                                                        self.settings_api_keys_child_elements(cx),
                                                    )
                                            )
                                    )
                            })
                            // --- アプリ情報 ---
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .child(self.settings_figma_heading(
                                        "ℹ",
                                        TEXT_SECONDARY,
                                        "アプリ情報",
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
                                                            .child("バージョン"),
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
                                                            .child("ビルド"),
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
                                    .child(
                                        div()
                                            .text_color(hex(TRAFFIC_GREEN))
                                            .child("$"),
                                    )
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
                                    .child(
                                        div()
                                            .text_color(hex(TRAFFIC_GREEN))
                                            .child("$"),
                                    )
                                    .child("_"),
                            ),
                    ),
            )
    }
}

// ============================================================
// Entry Point
// ============================================================

fn main() {
    Application::new().run(|cx: &mut App| {
        // キーバインド登録
        editor::actions::register_keybindings(cx);
        chat_composer::register_keybindings(cx);

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
                    let (file_tree, explorer_expanded) =
                        match read_tree_from_disk(&workspace_root) {
                            Ok(tree) => {
                                let exp = expanded_first_level(&tree);
                                (tree, exp)
                            }
                            Err(e) => {
                                eprintln!(
                                    "explorer: 初期読込失敗 ({e}), デモツリーを使用します"
                                );
                                (default_sample_tree(), default_expanded_set())
                            }
                        };
                    let local_llm = model_prefs::load_local_llm_prefs();
                    let api_keys = api_key_prefs::load_api_keys();
                    let appearance = local_llm.appearance.clone();
                    let editor_view = cx.new(|ecx| EditorView::new(ecx, &appearance));
                    let chat_composer = cx.new(|ecx| {
                        chat_composer::ChatComposer::new(ecx, "メッセージを入力…")
                    });
                    let _ = cx.subscribe(
                        &chat_composer,
                        |this: &mut AppView, _, _: &chat_composer::SubmitChat, cx| {
                            this.on_chat_submitted(cx);
                        },
                    );
                    AppView {
                        page: Page::Editor,
                        chat_messages: vec![ChatMsg {
                            role: "assistant".into(),
                            content: "こんにちは！Open Agents AIコーディングアシスタントです。コードの作成、編集、リファクタリングなど、お手伝いします。".into(),
                            thinking: None,
                        }],
                        chat_show_thinking: true,
                        settings_model_paths: local_llm.model_paths,
                        model_params: local_llm.model,
                        hardware_params: local_llm.hardware,
                        appearance_prefs: local_llm.appearance,
                        ai_prefs: local_llm.ai,
                        chat_prefs: local_llm.chat,
                        api_keys,
                        api_key_reveal: vec![false; api_key_prefs::PROVIDER_CATALOG.len()],
                        workspace_root,
                        file_tree,
                        explorer_expanded,
                        explorer_selection: None,
                        editor_view,
                        chat_composer,
                        chat_pending: false,
                    }
                })
            },
        )
        .unwrap();
    });
}
