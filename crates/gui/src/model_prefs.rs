//! モデル・ハードウェア・外観・AI 設定を永続化する。
//! 保存先: `%LOCALAPPDATA%\\open_agents_gui\\model_params.json`
//!
//! JSON はルートにモデル用キー（flatten）とオプションの `hardware` / `appearance`
//! / `ai` オブジェクト、および `model_paths`（読み込み済みモデル一覧）を持つ。
//! 旧形式の単一 `model_path` は読み込み時に `model_paths` へ移行される。
//! 以前のフラットな `model_params.json`（モデルのみ）も `load_local_llm_prefs` で読み込み可能。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ModelParams {
    /// サンプリング温度（`oag_sampler_params_t::temperature` に相当）0.0〜2.0
    pub temperature: f32,
    /// 1 応答あたりの最大生成トークン（`oag_chat_params_t::max_tokens`）
    pub max_output_tokens: i32,
    /// プロンプト・会話で確保するコンテキスト長（トークン；KV・メモリ設計の目安）
    pub context_length: i32,
}

impl Default for ModelParams {
    fn default() -> Self {
        Self {
            temperature: 0.7,
            max_output_tokens: 2048,
            context_length: 4096,
        }
    }
}

impl ModelParams {
    pub fn clamp(&mut self) {
        self.temperature = self.temperature.clamp(0.0, 2.0);
        self.max_output_tokens = self.max_output_tokens.clamp(256, 8192);
        self.context_length = self.context_length.clamp(512, 32768);
    }

    pub fn sanitize(mut self) -> Self {
        if !self.temperature.is_finite() {
            self.temperature = Self::default().temperature;
        }
        if self.max_output_tokens <= 0 {
            self.max_output_tokens = Self::default().max_output_tokens;
        }
        if self.context_length <= 0 {
            self.context_length = Self::default().context_length;
        }
        self.clamp();
        self
    }
}

/// ローカル推論のハードウェア関連（llama.cpp 系の CLI フラグに相当する目安）
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct HardwareParams {
    /// `-ngl` 利用の ON/OFF（レイヤー 0 のときは実質 CPU のイメージでも可）
    pub gpu_acceleration: bool,
    /// GPU にオフロードするレイヤー数（`-ngl`）
    pub gpu_layers: i32,
    /// CPU スレッド数（`--threads`）
    pub n_threads: i32,
    /// 推論バッチサイズ（バックエンドが参照する場合の目安）
    pub batch_size: i32,
}

impl Default for HardwareParams {
    fn default() -> Self {
        Self {
            gpu_acceleration: true,
            gpu_layers: 32,
            n_threads: 8,
            batch_size: 512,
        }
    }
}

impl HardwareParams {
    pub fn clamp(&mut self) {
        self.gpu_layers = self.gpu_layers.clamp(0, 80);
        self.n_threads = self.n_threads.clamp(1, 32);
        self.batch_size = self.batch_size.clamp(128, 2048);
    }

    pub fn sanitize(self) -> Self {
        let mut s = self;
        s.clamp();
        s
    }
}

/// エディタ UI テーマ（`Auto` は当面 Dark と同じ配色）
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum UiTheme {
    #[default]
    Dark,
    Light,
    Auto,
}

impl UiTheme {
    pub fn label(self) -> &'static str {
        match self {
            Self::Dark => "ダーク",
            Self::Light => "ライト",
            Self::Auto => "自動",
        }
    }
}

/// 許容フォントサイズ（px）
pub const APPEARANCE_FONT_SIZES: [i32; 4] = [12, 14, 16, 18];

/// エディタ外観（`model_params.json` の `appearance`）
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AppearancePrefs {
    #[serde(default)]
    pub theme: UiTheme,
    #[serde(default = "default_font_size_px")]
    pub font_size_px: i32,
    #[serde(default = "default_show_line_numbers")]
    pub show_line_numbers: bool,
}

fn default_font_size_px() -> i32 {
    14
}

fn default_show_line_numbers() -> bool {
    true
}

impl Default for AppearancePrefs {
    fn default() -> Self {
        Self {
            theme: UiTheme::default(),
            font_size_px: default_font_size_px(),
            show_line_numbers: default_show_line_numbers(),
        }
    }
}

impl AppearancePrefs {
    pub fn sanitize(mut self) -> Self {
        if !APPEARANCE_FONT_SIZES.contains(&self.font_size_px) {
            self.font_size_px = APPEARANCE_FONT_SIZES
                .iter()
                .copied()
                .min_by(|a, b| {
                    let da = (a - self.font_size_px).abs();
                    let db = (b - self.font_size_px).abs();
                    da.cmp(&db).then_with(|| b.cmp(a))
                })
                .unwrap_or(14);
        }
        self
    }

    pub fn clamp(&mut self) {
        *self = self.clone().sanitize();
    }

    /// ステップ `delta`（-1 / +1）で許容サイズ一覧上を移動
    pub fn step_font_size(current: i32, delta: i32) -> i32 {
        let idx = APPEARANCE_FONT_SIZES
            .iter()
            .position(|&s| s == current)
            .unwrap_or(1);
        let n = idx as i32 + delta;
        let clamped = n.clamp(0, APPEARANCE_FONT_SIZES.len() as i32 - 1);
        APPEARANCE_FONT_SIZES[clamped as usize]
    }

    pub fn cycle_theme(theme: UiTheme) -> UiTheme {
        match theme {
            UiTheme::Dark => UiTheme::Light,
            UiTheme::Light => UiTheme::Auto,
            UiTheme::Auto => UiTheme::Dark,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AiPrefs {
    #[serde(default = "default_true")]
    pub auto_complete: bool,
    #[serde(default = "default_true")]
    pub code_suggestions: bool,
    #[serde(default = "default_true")]
    pub streaming_responses: bool,
}

const fn default_true() -> bool {
    true
}

impl Default for AiPrefs {
    fn default() -> Self {
        Self {
            auto_complete: true,
            code_suggestions: true,
            streaming_responses: true,
        }
    }
}

impl AiPrefs {
    pub fn sanitize(self) -> Self {
        self
    }
}

fn sanitize_model_path(path: Option<PathBuf>) -> Option<PathBuf> {
    path.and_then(|path| {
        if path.as_os_str().is_empty() {
            None
        } else {
            Some(path.canonicalize().unwrap_or(path))
        }
    })
}

fn sanitize_model_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in paths {
        if let Some(q) = sanitize_model_path(Some(p)) {
            if !out.contains(&q) {
                out.push(q);
            }
        }
    }
    out
}

/// 旧 JSON の単一 `model_path` を `model_paths` に併合する（必要なら正規化後に保存で置き換わる）。
fn migrate_prefs_json(raw: &str) -> String {
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(raw) else {
        return raw.to_string();
    };
    let Some(obj) = v.as_object_mut() else {
        return raw.to_string();
    };
    if let Some(old) = obj.remove("model_path") {
        let paths_val = obj.entry("model_paths").or_insert(serde_json::json!([]));
        if let serde_json::Value::Array(ref mut paths) = paths_val {
            if let serde_json::Value::String(s) = old {
                if !s.is_empty() && !paths.iter().any(|p| p.as_str() == Some(s.as_str())) {
                    paths.push(serde_json::Value::String(s));
                }
            }
        }
    }
    v.to_string()
}

/// モデル（ルートに flatten）＋ `hardware` / `appearance` / `ai` サブオブジェクト
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LocalLlmPrefs {
    #[serde(flatten)]
    pub model: ModelParams,
    #[serde(default)]
    pub hardware: HardwareParams,
    #[serde(default)]
    pub appearance: AppearancePrefs,
    #[serde(default)]
    pub ai: AiPrefs,
    /// 読み込み済みローカルモデル（順序保持・下に追加）
    #[serde(default)]
    pub model_paths: Vec<PathBuf>,
}

impl Default for LocalLlmPrefs {
    fn default() -> Self {
        Self {
            model: ModelParams::default(),
            hardware: HardwareParams::default(),
            appearance: AppearancePrefs::default(),
            ai: AiPrefs::default(),
            model_paths: Vec::new(),
        }
    }
}

impl LocalLlmPrefs {
    pub fn sanitize(mut self) -> Self {
        self.model = self.model.sanitize();
        self.hardware = self.hardware.sanitize();
        self.appearance = self.appearance.sanitize();
        self.ai = self.ai.sanitize();
        self.model_paths = sanitize_model_paths(std::mem::take(&mut self.model_paths));
        self
    }

    pub fn clamp(&mut self) {
        self.model.clamp();
        self.hardware.clamp();
        self.appearance.clamp();
        self.ai = self.ai.clone().sanitize();
    }
}

fn prefs_file() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("open_agents_gui")
        .join("model_params.json")
}

pub fn load_local_llm_prefs() -> LocalLlmPrefs {
    let path = prefs_file();
    let Ok(raw) = fs::read_to_string(&path) else {
        return LocalLlmPrefs::default();
    };
    let migrated = migrate_prefs_json(&raw);
    match serde_json::from_str::<LocalLlmPrefs>(&migrated) {
        Ok(p) => p.sanitize(),
        Err(_) => {
            let model = serde_json::from_str::<ModelParams>(&raw)
                .or_else(|_| serde_json::from_str::<ModelParams>(&migrated))
                .unwrap_or_default()
                .sanitize();
            LocalLlmPrefs {
                model,
                ..LocalLlmPrefs::default()
            }
            .sanitize()
        }
    }
}

pub fn save_local_llm_prefs(prefs: &LocalLlmPrefs) {
    let path = prefs_file();
    let Some(parent) = path.parent() else {
        return;
    };
    if let Err(e) = fs::create_dir_all(parent) {
        eprintln!("model_prefs: ディレクトリ作成に失敗: {e}");
        return;
    }
    let mut p = prefs.clone();
    p.clamp();
    match serde_json::to_string_pretty(&p) {
        Ok(json) => {
            if let Err(e) = fs::write(&path, json) {
                eprintln!("model_prefs: 書き込み失敗: {e}");
            }
        }
        Err(e) => eprintln!("model_prefs: JSON 生成失敗: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_clamps_temperature_and_tokens() {
        let m = ModelParams {
            temperature: 5.0,
            max_output_tokens: 999_999,
            context_length: 10,
        }
        .sanitize();
        assert!((m.temperature - 2.0).abs() < 1e-6);
        assert_eq!(m.max_output_tokens, 8192);
        assert_eq!(m.context_length, 512);
    }

    #[test]
    fn hardware_sanitize_clamps() {
        let h = HardwareParams {
            gpu_acceleration: true,
            gpu_layers: 1000,
            n_threads: 0,
            batch_size: 50,
        }
        .sanitize();
        assert_eq!(h.gpu_layers, 80);
        assert_eq!(h.n_threads, 1);
        assert_eq!(h.batch_size, 128);
    }

    #[test]
    fn legacy_flat_json_loads_model_only() {
        let raw = r#"{"temperature":0.5,"max_output_tokens":1024,"context_length":2048}"#;
        let p: LocalLlmPrefs = serde_json::from_str(raw).unwrap();
        assert!((p.model.temperature - 0.5).abs() < 1e-6);
        assert_eq!(p.model.max_output_tokens, 1024);
        assert_eq!(p.hardware, HardwareParams::default());
        assert_eq!(p.appearance, AppearancePrefs::default());
        assert_eq!(p.ai, AiPrefs::default());
        assert!(p.model_paths.is_empty());
    }

    #[test]
    fn migrates_legacy_model_path_into_model_paths() {
        let raw = r#"{"temperature":0.7,"max_output_tokens":2048,"context_length":4096,"model_path":"D:/models/x.gguf"}"#;
        let migrated = migrate_prefs_json(raw);
        let p: LocalLlmPrefs = serde_json::from_str(&migrated).unwrap();
        assert_eq!(p.model_paths.len(), 1);
        assert!(p.model_paths[0].as_os_str().to_string_lossy().contains("x.gguf"));
    }

    #[test]
    fn appearance_json_roundtrip() {
        let raw = r#"{"temperature":0.7,"max_output_tokens":2048,"context_length":4096,"hardware":{},"appearance":{"theme":"light","font_size_px":16,"show_line_numbers":false}}"#;
        let p: LocalLlmPrefs = serde_json::from_str(raw).unwrap();
        assert_eq!(p.appearance.theme, UiTheme::Light);
        assert_eq!(p.appearance.font_size_px, 16);
        assert!(!p.appearance.show_line_numbers);
    }

    #[test]
    fn appearance_sanitize_snaps_font() {
        let a = AppearancePrefs {
            theme: UiTheme::Dark,
            font_size_px: 13,
            show_line_numbers: true,
        }
        .sanitize();
        assert_eq!(a.font_size_px, 14); // 13 は 12/14 同距離のとき大きい方へスナップ
    }

    #[test]
    fn sanitize_drops_empty_model_paths_entries() {
        let p = LocalLlmPrefs {
            model_paths: vec![PathBuf::new()],
            ..LocalLlmPrefs::default()
        }
        .sanitize();
        assert!(p.model_paths.is_empty());
    }

    #[test]
    fn sanitize_dedupes_model_paths() {
        let p = LocalLlmPrefs {
            model_paths: vec![PathBuf::from("same"), PathBuf::from("same")],
            ..LocalLlmPrefs::default()
        }
        .sanitize();
        assert_eq!(p.model_paths.len(), 1);
    }
}
