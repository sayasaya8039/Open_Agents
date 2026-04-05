//! 外部 API キーのローカル保存（平文 JSON）。
//! 保存先: `%LOCALAPPDATA%\\open_agents_gui\\api_keys.json`
//!
//! 共有 PC・バックアップ取り扱いに注意（OS の資格情報マネージャーは未使用）。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ApiKeyPrefs {
    pub openai: String,
    pub anthropic: String,
    pub google: String,
    pub openrouter: String,
}

impl ApiKeyPrefs {
    pub fn sanitize(mut self) -> Self {
        self.openai = self.openai.trim().to_string();
        self.anthropic = self.anthropic.trim().to_string();
        self.google = self.google.trim().to_string();
        self.openrouter = self.openrouter.trim().to_string();
        self
    }
}

fn keys_file() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("open_agents_gui")
        .join("api_keys.json")
}

pub fn load_api_keys() -> ApiKeyPrefs {
    let path = keys_file();
    let Ok(raw) = fs::read_to_string(&path) else {
        return ApiKeyPrefs::default();
    };
    serde_json::from_str::<ApiKeyPrefs>(&raw)
        .unwrap_or_default()
        .sanitize()
}

pub fn save_api_keys(prefs: &ApiKeyPrefs) {
    let path = keys_file();
    let Some(parent) = path.parent() else {
        return;
    };
    if let Err(e) = fs::create_dir_all(parent) {
        eprintln!("api_key_prefs: ディレクトリ作成に失敗: {e}");
        return;
    }
    let p = prefs.clone().sanitize();
    match serde_json::to_string_pretty(&p) {
        Ok(json) => {
            if let Err(e) = fs::write(&path, json) {
                eprintln!("api_key_prefs: 書き込み失敗: {e}");
            }
        }
        Err(e) => eprintln!("api_key_prefs: JSON 生成失敗: {e}"),
    }
}

/// マスク表示用（全文表示でないときは末尾4文字のみ判読可）
pub fn masked_line(key: &str, reveal_full: bool) -> String {
    if key.is_empty() {
        return "（未設定）".into();
    }
    if reveal_full {
        return key.to_string();
    }
    let count = key.chars().count();
    if count <= 4 {
        return "•".repeat(count);
    }
    let last: String = key
        .chars()
        .rev()
        .take(4)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    let n_hidden = count.saturating_sub(4);
    let dots = "•".repeat(n_hidden.min(12).max(1));
    format!("{dots}…{last}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_trims() {
        let p = ApiKeyPrefs {
            openai: "  sk-xx  \n".into(),
            ..Default::default()
        }
        .sanitize();
        assert_eq!(p.openai, "sk-xx");
    }

    #[test]
    fn masked_empty_and_tail() {
        assert_eq!(masked_line("", false), "（未設定）");
        assert_eq!(masked_line("abcd", false), "••••");
        let m = masked_line("sk-verylongsecret", false);
        assert!(m.contains('…'));
        assert!(m.ends_with("cret"));
    }
}
