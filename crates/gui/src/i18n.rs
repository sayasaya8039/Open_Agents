//! 軽量 i18n — システム言語に応じた UI 文字列切替
//!
//! Windows: `GetUserDefaultUILanguage()` で言語を検出
//! その他: `LANG` 環境変数でフォールバック

use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Ja,
    En,
}

static LANG: OnceLock<Lang> = OnceLock::new();

/// アプリ起動時に1回呼ぶ
pub fn init() {
    LANG.get_or_init(detect_system_lang);
}

/// 現在の言語
pub fn lang() -> Lang {
    *LANG.get().unwrap_or(&Lang::En)
}

/// 日本語/英語の文字列を言語に応じて返す
pub fn t(ja: &'static str, en: &'static str) -> &'static str {
    match lang() {
        Lang::Ja => ja,
        Lang::En => en,
    }
}

/// format! が必要なケース用（String を返す）
pub fn tf(ja: String, en: String) -> String {
    match lang() {
        Lang::Ja => ja,
        Lang::En => en,
    }
}

// ================================================================
// システム言語検出
// ================================================================

#[cfg(windows)]
fn detect_system_lang() -> Lang {
    extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
    }
    let lang_id = unsafe { GetUserDefaultUILanguage() };
    let primary = lang_id & 0x3FF;
    if primary == 0x11 {
        // LANG_JAPANESE
        Lang::Ja
    } else {
        Lang::En
    }
}

#[cfg(not(windows))]
fn detect_system_lang() -> Lang {
    std::env::var("LANG")
        .ok()
        .filter(|l| l.starts_with("ja"))
        .map(|_| Lang::Ja)
        .unwrap_or(Lang::En)
}

// ================================================================
// UI 文字列定数 — カテゴリ別
// ================================================================

// ── Chat Session ──

pub fn new_conversation() -> &'static str {
    t("新しい会話", "New Conversation")
}

pub fn greeting_message() -> &'static str {
    t(
        "こんにちは！Open Agents AIコーディングアシスタントです。コードの作成、編集、リファクタリングなど、お手伝いします。",
        "Hello! I'm Open Agents AI coding assistant. I can help you create, edit, and refactor code.",
    )
}

pub fn copy_suffix() -> &'static str {
    t(" のコピー", " (Copy)")
}

pub fn group_today() -> &'static str {
    t("今日", "Today")
}

pub fn group_yesterday() -> &'static str {
    t("昨日", "Yesterday")
}

pub fn group_earlier() -> &'static str {
    t("それ以前", "Earlier")
}

// ── Chat Page — Sidebar ──

pub fn new_session() -> &'static str {
    t("New Session", "New Session")
}

pub fn delete() -> &'static str {
    t("削除", "Delete")
}

pub fn save() -> &'static str {
    t("保存", "Save")
}

pub fn cancel() -> &'static str {
    t("取消", "Cancel")
}

pub fn rename() -> &'static str {
    t("名前変更", "Rename")
}

pub fn duplicate() -> &'static str {
    t("複製", "Duplicate")
}

pub fn export() -> &'static str {
    t("エクスポート", "Export")
}

pub fn show_in_explorer() -> &'static str {
    t("エクスプローラーで表示", "Show in Explorer")
}

pub fn settings() -> &'static str {
    t("設定", "Settings")
}

// ── Chat Page — Messages ──

pub fn thinking_show() -> &'static str {
    t("思考: 表示", "Thinking: Show")
}

pub fn thinking_hide() -> &'static str {
    t("思考: 非表示", "Thinking: Hide")
}

pub fn suggestions() -> &'static str {
    t("提案", "Suggestions")
}

pub fn suggestion_create_react() -> &'static str {
    t("Reactコンポーネントを作成", "Create React Component")
}

pub fn suggestion_fix_bug() -> &'static str {
    t("バグを修正", "Fix Bug")
}

pub fn suggestion_refactor() -> &'static str {
    t("コードをリファクタリング", "Refactor Code")
}

pub fn suggestion_add_tests() -> &'static str {
    t("テストを追加", "Add Tests")
}

pub fn older_messages(skip: usize) -> String {
    tf(
        format!("↑ 過去 {skip} 件のメッセージ"),
        format!("↑ {skip} older messages"),
    )
}

pub fn role_you() -> &'static str {
    t("You", "You")
}

pub fn role_agent() -> &'static str {
    t("Agent", "Agent")
}

pub fn next_model() -> &'static str {
    t("▶ 次のモデル", "▶ Next Model")
}

pub fn send() -> &'static str {
    t("送信", "Send")
}

pub fn newline() -> &'static str {
    t("改行", "Newline")
}

// ── Chat Page — Metrics ──

pub fn metric_tokens(count: u32) -> String {
    tf(
        format!("トークン数: {count} トークン"),
        format!("Tokens: {count}"),
    )
}

pub fn metric_tokens_per_sec(rate: f64) -> String {
    tf(
        format!("トークン毎秒: {rate:.2} トークン/秒"),
        format!("Speed: {rate:.2} tok/s"),
    )
}

pub fn metric_response_time(time: &str) -> String {
    tf(
        format!("応答時間: {time}"),
        format!("Response: {time}"),
    )
}

pub fn metric_stop_reason(reason: &str) -> String {
    tf(
        format!("停止理由: {reason}"),
        format!("Stop: {reason}"),
    )
}

// ── Chat Composer ──

pub fn placeholder_message() -> &'static str {
    t("メッセージを入力…", "Type a message…")
}

// ── Model Prefs — KV Cache ──

pub fn kv_cache_label(variant: &str) -> &'static str {
    match variant {
        "none" => t("FP16（標準）", "FP16 (Standard)"),
        "q8" => t("Q8（2x圧縮）", "Q8 (2x Compression)"),
        "q4" => t("Q4（4x圧縮）", "Q4 (4x Compression)"),
        "turbo3" => t("Turbo3（4.9x圧縮・推奨）", "Turbo3 (4.9x, Recommended)"),
        "turbo4" => t("Turbo4（3.8x圧縮・高品質）", "Turbo4 (3.8x, High Quality)"),
        "turbo2" => t("Turbo2（6.4x圧縮・最大）", "Turbo2 (6.4x, Maximum)"),
        _ => "Unknown",
    }
}

// ── Model Prefs — Runtime Preset ──

pub fn runtime_preset_label(variant: &str) -> &'static str {
    match variant {
        "auto" => t("自動検出（推奨）", "Auto Detect (Recommended)"),
        "nvidia_cuda" => "NVIDIA CUDA",
        "vulkan_hybrid" => t("Vulkan 混成（実験）", "Vulkan Hybrid (Experimental)"),
        "cpu_only" => t("CPU のみ", "CPU Only"),
        _ => "Unknown",
    }
}

pub fn runtime_preset_subtitle(variant: &str) -> &'static str {
    match variant {
        "auto" => t(
            "起動時に GPU を検出して最適な設定を自動適用",
            "Auto-detect GPU at startup and apply optimal settings",
        ),
        "nvidia_cuda" => t(
            "CUDA 単独で速度を優先（NVIDIA GPU 必須）",
            "CUDA-only for speed (NVIDIA GPU required)",
        ),
        "vulkan_hybrid" => t(
            "Vulkan で複数 GPU の混成を試す",
            "Try multi-GPU hybrid via Vulkan",
        ),
        "cpu_only" => t(
            "GPU を使わず CPU のみで推論する省電力モード",
            "CPU-only inference for power saving",
        ),
        _ => "",
    }
}

pub fn gpu_not_detected() -> &'static str {
    t("GPU 未検出 — CPU のみ", "No GPU detected — CPU only")
}

// ── Model Prefs — Power Mode ──

pub fn power_balanced() -> &'static str {
    t("バランス", "Balanced")
}

pub fn power_max_performance() -> &'static str {
    t("最大性能（AC接続推奨）", "Max Performance (AC recommended)")
}

// ── Model Prefs — Theme ──

pub fn theme_dark() -> &'static str {
    t("ダーク", "Dark")
}

pub fn theme_light() -> &'static str {
    t("ライト", "Light")
}

pub fn theme_auto() -> &'static str {
    t("自動", "Auto")
}

// ── Model Prefs — CoT ──

pub fn cot_off() -> &'static str {
    t("無効", "Off")
}

pub fn cot_basic() -> &'static str {
    t("基本（推奨）", "Basic (Recommended)")
}

pub fn cot_detailed() -> &'static str {
    t("詳細（4段階推論）", "Detailed (4-Step Reasoning)")
}

pub fn cot_subtitle_off() -> &'static str {
    t("CoT なし — 直接回答する", "No CoT — direct answer")
}

pub fn cot_subtitle_basic() -> &'static str {
    t(
        "ステップバイステップで考えてから回答する",
        "Think step by step before answering",
    )
}

pub fn cot_subtitle_detailed() -> &'static str {
    t(
        "分解→仮説→検証→結論の4段階で推論する",
        "Decompose → Hypothesize → Verify → Conclude",
    )
}

// ── Model Prefs — ToT ──

pub fn tot_off() -> &'static str {
    cot_off()
}

pub fn tot_branch2() -> &'static str {
    t("2経路（高速）", "2 Branches (Fast)")
}

pub fn tot_branch3() -> &'static str {
    t("3経路（標準）", "3 Branches (Standard)")
}

pub fn tot_subtitle_off() -> &'static str {
    t("ToT なし — 通常の 1 経路推論", "No ToT — single-path reasoning")
}

pub fn tot_subtitle_branch2() -> &'static str {
    t(
        "2つの思考経路を探索して最良を合成",
        "Explore 2 thought paths and synthesize the best",
    )
}

pub fn tot_subtitle_branch3() -> &'static str {
    t(
        "3つの思考経路を探索して最良を合成",
        "Explore 3 thought paths and synthesize the best",
    )
}

// ── Model Prefs — ReAct ──

pub fn react_off() -> &'static str {
    cot_off()
}

pub fn react_steps3() -> &'static str {
    t("最大3ステップ", "Max 3 Steps")
}

pub fn react_steps5() -> &'static str {
    t("最大5ステップ", "Max 5 Steps")
}

pub fn react_subtitle_off() -> &'static str {
    t("ツール不使用の通常推論", "Standard reasoning without tools")
}

pub fn react_subtitle_steps3() -> &'static str {
    t(
        "検索・計算を最大3回使って回答",
        "Use search/calc up to 3 times",
    )
}

pub fn react_subtitle_steps5() -> &'static str {
    t(
        "検索・計算を最大5回使って回答",
        "Use search/calc up to 5 times",
    )
}

// ── Model Prefs — Self-Consistency ──

pub fn sc_off() -> &'static str {
    cot_off()
}

pub fn sc_vote3() -> &'static str {
    t("3回投票", "3 Votes")
}

pub fn sc_vote5() -> &'static str {
    t("5回投票", "5 Votes")
}

pub fn sc_subtitle_off() -> &'static str {
    t("1回の推論で回答（高速）", "Single inference (fast)")
}

pub fn sc_subtitle_vote3() -> &'static str {
    t(
        "3回推論して最も一致する回答を採用",
        "Run 3 inferences and pick the consensus answer",
    )
}

pub fn sc_subtitle_vote5() -> &'static str {
    t(
        "5回推論して最も一致する回答を採用（高品質）",
        "Run 5 inferences and pick the consensus answer (high quality)",
    )
}

// ── Model Prefs — Chat Inference Source ──

pub fn source_cloud_api() -> &'static str {
    t("クラウド API", "Cloud API")
}

pub fn source_ollama() -> &'static str {
    "Ollama (HTTP)"
}

pub fn source_local_gguf() -> &'static str {
    t("ローカル GGUF/ONNX", "Local GGUF/ONNX")
}

// ── API Key Prefs — Groups ──

pub fn api_group_cloud_primary() -> &'static str {
    t("クラウド LLM（主要）", "Cloud LLM (Primary)")
}

pub fn api_group_cloud_extra() -> &'static str {
    t("クラウド LLM・推論（追加）", "Cloud LLM (Additional)")
}

pub fn api_group_local() -> &'static str {
    t(
        "ローカル / セルフホスト（Ollama・Llama）",
        "Local / Self-Hosted (Ollama / Llama)",
    )
}

pub fn api_group_openai_compat() -> &'static str {
    t("OpenAI 互換（汎用）", "OpenAI Compatible (Generic)")
}

pub fn api_group_azure_aws() -> &'static str {
    "Azure / AWS"
}

pub fn api_group_hub() -> &'static str {
    t("Hub・Embedding・検索", "Hub / Embedding / Search")
}

pub fn not_set() -> &'static str {
    t("（未設定）", "(Not Set)")
}

// ── Editor ──

pub fn editor_welcome() -> &'static str {
    t(
        "// Open Agents エディタへようこそ！\n// ここにコードを書いてください。\n\nfn main() {\n    println!(\"Hello, world!\");\n}\n",
        "// Welcome to Open Agents Editor!\n// Write your code here.\n\nfn main() {\n    println!(\"Hello, world!\");\n}\n",
    )
}

pub fn untitled() -> &'static str {
    t("無題", "Untitled")
}

pub fn file_read_error() -> &'static str {
    t("ファイル読み込みエラー", "File read error")
}

pub fn save_error() -> &'static str {
    t("保存エラー", "Save error")
}

// ── Session Title Editor ──

pub fn session_name_placeholder() -> &'static str {
    t("セッション名", "Session Name")
}

// ── Settings Page (main.rs) ──

pub fn settings_local_llm() -> &'static str {
    t("ローカルLLM設定", "Local LLM Settings")
}

pub fn settings_model_format() -> &'static str {
    t("モデル形式", "Model Format")
}

pub fn settings_model_format_hint() -> &'static str {
    t(
        "選択したモデルファイルから自動判定",
        "Auto-detected from selected model file",
    )
}

pub fn settings_model_file() -> &'static str {
    t("モデルファイル", "Model File")
}

pub fn settings_load_model() -> &'static str {
    t("モデルファイルを読み込む", "Load Model File")
}

pub fn settings_loaded_models() -> &'static str {
    t("読み込み済みモデル", "Loaded Models")
}

pub fn settings_loaded_models_hint() -> &'static str {
    t(
        "上のボタンで追加するとここに並びます（次回起動後も保持）",
        "Add models above — they persist across restarts",
    )
}

pub fn settings_model_params() -> &'static str {
    t("モデルパラメータ", "Model Parameters")
}

pub fn settings_temperature_hint() -> &'static str {
    t(
        "低い値ほど決定論的、高い値ほど創造的",
        "Lower = more deterministic, higher = more creative",
    )
}

pub fn settings_max_tokens() -> &'static str {
    t("最大トークン数", "Max Tokens")
}

pub fn settings_max_tokens_hint() -> &'static str {
    t(
        "ローカル GGUF は 256〜512 推奨。大きい値は応答完了まで極端に遅くなります",
        "256-512 recommended for local GGUF. Larger values dramatically slow down responses",
    )
}

pub fn settings_context_length() -> &'static str {
    t("コンテキスト長", "Context Length")
}

pub fn settings_hardware() -> &'static str {
    t("ハードウェア設定", "Hardware Settings")
}

pub fn settings_gpu_layers() -> &'static str {
    t("GPU レイヤー数", "GPU Layers")
}

pub fn settings_gpu_layers_hint() -> &'static str {
    t(
        "選択中モードの llama-server に対して --n-gpu-layers へ渡します。混成モードでもレイヤー数の上限として扱います。",
        "Passed as --n-gpu-layers to llama-server. Also used as layer cap in hybrid mode.",
    )
}

pub fn settings_threads() -> &'static str {
    t("スレッド数", "Threads")
}

pub fn settings_batch_size() -> &'static str {
    t("バッチサイズ", "Batch Size")
}

pub fn settings_appearance() -> &'static str {
    t("外観", "Appearance")
}

pub fn settings_ai() -> &'static str {
    t("AI設定", "AI Settings")
}

pub fn settings_auto_complete() -> &'static str {
    t("自動補完", "Auto Complete")
}

pub fn settings_auto_complete_hint() -> &'static str {
    t("AI による自動補完を有効化", "Enable AI auto-completion")
}

pub fn settings_code_suggestions() -> &'static str {
    t("コード提案", "Code Suggestions")
}

pub fn settings_code_suggestions_hint() -> &'static str {
    t(
        "リアルタイムのコード提案を表示",
        "Show real-time code suggestions",
    )
}

pub fn settings_streaming() -> &'static str {
    t("ストリーミング応答", "Streaming Responses")
}

pub fn settings_streaming_hint() -> &'static str {
    t("応答をリアルタイムで表示", "Display responses in real-time")
}

pub fn settings_api_keys() -> &'static str {
    t("APIキー管理", "API Key Management")
}

pub fn settings_api_keys_description() -> &'static str {
    t(
        "外部 API・ローカル推論（Ollama / llama.cpp 等）のキーと URL をローカルに保存します（api_keys.json の entries）。キーをコピーして「貼り付け」で取り込めます。カタログにないマイナー API は同ファイルの entries に手動で ID を追加してください。",
        "Store API keys and URLs for external APIs and local inference (Ollama / llama.cpp etc.) locally in api_keys.json. Paste keys to import. For unlisted APIs, manually add IDs to the entries in the JSON file.",
    )
}

pub fn settings_extra_entries(count: usize) -> String {
    tf(
        format!("カタログ外のエントリが {count} 件あります（api_keys.json を参照）。"),
        format!("{count} extra entries found outside the catalog (see api_keys.json)."),
    )
}

pub fn settings_registered_catalog() -> &'static str {
    t("登録済み（カタログ順）", "Registered (Catalog Order)")
}

pub fn settings_app_info() -> &'static str {
    t("アプリ情報", "App Info")
}

pub fn settings_version() -> &'static str {
    t("バージョン", "Version")
}

pub fn settings_build() -> &'static str {
    t("ビルド", "Build")
}

// ── CoT System Instructions (always Japanese — sent to LLM) ──
// These are NOT translated because they're prompts for the LLM, not UI strings.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_returns_static_str() {
        // init() 未呼び出し → En フォールバック
        let s = t("日本語", "English");
        assert!(s == "日本語" || s == "English");
    }

    #[test]
    fn tf_returns_string() {
        let s = tf("テスト1".to_string(), "test1".to_string());
        assert!(s == "テスト1" || s == "test1");
    }
}
