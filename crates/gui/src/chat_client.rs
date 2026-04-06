//! Chat ページ用: クラウド API・Ollama HTTP・ネイティブ GGUF/ONNX
//!
//! API キーは `api_keys.json`（`ApiKeyPrefs`）から解決する。

use std::path::PathBuf;

use crate::api_key_prefs::ApiKeyPrefs;
use crate::llama_cpp_chat;
use crate::model_prefs::{ChatInferenceSource, ChatPrefs, HardwareParams};
use serde_json::{json, Value};

const OPENROUTER_BASE: &str = "https://openrouter.ai/api";
const OPENAI_BASE: &str = "https://api.openai.com/v1";

#[derive(Clone, Debug)]
pub enum ChatBackend {
    OpenAiCompatible {
        base_url: String,
        api_key: String,
        model: String,
    },
    Ollama {
        base_url: String,
        model: String,
    },
    /// llama.cpp サーバ経由でローカル GGUF を実行。
    /// GUI では raw `/completion` を使わず、常に OpenAI 互換 `/v1/chat/completions` を使う。
    LlamaCppLocal { path: PathBuf },
}

fn trimmed_or(s: &str, default: &str) -> String {
    let t = s.trim();
    if t.is_empty() {
        default.to_string()
    } else {
        t.to_string()
    }
}

/// 設定の Chat 推論先に従いバックエンドを決定する
pub fn resolve_chat_backend(
    api: &ApiKeyPrefs,
    chat: &ChatPrefs,
    local_model_paths: &[PathBuf],
) -> Result<ChatBackend, String> {
    match chat.source {
        ChatInferenceSource::Local => resolve_ollama_backend(api, &chat.ollama_model),
        ChatInferenceSource::Api => resolve_api_backend(api, &chat.api_model),
        ChatInferenceSource::LocalWeights => {
            resolve_local_weights(local_model_paths, chat.local_model_index)
        }
    }
}

fn resolve_local_weights(paths: &[PathBuf], index: usize) -> Result<ChatBackend, String> {
    if paths.is_empty() {
        return Err(
            "Chat をローカル GGUF/ONNX に設定していますが、設定の「ローカルLLM」にモデルファイルが未追加です。"
                .into(),
        );
    }
    let idx = index.min(paths.len() - 1);
    let path = paths[idx].clone();
    if !path.is_file() {
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("モデルファイル");
        return Err(format!(
            "選択中のローカルモデル `{file_name}` が見つかりません。設定の「ローカルLLM」でこの登録を削除して再追加してください。保存パス: {}",
            path.display()
        ));
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("gguf") => Ok(ChatBackend::LlamaCppLocal { path }),
        Some("onnx") => Err(
            "Chat のローカル ONNX は現状未対応です。GGUF を選択するか、設定で「Ollama」またはクラウド API に切り替えてください。"
                .into(),
        ),
        Some(other) => Err(format!(
            "Chat 用ローカルモデルは .gguf または .onnx です（現在: .{other}）"
        )),
        None => Err(
            "モデルファイルに拡張子がありません。GGUF または ONNX を選択してください。".into(),
        ),
    }
}

/// クラウド API のみ（OpenRouter → OpenAI → 汎用 OpenAI 互換）
/// プロバイダ ID → (ベース URL, デフォルトモデル)
const PROVIDER_ENDPOINTS: &[(&str, &str, &str)] = &[
    ("openrouter",  "https://openrouter.ai/api",        "openai/gpt-4o-mini"),
    ("openai",      "https://api.openai.com/v1",         "gpt-4o-mini"),
    ("anthropic",   "https://api.anthropic.com/v1",      "claude-sonnet-4-20250514"),
    ("google_gemini", "https://generativelanguage.googleapis.com/v1beta/openai", "gemini-2.5-flash"),
    ("groq",        "https://api.groq.com/openai/v1",    "llama-3.3-70b-versatile"),
    ("mistral",     "https://api.mistral.ai/v1",         "mistral-large-latest"),
    ("deepseek",    "https://api.deepseek.com",          "deepseek-chat"),
    ("xai",         "https://api.x.ai/v1",               "grok-3-mini"),
    ("cohere",      "https://api.cohere.com/v2",         "command-a-03-2025"),
    ("perplexity",  "https://api.perplexity.ai",         "sonar"),
    ("fireworks",   "https://api.fireworks.ai/inference/v1", "accounts/fireworks/models/llama4-scout-instruct-basic"),
    ("together",    "https://api.together.xyz/v1",       "meta-llama/Llama-4-Scout-17B-16E-Instruct"),
    ("moonshot",    "https://api.moonshot.cn/v1",        "moonshot-v1-128k"),
    ("siliconflow", "https://api.siliconflow.cn/v1",     "deepseek-ai/DeepSeek-R1"),
    ("novita",      "https://api.novita.ai/v3/openai",   "deepseek/deepseek-r1"),
    ("nebius",      "https://api.studio.nebius.ai/v1",   "deepseek-r1"),
];

fn resolve_api_backend(api: &ApiKeyPrefs, chat_model: &str) -> Result<ChatBackend, String> {
    let chat_model = chat_model.trim();

    // 登録済みプロバイダを優先順に検索
    for &(id, base_url, default_model) in PROVIDER_ENDPOINTS {
        let key = api.get_str(id);
        if !key.is_empty() {
            let model = trimmed_or(chat_model, default_model);
            return Ok(ChatBackend::OpenAiCompatible {
                base_url: base_url.to_string(),
                api_key: key.to_string(),
                model,
            });
        }
    }

    // 汎用 OpenAI 互換
    let gen_base = api.get_str("generic_openai_base_url");
    let gen_key = api.get_str("generic_openai_api_key");
    if !gen_base.is_empty() && !gen_key.is_empty() {
        let model = trimmed_or(chat_model, "gpt-4o-mini");
        let mut base = gen_base.trim_end_matches('/').to_string();
        if base.ends_with("/v1") {
            base.truncate(base.len() - "/v1".len());
        }
        return Ok(ChatBackend::OpenAiCompatible {
            base_url: base,
            api_key: gen_key.to_string(),
            model,
        });
    }

    Err(
        "Chat をクラウド API に設定していますが、利用可能なプロバイダがありません。Settings の「API キー管理」でキーを登録してください。"
            .into(),
    )
}

fn resolve_ollama_backend(api: &ApiKeyPrefs, ollama_model: &str) -> Result<ChatBackend, String> {
    let ollama = api.get_str("ollama_base_url");
    if ollama.is_empty() {
        return Err(
            "Chat をローカル (Ollama) に設定していますが、Ollama ベース URL が未登録です。API キー管理で ollama_base_url を設定してください。"
                .into(),
        );
    }
    let model = trimmed_or(ollama_model, "llama3.2");
    Ok(ChatBackend::Ollama {
        base_url: ollama.trim_end_matches('/').to_string(),
        model,
    })
}

fn messages_to_openai_json(messages: &[(String, String)]) -> Vec<Value> {
    messages
        .iter()
        .map(|(role, content)| {
            json!({
                "role": role,
                "content": content,
            })
        })
        .collect()
}

/// 同期 HTTP（`smol::unblock` 内から呼ぶ想定）
pub fn complete_chat_blocking(
    backend: &ChatBackend,
    messages: &[(String, String)],
    temperature: f32,
    max_tokens: i32,
    context_length: i32,
    hardware: &HardwareParams,
) -> Result<String, String> {
    match backend {
        ChatBackend::OpenAiCompatible {
            base_url,
            api_key,
            model,
        } => {
            let base = base_url.trim_end_matches('/');
            let url = if base.ends_with("/v1") || base.ends_with("/v1beta/openai") || base.ends_with("/v2") {
                format!("{base}/chat/completions")
            } else {
                format!("{base}/v1/chat/completions")
            };
            let body = json!({
                "model": model,
                "messages": messages_to_openai_json(messages),
                "temperature": temperature,
                "max_tokens": max_tokens,
            });
            let mut req = ureq::post(&url)
                .set("Authorization", &format!("Bearer {}", api_key))
                .set("Content-Type", "application/json");
            if base_url.contains("openrouter.ai") {
                req = req
                    .set("HTTP-Referer", "https://github.com/sayasaya8039/Open_Agents")
                    .set("X-Title", "Open Agents");
            }
            let resp = req
                .send_json(body)
                .map_err(|e| format!("リクエスト失敗: {e}"))?;
            let status = resp.status();
            let text = resp
                .into_string()
                .map_err(|e| format!("本文読み取り: {e}"))?;
            if status >= 400 {
                return Err(format!("HTTP {status}: {text}"));
            }
            let v: Value =
                serde_json::from_str(&text).map_err(|e| format!("JSON 解析: {e} ({text})"))?;
            extract_openai_message_content(&v)
        }
        ChatBackend::Ollama { base_url, model } => {
            let url = format!("{}/api/chat", base_url);
            let ollama_msgs: Vec<Value> = messages
                .iter()
                .map(|(role, content)| {
                    json!({
                        "role": role,
                        "content": content,
                    })
                })
                .collect();
            let body = json!({
                "model": model,
                "messages": ollama_msgs,
                "stream": false,
            });
            let resp = ureq::post(&url)
                .set("Content-Type", "application/json")
                .send_json(body)
                .map_err(|e| format!("Ollama リクエスト失敗: {e}"))?;
            let status = resp.status();
            let text = resp
                .into_string()
                .map_err(|e| format!("本文読み取り: {e}"))?;
            if status >= 400 {
                return Err(format!("Ollama HTTP {status}: {text}"));
            }
            let v: Value =
                serde_json::from_str(&text).map_err(|e| format!("JSON 解析: {e}"))?;
            v.pointer("/message/content")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| format!("想定外の Ollama 応答: {text}"))
        }
        ChatBackend::LlamaCppLocal { path } => {
            llama_cpp_chat::complete_llama_cpp_chat_blocking(
                path,
                messages,
                temperature,
                max_tokens,
                context_length,
                hardware,
            )
            .map(|response| response.content)
        }
    }
}

fn extract_openai_message_content(v: &Value) -> Result<String, String> {
    v.pointer("/choices/0/message/content")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            // 一部プロバイダはトップレベル text
            v.get("content").and_then(|x| x.as_str()).map(|s| s.to_string())
        })
        .ok_or_else(|| {
            format!(
                "choices[0].message.content がありません: {}",
                serde_json::to_string(v).unwrap_or_default()
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_model_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("open_agents_{name}_{nonce}.gguf"))
    }

    #[test]
    fn resolve_prefers_openrouter() {
        let mut p = ApiKeyPrefs::default();
        p.entries = BTreeMap::from([
            ("openrouter".into(), "rk".into()),
            ("openai".into(), "sk".into()),
        ]);
        let chat = ChatPrefs::default();
        let b = resolve_chat_backend(&p, &chat, &[]).expect("backend");
        match b {
            ChatBackend::OpenAiCompatible { base_url, model, .. } => {
                assert!(base_url.contains("openrouter"));
                assert_eq!(model, "openai/gpt-4o-mini");
            }
            _ => panic!("expected openrouter"),
        }
    }

    #[test]
    fn ollama_when_local_source_and_base() {
        let mut p = ApiKeyPrefs::default();
        p.set_entry("ollama_base_url", "http://127.0.0.1:11434".into());
        let chat = ChatPrefs {
            source: ChatInferenceSource::Local,
            ..Default::default()
        };
        let b = resolve_chat_backend(&p, &chat, &[]).expect("backend");
        match b {
            ChatBackend::Ollama { model, .. } => assert_eq!(model, "llama3.2"),
            _ => panic!("expected ollama"),
        }
    }

    #[test]
    fn api_source_fails_without_cloud_keys() {
        let mut p = ApiKeyPrefs::default();
        p.set_entry("ollama_base_url", "http://127.0.0.1:11434".into());
        let chat = ChatPrefs {
            source: ChatInferenceSource::Api,
            ..Default::default()
        };
        assert!(resolve_chat_backend(&p, &chat, &[]).is_err());
    }

    #[test]
    fn local_weights_requires_registered_file() {
        let p = ApiKeyPrefs::default();
        let chat = ChatPrefs {
            source: ChatInferenceSource::LocalWeights,
            ..Default::default()
        };
        assert!(resolve_chat_backend(&p, &chat, &[]).is_err());
    }

    #[test]
    fn local_weights_accepts_gguf_path() {
        let p = ApiKeyPrefs::default();
        let chat = ChatPrefs {
            source: ChatInferenceSource::LocalWeights,
            ..Default::default()
        };
        let path = temp_model_path("existing");
        fs::write(&path, b"gguf").unwrap();
        let paths = vec![path.clone()];
        let b = resolve_chat_backend(&p, &chat, &paths).expect("backend");
        match b {
            ChatBackend::LlamaCppLocal { path } => {
                assert!(path.to_string_lossy().ends_with(".gguf"))
            }
            _ => panic!("expected llama.cpp local backend"),
        }
        let _ = fs::remove_file(path);
    }

    #[test]
    fn local_weights_rejects_onnx_for_chat() {
        let p = ApiKeyPrefs::default();
        let chat = ChatPrefs {
            source: ChatInferenceSource::LocalWeights,
            ..Default::default()
        };
        let path = std::env::temp_dir().join("open_agents_chat_local_model.onnx");
        fs::write(&path, b"onnx").unwrap();
        let err = resolve_chat_backend(&p, &chat, &[path.clone()]).unwrap_err();
        assert!(err.contains("ONNX"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn local_weights_rejects_missing_registered_file() {
        let p = ApiKeyPrefs::default();
        let chat = ChatPrefs {
            source: ChatInferenceSource::LocalWeights,
            ..Default::default()
        };
        let err = resolve_chat_backend(&p, &chat, &[temp_model_path("missing")]).unwrap_err();
        assert!(err.contains("見つかりません"));
        assert!(err.contains("ローカルLLM"));
    }
}
