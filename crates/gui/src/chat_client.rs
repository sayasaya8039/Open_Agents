//! Chat ページ用: クラウド API・Ollama HTTP・ネイティブ GGUF/ONNX
//!
//! API キーは `api_keys.json`（`ApiKeyPrefs`）から解決する。

use std::path::PathBuf;

use crate::api_key_prefs::ApiKeyPrefs;
use crate::model_prefs::{ChatInferenceSource, ChatPrefs};
use crate::native_chat;
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
    /// ネイティブ C コア（GGUF / ONNX ファイルパス）
    Native { path: PathBuf },
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
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("gguf") | Some("onnx") => Ok(ChatBackend::Native { path }),
        Some(other) => Err(format!(
            "Chat 用ローカルモデルは .gguf または .onnx です（現在: .{other}）"
        )),
        None => Err(
            "モデルファイルに拡張子がありません。GGUF または ONNX を選択してください。".into(),
        ),
    }
}

/// クラウド API のみ（OpenRouter → OpenAI → 汎用 OpenAI 互換）
fn resolve_api_backend(api: &ApiKeyPrefs, chat_model: &str) -> Result<ChatBackend, String> {
    let chat_model = chat_model.trim();
    let openrouter_key = api.get_str("openrouter");
    let openai_key = api.get_str("openai");
    let gen_base = api.get_str("generic_openai_base_url");
    let gen_key = api.get_str("generic_openai_api_key");

    if !openrouter_key.is_empty() {
        let model = trimmed_or(chat_model, "openai/gpt-4o-mini");
        return Ok(ChatBackend::OpenAiCompatible {
            base_url: OPENROUTER_BASE.to_string(),
            api_key: openrouter_key.to_string(),
            model,
        });
    }
    if !openai_key.is_empty() {
        let model = trimmed_or(chat_model, "gpt-4o-mini");
        return Ok(ChatBackend::OpenAiCompatible {
            base_url: OPENAI_BASE.to_string(),
            api_key: openai_key.to_string(),
            model,
        });
    }
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
        "Chat をクラウド API に設定していますが、OpenRouter / OpenAI / 汎用 OpenAI 互換のいずれも利用できません。API キー管理でキーを登録するか、設定で「Ollama」または「ローカル GGUF/ONNX」に切り替えてください。"
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
) -> Result<String, String> {
    match backend {
        ChatBackend::OpenAiCompatible {
            base_url,
            api_key,
            model,
        } => {
            let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
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
        ChatBackend::Native { path } => {
            native_chat::complete_native_chat_blocking(
                path,
                messages,
                temperature,
                max_tokens,
                context_length,
            )
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
    use std::path::PathBuf;

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
        let paths = vec![PathBuf::from(r"C:\models\test.gguf")];
        let b = resolve_chat_backend(&p, &chat, &paths).expect("backend");
        match b {
            ChatBackend::Native { path } => assert!(path.to_string_lossy().ends_with(".gguf")),
            _ => panic!("expected native"),
        }
    }
}
