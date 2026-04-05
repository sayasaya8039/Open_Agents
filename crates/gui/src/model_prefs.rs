//! ローカル LLM のモデルパラメータとハードウェア設定を永続化する。
//! 保存先: `%LOCALAPPDATA%\\open_agents_gui\\model_params.json`
//!
//! JSON はルートにモデル用キー（flatten）とオプションの `hardware` オブジェクトを持つ。
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

/// モデル（ルートに flatten）＋ `hardware` サブオブジェクト
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LocalLlmPrefs {
    #[serde(flatten)]
    pub model: ModelParams,
    #[serde(default)]
    pub hardware: HardwareParams,
}

impl Default for LocalLlmPrefs {
    fn default() -> Self {
        Self {
            model: ModelParams::default(),
            hardware: HardwareParams::default(),
        }
    }
}

impl LocalLlmPrefs {
    pub fn sanitize(mut self) -> Self {
        self.model = self.model.sanitize();
        self.hardware = self.hardware.sanitize();
        self
    }

    pub fn clamp(&mut self) {
        self.model.clamp();
        self.hardware.clamp();
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
    match serde_json::from_str::<LocalLlmPrefs>(&raw) {
        Ok(p) => p.sanitize(),
        Err(_) => {
            let model = serde_json::from_str::<ModelParams>(&raw)
                .unwrap_or_default()
                .sanitize();
            LocalLlmPrefs {
                model,
                hardware: HardwareParams::default(),
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
    }
}
