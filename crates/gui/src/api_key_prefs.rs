//! 外部 API・ローカル推論エンドポイントの資格情報をローカル保存（平文 JSON）。
//! 保存先: `%LOCALAPPDATA%\\open_agents_gui\\api_keys.json`
//!
//! - `entries`: プロバイダ ID → 値（API キーまたはベース URL）。**カタログ外の ID も保存可能**（マイナー API を JSON で追加可能）。
//! - 旧形式 `{ openai, anthropic, google, openrouter }` は読込時に `entries` へ移行。
//!
//! 共有 PC・バックアップ・スクリーン共有に注意。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use crate::i18n;

/// 設定 UI および推奨環境変数名のメタデータ（値は `entries` に格納）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CredentialKind {
    /// API キー / トークン
    SecretToken,
    /// ベース URL（Ollama / llama.cpp / vLLM 等）
    BaseUrl,
}

#[derive(Clone, Copy, Debug)]
pub struct ProviderDef {
    pub id: &'static str,
    pub title: &'static str,
    pub env_hint: &'static str,
    pub kind: CredentialKind,
    pub group: &'static str,
}

/// 設定画面に出す行の順序（グループは表示時に見出しを挟む）
pub const PROVIDER_CATALOG: &[ProviderDef] = &[
    // --- クラウド LLM（主要）---
    ProviderDef {
        id: "openai",
        title: "OpenAI",
        env_hint: "OPENAI_API_KEY",
        kind: CredentialKind::SecretToken,
        group: "クラウド LLM（主要）",
    },
    ProviderDef {
        id: "anthropic",
        title: "Anthropic (Claude)",
        env_hint: "ANTHROPIC_API_KEY",
        kind: CredentialKind::SecretToken,
        group: "クラウド LLM（主要）",
    },
    ProviderDef {
        id: "google_gemini",
        title: "Google Gemini",
        env_hint: "GOOGLE_API_KEY / GEMINI_API_KEY",
        kind: CredentialKind::SecretToken,
        group: "クラウド LLM（主要）",
    },
    ProviderDef {
        id: "openrouter",
        title: "OpenRouter",
        env_hint: "OPENROUTER_API_KEY",
        kind: CredentialKind::SecretToken,
        group: "クラウド LLM（主要）",
    },
    ProviderDef {
        id: "groq",
        title: "Groq",
        env_hint: "GROQ_API_KEY",
        kind: CredentialKind::SecretToken,
        group: "クラウド LLM（主要）",
    },
    ProviderDef {
        id: "mistral",
        title: "Mistral AI",
        env_hint: "MISTRAL_API_KEY",
        kind: CredentialKind::SecretToken,
        group: "クラウド LLM（主要）",
    },
    ProviderDef {
        id: "cohere",
        title: "Cohere",
        env_hint: "COHERE_API_KEY",
        kind: CredentialKind::SecretToken,
        group: "クラウド LLM（主要）",
    },
    ProviderDef {
        id: "deepseek",
        title: "DeepSeek",
        env_hint: "DEEPSEEK_API_KEY",
        kind: CredentialKind::SecretToken,
        group: "クラウド LLM（主要）",
    },
    ProviderDef {
        id: "xai",
        title: "xAI (Grok)",
        env_hint: "XAI_API_KEY",
        kind: CredentialKind::SecretToken,
        group: "クラウド LLM（主要）",
    },
    // --- クラウド LLM・推論（追加）---
    ProviderDef {
        id: "perplexity",
        title: "Perplexity",
        env_hint: "PERPLEXITY_API_KEY",
        kind: CredentialKind::SecretToken,
        group: "クラウド LLM・推論（追加）",
    },
    ProviderDef {
        id: "fireworks",
        title: "Fireworks AI",
        env_hint: "FIREWORKS_API_KEY",
        kind: CredentialKind::SecretToken,
        group: "クラウド LLM・推論（追加）",
    },
    ProviderDef {
        id: "together",
        title: "Together AI",
        env_hint: "TOGETHER_API_KEY",
        kind: CredentialKind::SecretToken,
        group: "クラウド LLM・推論（追加）",
    },
    ProviderDef {
        id: "replicate",
        title: "Replicate",
        env_hint: "REPLICATE_API_TOKEN",
        kind: CredentialKind::SecretToken,
        group: "クラウド LLM・推論（追加）",
    },
    ProviderDef {
        id: "anyscale",
        title: "Anyscale",
        env_hint: "ANYSCALE_API_KEY",
        kind: CredentialKind::SecretToken,
        group: "クラウド LLM・推論（追加）",
    },
    ProviderDef {
        id: "moonshot",
        title: "Moonshot (Kimi)",
        env_hint: "MOONSHOT_API_KEY",
        kind: CredentialKind::SecretToken,
        group: "クラウド LLM・推論（追加）",
    },
    ProviderDef {
        id: "zhipu",
        title: "Zhipu GLM",
        env_hint: "ZHIPU_API_KEY",
        kind: CredentialKind::SecretToken,
        group: "クラウド LLM・推論（追加）",
    },
    ProviderDef {
        id: "siliconflow",
        title: "SiliconFlow",
        env_hint: "SILICONFLOW_API_KEY",
        kind: CredentialKind::SecretToken,
        group: "クラウド LLM・推論（追加）",
    },
    ProviderDef {
        id: "novita",
        title: "Novita AI",
        env_hint: "NOVITA_API_KEY",
        kind: CredentialKind::SecretToken,
        group: "クラウド LLM・推論（追加）",
    },
    ProviderDef {
        id: "nebius",
        title: "Nebius",
        env_hint: "NEBIUS_API_KEY",
        kind: CredentialKind::SecretToken,
        group: "クラウド LLM・推論（追加）",
    },
    // --- ローカル / セルフホスト（Ollama・Llama 系）---
    ProviderDef {
        id: "ollama_base_url",
        title: "Ollama ベース URL",
        env_hint: "OLLAMA_HOST（例: http://127.0.0.1:11434）",
        kind: CredentialKind::BaseUrl,
        group: "ローカル / セルフホスト（Ollama・Llama）",
    },
    ProviderDef {
        id: "ollama_api_key",
        title: "Ollama API キー",
        env_hint: "OLLAMA_API_KEY（任意・クラウド時など）",
        kind: CredentialKind::SecretToken,
        group: "ローカル / セルフホスト（Ollama・Llama）",
    },
    ProviderDef {
        id: "llama_cpp_server_url",
        title: "llama.cpp サーバー URL",
        env_hint: "LLAMA_SERVER_URL（llama-server 等）",
        kind: CredentialKind::BaseUrl,
        group: "ローカル / セルフホスト（Ollama・Llama）",
    },
    ProviderDef {
        id: "llama_cpp_api_key",
        title: "llama.cpp Bearer トークン",
        env_hint: "任意（リバースプロキシ認証用）",
        kind: CredentialKind::SecretToken,
        group: "ローカル / セルフホスト（Ollama・Llama）",
    },
    ProviderDef {
        id: "lm_studio_base_url",
        title: "LM Studio API URL",
        env_hint: "例: http://127.0.0.1:1234/v1",
        kind: CredentialKind::BaseUrl,
        group: "ローカル / セルフホスト（Ollama・Llama）",
    },
    ProviderDef {
        id: "vllm_base_url",
        title: "vLLM（OpenAI 互換）ベース URL",
        env_hint: "例: http://127.0.0.1:8000/v1",
        kind: CredentialKind::BaseUrl,
        group: "ローカル / セルフホスト（Ollama・Llama）",
    },
    ProviderDef {
        id: "text_generation_inference_url",
        title: "Hugging Face TGI URL",
        env_hint: "TGI / Router ベース URL",
        kind: CredentialKind::BaseUrl,
        group: "ローカル / セルフホスト（Ollama・Llama）",
    },
    // --- OpenAI 互換（マイナー・自己ホスト集約）---
    ProviderDef {
        id: "generic_openai_base_url",
        title: "OpenAI 互換ベース URL",
        env_hint: "LiteLLM / 未掲載プロバータ向け",
        kind: CredentialKind::BaseUrl,
        group: "OpenAI 互換（汎用）",
    },
    ProviderDef {
        id: "generic_openai_api_key",
        title: "OpenAI 互換 API キー",
        env_hint: "上記 URL 用のキー",
        kind: CredentialKind::SecretToken,
        group: "OpenAI 互換（汎用）",
    },
    // --- Azure / AWS ---
    ProviderDef {
        id: "azure_openai_endpoint",
        title: "Azure OpenAI エンドポイント",
        env_hint: "AZURE_OPENAI_ENDPOINT",
        kind: CredentialKind::BaseUrl,
        group: "Azure / AWS",
    },
    ProviderDef {
        id: "azure_openai_key",
        title: "Azure OpenAI API キー",
        env_hint: "AZURE_OPENAI_API_KEY",
        kind: CredentialKind::SecretToken,
        group: "Azure / AWS",
    },
    ProviderDef {
        id: "aws_access_key_id",
        title: "AWS Access Key ID",
        env_hint: "AWS_ACCESS_KEY_ID（Bedrock 等）",
        kind: CredentialKind::SecretToken,
        group: "Azure / AWS",
    },
    ProviderDef {
        id: "aws_secret_access_key",
        title: "AWS Secret Access Key",
        env_hint: "AWS_SECRET_ACCESS_KEY",
        kind: CredentialKind::SecretToken,
        group: "Azure / AWS",
    },
    ProviderDef {
        id: "aws_region",
        title: "AWS リージョン",
        env_hint: "AWS_REGION（例: us-east-1）",
        kind: CredentialKind::SecretToken,
        group: "Azure / AWS",
    },
    // --- Hub・検索・その他 ---
    ProviderDef {
        id: "huggingface",
        title: "Hugging Face Hub",
        env_hint: "HF_TOKEN / HUGGINGFACE_HUB_TOKEN",
        kind: CredentialKind::SecretToken,
        group: "Hub・Embedding・検索",
    },
    ProviderDef {
        id: "voyage",
        title: "Voyage AI（Embedding）",
        env_hint: "VOYAGE_API_KEY",
        kind: CredentialKind::SecretToken,
        group: "Hub・Embedding・検索",
    },
    ProviderDef {
        id: "jina",
        title: "Jina AI",
        env_hint: "JINA_API_KEY",
        kind: CredentialKind::SecretToken,
        group: "Hub・Embedding・検索",
    },
    ProviderDef {
        id: "tavily",
        title: "Tavily Search",
        env_hint: "TAVILY_API_KEY",
        kind: CredentialKind::SecretToken,
        group: "Hub・Embedding・検索",
    },
    ProviderDef {
        id: "serper",
        title: "Serper (Google Search JSON API)",
        env_hint: "SERPER_API_KEY",
        kind: CredentialKind::SecretToken,
        group: "Hub・Embedding・検索",
    },
    ProviderDef {
        id: "brave_search",
        title: "Brave Search API",
        env_hint: "BRAVE_API_KEY",
        kind: CredentialKind::SecretToken,
        group: "Hub・Embedding・検索",
    },
];

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiKeyPrefs {
    /// プロバイダ ID → 秘密値または URL。カタログ外キーも許容。
    #[serde(default)]
    pub entries: BTreeMap<String, String>,
}

impl ApiKeyPrefs {
    pub fn get_str(&self, id: &str) -> &str {
        self.entries.get(id).map(|s| s.as_str()).unwrap_or("")
    }

    /// entries → 環境変数の順で値を解決し `String` で返す
    pub fn get_value(&self, id: &str) -> String {
        if let Some(v) = self.entries.get(id) {
            if !v.is_empty() {
                return v.clone();
            }
        }
        if let Some(def) = PROVIDER_CATALOG.iter().find(|p| p.id == id) {
            for name in extract_env_var_names(def.env_hint) {
                if let Ok(val) = std::env::var(&name) {
                    if !val.is_empty() {
                        return val;
                    }
                }
            }
        }
        String::new()
    }

    /// entries に値がなく環境変数に値がある場合 `true`
    pub fn is_from_env(&self, id: &str) -> bool {
        if let Some(v) = self.entries.get(id) {
            if !v.is_empty() {
                return false;
            }
        }
        if let Some(def) = PROVIDER_CATALOG.iter().find(|p| p.id == id) {
            for name in extract_env_var_names(def.env_hint) {
                if let Ok(val) = std::env::var(&name) {
                    if !val.is_empty() {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// 環境変数から解決されている場合、その変数名を返す
    pub fn active_env_var_name(&self, id: &str) -> Option<String> {
        if let Some(v) = self.entries.get(id) {
            if !v.is_empty() {
                return None;
            }
        }
        if let Some(def) = PROVIDER_CATALOG.iter().find(|p| p.id == id) {
            for name in extract_env_var_names(def.env_hint) {
                if let Ok(val) = std::env::var(&name) {
                    if !val.is_empty() {
                        return Some(name);
                    }
                }
            }
        }
        None
    }

    pub fn set_entry(&mut self, id: &str, value: String) {
        let v = value.trim().to_string();
        if v.is_empty() {
            self.entries.remove(id);
        } else {
            self.entries.insert(id.to_string(), v);
        }
    }

    pub fn sanitize(mut self) -> Self {
        self.entries = self
            .entries
            .into_iter()
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .filter(|(k, v)| !k.is_empty() && !v.is_empty())
            .collect();
        self
    }
}

/// env_hint 文字列から環境変数名を抽出する。
/// `[A-Z][A-Z0-9_]+` にマッチする2文字以上のトークンを返す。
pub fn extract_env_var_names(env_hint: &str) -> Vec<String> {
    let mut names = Vec::new();
    let chars: Vec<char> = env_hint.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_uppercase() {
            let start = i;
            i += 1;
            while i < chars.len()
                && (chars[i].is_ascii_uppercase()
                    || chars[i].is_ascii_digit()
                    || chars[i] == '_')
            {
                i += 1;
            }
            if i - start >= 2 {
                names.push(chars[start..i].iter().collect());
            }
        } else {
            i += 1;
        }
    }
    names
}

fn keys_file() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("open_agents_gui")
        .join("api_keys.json")
}

/// 旧トップレベル `openai` 等のみの JSON を `entries` に変換（既に `entries` がある場合は None）
fn migrate_legacy_flat_json(val: &serde_json::Value) -> Option<BTreeMap<String, String>> {
    let obj = val.as_object()?;
    if obj.contains_key("entries") {
        return None;
    }
    let keys = ["openai", "anthropic", "google", "openrouter"];
    if !keys.iter().any(|k| obj.contains_key(*k)) {
        return None;
    }
    let mut m = BTreeMap::new();
    for k in keys {
        let Some(v) = obj.get(k) else {
            continue;
        };
        let Some(s) = v.as_str() else {
            continue;
        };
        if s.is_empty() {
            continue;
        }
        let id = if k == "google" { "google_gemini" } else { k };
        m.insert(id.to_string(), s.to_string());
    }
    Some(m)
}

pub fn load_api_keys() -> ApiKeyPrefs {
    let path = keys_file();
    let Ok(raw) = fs::read_to_string(&path) else {
        return ApiKeyPrefs::default();
    };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return ApiKeyPrefs::default();
    };

    let prefs = if let Some(map) = migrate_legacy_flat_json(&val) {
        ApiKeyPrefs { entries: map }
    } else {
        serde_json::from_value::<ApiKeyPrefs>(val).unwrap_or_default()
    };
    prefs.sanitize()
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

/// グループ ID（日本語）を現在の言語に翻訳する
pub fn translate_group(group: &str) -> &'static str {
    match group {
        "クラウド LLM（主要）" => i18n::api_group_cloud_primary(),
        "クラウド LLM・推論（追加）" => i18n::api_group_cloud_extra(),
        "ローカル / セルフホスト（Ollama・Llama）" => i18n::api_group_local(),
        "OpenAI 互換（汎用）" => i18n::api_group_openai_compat(),
        "Azure / AWS" => i18n::api_group_azure_aws(),
        "Hub・Embedding・検索" => i18n::api_group_hub(),
        _ => i18n::api_group_cloud_primary(), // fallback
    }
}

/// マスク表示用（URL・トークン共通）
pub fn masked_line(key: &str, reveal_full: bool) -> String {
    if key.is_empty() {
        return i18n::not_set().into();
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

/// カタログに無いが `entries` に残っているキー数（拡張・手編集用）
pub fn extra_entry_count(prefs: &ApiKeyPrefs) -> usize {
    let known: std::collections::HashSet<&'static str> =
        PROVIDER_CATALOG.iter().map(|p| p.id).collect();
    prefs
        .entries
        .keys()
        .filter(|k| !known.contains(k.as_str()))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_entry_roundtrip() {
        let mut p = ApiKeyPrefs::default();
        p.set_entry("openai", "  sk-xx  \n".into());
        assert_eq!(p.get_str("openai"), "sk-xx");
        p.set_entry("openai", "  ".into());
        assert_eq!(p.get_str("openai"), "");
    }

    #[test]
    fn masked_empty_and_tail() {
        assert_eq!(masked_line("", false), "（未設定）");
        assert_eq!(masked_line("abcd", false), "••••");
        let m = masked_line("sk-verylongsecret", false);
        assert!(m.contains('…'));
        assert!(m.ends_with("cret"));
    }

    #[test]
    fn migrates_legacy_top_level_google_to_gemini_id() {
        let raw = r#"{"openai":"sk-a","anthropic":"","google":"g","openrouter":"r"}"#;
        let v: serde_json::Value = serde_json::from_str(raw).unwrap();
        let m = migrate_legacy_flat_json(&v).unwrap();
        assert_eq!(m.get("openai").map(String::as_str), Some("sk-a"));
        assert_eq!(m.get("google_gemini").map(String::as_str), Some("g"));
        assert_eq!(m.get("openrouter").map(String::as_str), Some("r"));
    }

    #[test]
    fn entries_format_loads() {
        let raw = r#"{"entries":{"openrouter":"x","custom_minor":"y"}}"#;
        let p: ApiKeyPrefs = serde_json::from_str(raw).unwrap();
        assert_eq!(p.get_str("openrouter"), "x");
        assert_eq!(p.get_str("custom_minor"), "y");
    }

    #[test]
    fn catalog_covers_openrouter_and_ollama() {
        let ids: Vec<_> = PROVIDER_CATALOG.iter().map(|p| p.id).collect();
        assert!(ids.contains(&"openrouter"));
        assert!(ids.contains(&"ollama_base_url"));
        assert!(ids.contains(&"llama_cpp_server_url"));
    }

    #[test]
    fn extract_env_var_names_basic() {
        assert_eq!(extract_env_var_names("OPENAI_API_KEY"), vec!["OPENAI_API_KEY"]);
        assert_eq!(
            extract_env_var_names("GOOGLE_API_KEY / GEMINI_API_KEY"),
            vec!["GOOGLE_API_KEY", "GEMINI_API_KEY"]
        );
        assert_eq!(
            extract_env_var_names("OLLAMA_HOST（例: http://127.0.0.1:11434）"),
            vec!["OLLAMA_HOST"]
        );
        assert_eq!(
            extract_env_var_names("HF_TOKEN / HUGGINGFACE_HUB_TOKEN"),
            vec!["HF_TOKEN", "HUGGINGFACE_HUB_TOKEN"]
        );
        assert!(extract_env_var_names("例: http://127.0.0.1:1234/v1").is_empty());
        assert!(extract_env_var_names("任意（リバースプロキシ認証用）").is_empty());
    }

    #[test]
    fn get_value_falls_back_to_env() {
        let p = ApiKeyPrefs::default();
        // entries は空なので env var がなければ空文字列
        assert_eq!(p.get_value("openai"), "");

        // entries に値があれば env var より優先
        let mut p2 = ApiKeyPrefs::default();
        p2.set_entry("openai", "sk-stored".into());
        assert_eq!(p2.get_value("openai"), "sk-stored");
        assert!(!p2.is_from_env("openai"));
    }
}
