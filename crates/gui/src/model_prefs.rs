//! ローカル LLM のモデルパラメータ（Temperature / 最大生成トークン / コンテキスト長）を永続化する。
//! 保存先: `%LOCALAPPDATA%\\open_agents_gui\\model_params.json`
//!
//! 将来、C 側の `oag_chat_params_t` や CLI と値を共有する際の単一ソースとして利用できる。

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

fn prefs_file() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("open_agents_gui")
        .join("model_params.json")
}

pub fn load_model_params() -> ModelParams {
    let path = prefs_file();
    let Ok(raw) = fs::read_to_string(&path) else {
        return ModelParams::default();
    };
    serde_json::from_str::<ModelParams>(&raw)
        .unwrap_or_default()
        .sanitize()
}

pub fn save_model_params(params: &ModelParams) {
    let path = prefs_file();
    let Some(parent) = path.parent() else {
        return;
    };
    if let Err(e) = fs::create_dir_all(parent) {
        eprintln!("model_prefs: ディレクトリ作成に失敗: {e}");
        return;
    }
    let mut p = params.clone();
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
}
