//! Chat ページ用: クラウド API・Ollama HTTP・ネイティブ GGUF/ONNX
//!
//! API キーは `api_keys.json`（`ApiKeyPrefs`）から解決する。

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::api_key_prefs::ApiKeyPrefs;
use crate::chat_session::ChatMsgMetrics;
use crate::llama_cpp_chat;
use crate::model_prefs::{ChatInferenceSource, ChatPrefs, HardwareParams};
use serde_json::{json, Value};

fn is_xai_multi_agent_model(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    model.contains("multi-agent") || model.contains("multiagent")
}

fn supports_chat_completions(provider_id: &str, model: &str) -> bool {
    !(provider_id == "xai" && is_xai_multi_agent_model(model))
}

fn unsupported_chat_completions_message(provider_id: &str, model: &str) -> String {
    match provider_id {
        "xai" if is_xai_multi_agent_model(model) => format!(
            "選択中の xAI モデル `{model}` は Chat Completions API 非対応です。xAI docs では Responses API を使う必要があります。現バージョンの Open_Agents クラウド API チャットは Chat Completions のみ対応なので、`grok-4.20-0309-reasoning` か `grok-4.20-0309-non-reasoning` などの通常モデルへ切り替えてください。"
        ),
        _ => format!(
            "選択中のモデル `{model}` は現在の Chat Completions 実装では利用できません。"
        ),
    }
}

/// プロバイダの /v1/models エンドポイントからモデル一覧を取得（同期・重い処理）
/// (プロバイダID, 表示ラベル, モデルID一覧)
pub fn fetch_provider_models(api: &ApiKeyPrefs) -> Vec<(String, String, Vec<String>)> {
    let mut results = Vec::new();

    for &(id, base_url, _default) in PROVIDER_ENDPOINTS {
        let key = api.get_value(id);
        if key.is_empty() {
            continue;
        }
        let base = base_url.trim_end_matches('/');
        let url =
            if base.ends_with("/v1") || base.ends_with("/v1beta/openai") || base.ends_with("/v2") {
                format!("{base}/models")
            } else {
                format!("{base}/v1/models")
            };

        match fetch_models_from_url(&url, &key) {
            Ok(models) => {
                let models: Vec<String> = models
                    .into_iter()
                    .filter(|model| supports_chat_completions(id, model))
                    .collect();
                if models.is_empty() {
                    eprintln!("models: {id} — Chat Completions で使えるモデルがありません");
                    continue;
                }
                let label = match id {
                    "openrouter" => "OpenRouter",
                    "openai" => "OpenAI",
                    "anthropic" => "Anthropic",
                    "google_gemini" => "Google Gemini",
                    "groq" => "Groq",
                    "mistral" => "Mistral AI",
                    "deepseek" => "DeepSeek",
                    "xai" => "xAI (Grok)",
                    "cohere" => "Cohere",
                    "perplexity" => "Perplexity",
                    "fireworks" => "Fireworks AI",
                    "together" => "Together AI",
                    "moonshot" => "Moonshot",
                    "siliconflow" => "SiliconFlow",
                    "novita" => "Novita AI",
                    "nebius" => "Nebius",
                    other => other,
                };
                results.push((id.to_string(), label.to_string(), models));
            }
            Err(e) => eprintln!("models: {id} — 取得失敗: {e}"),
        }
    }
    results
}

fn fetch_models_from_url(url: &str, api_key: &str) -> Result<Vec<String>, String> {
    let resp = ureq::get(url)
        .set("Authorization", &format!("Bearer {api_key}"))
        .set("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .map_err(|e| format!("リクエスト失敗: {e}"))?;
    let status = resp.status();
    let text = resp
        .into_string()
        .map_err(|e| format!("読み取り失敗: {e}"))?;
    if status >= 400 {
        return Err(format!("HTTP {status}"));
    }
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("JSON解析失敗: {e}"))?;

    let mut models: Vec<String> = v
        .pointer("/data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    item.get("id")
                        .and_then(|id| id.as_str())
                        .map(|s| s.to_string())
                })
                .collect()
        })
        .unwrap_or_default();
    models.sort();
    // 最大50件に制限（OpenRouter等は数千モデルあるため）
    models.truncate(30);
    Ok(models)
}

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
    LlamaCppLocal {
        path: PathBuf,
    },
}

impl ChatBackend {
    /// API サーバーのプロキシ転送先を `(base_url, api_key)` で返す。
    /// Ollama はネイティブ `/api/chat` なので URL をそのまま返す。
    /// LlamaCppLocal は llama-server の OpenAI 互換ポートを返す。
    pub fn proxy_target(&self) -> (String, String) {
        match self {
            Self::OpenAiCompatible {
                base_url, api_key, ..
            } => (base_url.clone(), api_key.clone()),
            Self::Ollama { base_url, .. } => (base_url.clone(), String::new()),
            Self::LlamaCppLocal { .. } => {
                // llama-server は 8080 で起動する（llama_cpp_chat 参照）
                ("http://127.0.0.1:8080/v1".to_string(), String::new())
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct ChatCompletionResult {
    pub content: String,
    pub metrics: Option<ChatMsgMetrics>,
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
        ChatInferenceSource::Api => resolve_api_backend(api, chat),
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
pub const PROVIDER_ENDPOINTS: &[(&str, &str, &str)] = &[
    (
        "openrouter",
        "https://openrouter.ai/api",
        "openai/gpt-4o-mini",
    ),
    ("openai", "https://api.openai.com/v1", "gpt-4o-mini"),
    (
        "anthropic",
        "https://api.anthropic.com/v1",
        "claude-sonnet-4-20250514",
    ),
    (
        "google_gemini",
        "https://generativelanguage.googleapis.com/v1beta/openai",
        "gemini-2.5-flash",
    ),
    (
        "groq",
        "https://api.groq.com/openai/v1",
        "llama-3.3-70b-versatile",
    ),
    (
        "mistral",
        "https://api.mistral.ai/v1",
        "mistral-large-latest",
    ),
    ("deepseek", "https://api.deepseek.com", "deepseek-chat"),
    ("xai", "https://api.x.ai/v1", "grok-3-mini"),
    ("cohere", "https://api.cohere.com/v2", "command-a-03-2025"),
    ("perplexity", "https://api.perplexity.ai", "sonar"),
    (
        "fireworks",
        "https://api.fireworks.ai/inference/v1",
        "accounts/fireworks/models/llama4-scout-instruct-basic",
    ),
    (
        "together",
        "https://api.together.xyz/v1",
        "meta-llama/Llama-4-Scout-17B-16E-Instruct",
    ),
    ("moonshot", "https://api.moonshot.cn/v1", "moonshot-v1-128k"),
    (
        "siliconflow",
        "https://api.siliconflow.cn/v1",
        "deepseek-ai/DeepSeek-R1",
    ),
    (
        "novita",
        "https://api.novita.ai/v3/openai",
        "deepseek/deepseek-r1",
    ),
    ("nebius", "https://api.studio.nebius.ai/v1", "deepseek-r1"),
];

/// モデルIDからプロバイダを推定（grok-* → xai, deepseek-* → deepseek 等）
fn guess_provider_from_model(model: &str) -> Option<&'static str> {
    let m = model.to_lowercase();
    if m.starts_with("gpt-")
        || m.starts_with("o3")
        || m.starts_with("o4")
        || m.starts_with("dall-e")
        || m.starts_with("text-")
        || m.starts_with("tts-")
        || m.starts_with("whisper")
    {
        return Some("openai");
    }
    if m.starts_with("claude-") {
        return Some("anthropic");
    }
    if m.starts_with("gemini-") {
        return Some("google_gemini");
    }
    if m.starts_with("grok-") {
        return Some("xai");
    }
    if m.starts_with("deepseek-") || m == "deepseek-chat" || m == "deepseek-reasoner" {
        return Some("deepseek");
    }
    if m.starts_with("mistral-") || m.starts_with("codestral") || m.starts_with("open-mistral") {
        return Some("mistral");
    }
    if m.starts_with("llama-") && (m.contains("instant") || m.contains("versatile")) {
        return Some("groq");
    }
    if m.starts_with("command-") {
        return Some("cohere");
    }
    if m.starts_with("sonar") {
        return Some("perplexity");
    }
    if m.starts_with("moonshot-") {
        return Some("moonshot");
    }
    // OpenRouter uses provider/model format
    if m.contains('/') {
        return Some("openrouter");
    }
    None
}

fn resolve_api_backend(api: &ApiKeyPrefs, chat: &ChatPrefs) -> Result<ChatBackend, String> {
    let chat_model = chat.api_model.trim();
    let mut provider_id = chat.api_provider.trim().to_string();

    // provider が空ならモデル名から推定
    if provider_id.is_empty() && !chat_model.is_empty() {
        if let Some(guessed) = guess_provider_from_model(chat_model) {
            provider_id = guessed.to_string();
            eprintln!("chat_client: api_provider 未設定 — モデル「{chat_model}」からプロバイダ「{guessed}」を推定");
        }
    }

    // 1. 明示的または推定されたプロバイダを使う
    if !provider_id.is_empty() {
        if let Some(&(_, base_url, default_model)) = PROVIDER_ENDPOINTS
            .iter()
            .find(|(id, _, _)| *id == provider_id)
        {
            let key = api.get_value(&provider_id);
            if !key.is_empty() {
                let model = trimmed_or(chat_model, default_model);
                if !supports_chat_completions(&provider_id, &model) {
                    return Err(unsupported_chat_completions_message(&provider_id, &model));
                }
                return Ok(ChatBackend::OpenAiCompatible {
                    base_url: base_url.to_string(),
                    api_key: key,
                    model,
                });
            }
        }
    }

    // 2. フォールバック: 登録済みプロバイダを優先順に検索
    for &(id, base_url, default_model) in PROVIDER_ENDPOINTS {
        let key = api.get_value(id);
        if !key.is_empty() {
            let model = trimmed_or(chat_model, default_model);
            if !supports_chat_completions(id, &model) {
                return Err(unsupported_chat_completions_message(id, &model));
            }
            return Ok(ChatBackend::OpenAiCompatible {
                base_url: base_url.to_string(),
                api_key: key,
                model,
            });
        }
    }

    // 3. ローカル / セルフホスト OpenAI 互換サーバ（LM Studio, vLLM, llama.cpp server）
    //    これらは API キー不要でアクセスできるため、URL だけで利用可能
    for &(id, default_model) in &[
        ("lm_studio_base_url", "default"),
        ("vllm_base_url", "default"),
        ("llama_cpp_server_url", "default"),
    ] {
        let base_url = api.get_value(id);
        if !base_url.is_empty() {
            let model = trimmed_or(chat_model, default_model);
            let base = normalize_local_server_url(&base_url);
            return Ok(ChatBackend::OpenAiCompatible {
                base_url: base,
                api_key: "no-key".to_string(), // ローカルサーバはキー不要
                model,
            });
        }
    }

    // 4. 汎用 OpenAI 互換
    let gen_base = api.get_value("generic_openai_base_url");
    let gen_key = api.get_value("generic_openai_api_key");
    if !gen_base.is_empty() && !gen_key.is_empty() {
        let model = trimmed_or(chat_model, "gpt-4o-mini");
        let mut base = gen_base.trim_end_matches('/').to_string();
        if base.ends_with("/v1") {
            base.truncate(base.len() - "/v1".len());
        }
        return Ok(ChatBackend::OpenAiCompatible {
            base_url: base,
            api_key: gen_key,
            model,
        });
    }

    Err(
        "Chat をクラウド API に設定していますが、利用可能なプロバイダがありません。Settings の「API キー管理」でキーを登録するか、ローカル / セルフホストの URL を設定してください。"
            .into(),
    )
}

fn resolve_ollama_backend(api: &ApiKeyPrefs, ollama_model: &str) -> Result<ChatBackend, String> {
    let ollama = api.get_value("ollama_base_url");
    if !ollama.is_empty() {
        let model = trimmed_or(ollama_model, "llama3.2");
        return Ok(ChatBackend::Ollama {
            base_url: ollama.trim_end_matches('/').to_string(),
            model,
        });
    }

    // Ollama URL が未設定の場合、LM Studio / vLLM / llama.cpp server を OpenAI 互換として使用
    for &id in &["lm_studio_base_url", "vllm_base_url", "llama_cpp_server_url"] {
        let url = api.get_value(id);
        if !url.is_empty() {
            let model = trimmed_or(ollama_model, "default");
            let base = normalize_local_server_url(&url);
            return Ok(ChatBackend::OpenAiCompatible {
                base_url: base,
                api_key: "no-key".to_string(),
                model,
            });
        }
    }

    Err(
        "Chat をローカル (Ollama) に設定していますが、Ollama ベース URL が未登録です。API キー管理で ollama_base_url または LM Studio / vLLM の URL を設定してください。"
            .into(),
    )
}

/// ローカルサーバ URL を正規化。
/// ユーザーが以下のどの形式で入力しても `http://host:port` に統一する:
///   - `http://localhost:1234`
///   - `http://localhost:1234/`
///   - `http://localhost:1234/v1`
///   - `http://localhost:1234/v1/`
///   - `http://localhost:1234/v1/chat/completions`
///   - `http://localhost:1234/api/v1/chat`
fn normalize_local_server_url(url: &str) -> String {
    let mut base = url.trim().trim_end_matches('/').to_string();
    // 末尾の OpenAI パス部分を除去（ユーザーが完全 URL を貼った場合）
    for suffix in &[
        "/chat/completions",
        "/completions",
        "/models",
        "/chat",
    ] {
        if base.ends_with(suffix) {
            base.truncate(base.len() - suffix.len());
        }
    }
    // /v1 や /api/v1 は残す — complete_chat_blocking が /v1 の有無を見て URL を構築する
    // ただし末尾の / は除去
    base.trim_end_matches('/').to_string()
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
) -> Result<ChatCompletionResult, String> {
    match backend {
        ChatBackend::OpenAiCompatible {
            base_url,
            api_key,
            model,
        } => {
            let started = Instant::now();
            let base = base_url.trim_end_matches('/');
            let url = if base.ends_with("/v1")
                || base.ends_with("/v1beta/openai")
                || base.ends_with("/v2")
            {
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
                .set("Content-Type", "application/json")
                .timeout(Duration::from_secs(60));
            if base_url.contains("openrouter.ai") {
                req = req
                    .set(
                        "HTTP-Referer",
                        "https://github.com/sayasaya8039/Open_Agents",
                    )
                    .set("X-Title", "Open Agents");
            }
            let resp = match req.send_json(body) {
                Ok(r) => r,
                Err(ureq::Error::Status(code, response)) => {
                    let body = response.into_string().unwrap_or_default();
                    return Err(format!("HTTP {code}: {body}"));
                }
                Err(e) => return Err(format!("リクエスト失敗: {e}")),
            };
            let status = resp.status();
            let text = resp
                .into_string()
                .map_err(|e| format!("本文読み取り: {e}"))?;
            if status >= 400 {
                return Err(format!("HTTP {status}: {text}"));
            }
            let v: Value =
                serde_json::from_str(&text).map_err(|e| format!("JSON 解析: {e} ({text})"))?;
            extract_openai_chat_result(&v, model, started.elapsed())
        }
        ChatBackend::Ollama { base_url, model } => {
            let started = Instant::now();
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
                .timeout(Duration::from_secs(60))
                .send_json(body)
                .map_err(|e| format!("Ollama リクエスト失敗: {e}"))?;
            let status = resp.status();
            let text = resp
                .into_string()
                .map_err(|e| format!("本文読み取り: {e}"))?;
            if status >= 400 {
                return Err(format!("Ollama HTTP {status}: {text}"));
            }
            let v: Value = serde_json::from_str(&text).map_err(|e| format!("JSON 解析: {e}"))?;
            extract_ollama_chat_result(&v, model, started.elapsed())
                .map_err(|err| format!("想定外の Ollama 応答: {err}; {text}"))
        }
        ChatBackend::LlamaCppLocal { path } => llama_cpp_chat::complete_llama_cpp_chat_blocking(
            path,
            messages,
            temperature,
            max_tokens,
            context_length,
            hardware,
        )
        .map(|response| ChatCompletionResult {
            content: response.content,
            metrics: response.metrics,
        }),
    }
}

fn extract_openai_message_content(v: &Value) -> Result<String, String> {
    v.pointer("/choices/0/message/content")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            // 一部プロバイダはトップレベル text
            v.get("content")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
        })
        .ok_or_else(|| {
            format!(
                "choices[0].message.content がありません: {}",
                serde_json::to_string(v).unwrap_or_default()
            )
        })
}

fn extract_openai_chat_result(
    v: &Value,
    model: &str,
    elapsed: Duration,
) -> Result<ChatCompletionResult, String> {
    let content = extract_openai_message_content(v)?;
    let elapsed_ms = elapsed.as_millis().min(u128::from(u64::MAX)) as u64;
    let prompt_tokens = value_to_u32(v.pointer("/usage/prompt_tokens"));
    let completion_tokens = value_to_u32(v.pointer("/usage/completion_tokens"));
    let total_tokens = value_to_u32(v.pointer("/usage/total_tokens"));
    let stop_reason = v
        .pointer("/choices/0/finish_reason")
        .and_then(|value| value.as_str())
        .map(normalize_stop_reason);
    let tokens_per_second = tokens_per_second(completion_tokens, elapsed_ms);

    Ok(ChatCompletionResult {
        content,
        metrics: Some(ChatMsgMetrics {
            model_label: Some(model.to_string()),
            prompt_tokens,
            completion_tokens,
            total_tokens,
            tokens_per_second,
            elapsed_ms: Some(elapsed_ms),
            stop_reason,
        }),
    })
}

fn extract_ollama_chat_result(
    v: &Value,
    model: &str,
    elapsed: Duration,
) -> Result<ChatCompletionResult, String> {
    let content = v
        .pointer("/message/content")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "missing message.content".to_string())?;
    let prompt_tokens = value_to_u32(v.pointer("/prompt_eval_count"));
    let completion_tokens = value_to_u32(v.pointer("/eval_count"));
    let total_tokens = match (prompt_tokens, completion_tokens) {
        (Some(prompt), Some(completion)) => prompt.checked_add(completion),
        _ => None,
    };
    let elapsed_ms = duration_ns_to_ms(v.pointer("/total_duration"))
        .unwrap_or_else(|| elapsed.as_millis().min(u128::from(u64::MAX)) as u64);
    let eval_ms = duration_ns_to_ms(v.pointer("/eval_duration"));
    let tokens_per_second = match (completion_tokens, eval_ms) {
        (Some(tokens), Some(ms)) if ms > 0 => Some(tokens as f64 / (ms as f64 / 1000.0)),
        _ => tokens_per_second(completion_tokens, elapsed_ms),
    };
    let stop_reason = v
        .pointer("/done_reason")
        .and_then(|value| value.as_str())
        .map(normalize_stop_reason);

    Ok(ChatCompletionResult {
        content,
        metrics: Some(ChatMsgMetrics {
            model_label: Some(model.to_string()),
            prompt_tokens,
            completion_tokens,
            total_tokens,
            tokens_per_second,
            elapsed_ms: Some(elapsed_ms),
            stop_reason,
        }),
    })
}

fn value_to_u32(value: Option<&Value>) -> Option<u32> {
    value
        .and_then(|value| value.as_u64())
        .and_then(|value| u32::try_from(value).ok())
}

fn duration_ns_to_ms(value: Option<&Value>) -> Option<u64> {
    value
        .and_then(|value| value.as_u64())
        .map(|value| value / 1_000_000)
}

fn tokens_per_second(tokens: Option<u32>, elapsed_ms: u64) -> Option<f64> {
    match (tokens, elapsed_ms) {
        (Some(tokens), elapsed_ms) if elapsed_ms > 0 => {
            Some(tokens as f64 / (elapsed_ms as f64 / 1000.0))
        }
        _ => None,
    }
}

fn normalize_stop_reason(reason: &str) -> String {
    match reason.trim().to_ascii_lowercase().as_str() {
        "stop" => "EOSトークン検出".to_string(),
        "length" => "最大トークン到達".to_string(),
        "tool_calls" => "ツール呼び出し".to_string(),
        "content_filter" => "コンテンツフィルタ".to_string(),
        other if other.is_empty() => "完了".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::time::Duration;
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
            ChatBackend::OpenAiCompatible {
                base_url, model, ..
            } => {
                assert!(base_url.contains("openrouter"));
                assert_eq!(model, "openai/gpt-4o-mini");
            }
            _ => panic!("expected openrouter"),
        }
    }

    #[test]
    fn xai_multi_agent_model_is_rejected_before_request() {
        let mut p = ApiKeyPrefs::default();
        p.set_entry("xai", "xai-key".into());
        let chat = ChatPrefs {
            source: ChatInferenceSource::Api,
            api_model: "grok-4.20-multi-agent-0309".into(),
            api_provider: "xai".into(),
            ..Default::default()
        };

        let err = resolve_chat_backend(&p, &chat, &[]).unwrap_err();
        assert!(err.contains("Chat Completions API 非対応"));
        assert!(err.contains("Responses API"));
    }

    #[test]
    fn xai_reasoning_model_is_allowed() {
        let mut p = ApiKeyPrefs::default();
        p.set_entry("xai", "xai-key".into());
        let chat = ChatPrefs {
            source: ChatInferenceSource::Api,
            api_model: "grok-4.20-0309-reasoning".into(),
            api_provider: "xai".into(),
            ..Default::default()
        };

        let backend = resolve_chat_backend(&p, &chat, &[]).expect("backend");
        match backend {
            ChatBackend::OpenAiCompatible {
                base_url, model, ..
            } => {
                assert_eq!(base_url, "https://api.x.ai/v1");
                assert_eq!(model, "grok-4.20-0309-reasoning");
            }
            _ => panic!("expected xAI backend"),
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
    fn openai_response_extracts_metrics_and_stop_reason() {
        let value: Value = serde_json::from_str(
            r#"{
                "choices":[{"message":{"content":"こんにちは"},"finish_reason":"stop"}],
                "usage":{"prompt_tokens":12,"completion_tokens":66,"total_tokens":78}
            }"#,
        )
        .unwrap();

        let result = extract_openai_chat_result(
            &value,
            "grok-4.20-0309-reasoning",
            Duration::from_millis(430),
        )
        .expect("result");

        assert_eq!(result.content, "こんにちは");
        let metrics = result.metrics.expect("metrics");
        assert_eq!(
            metrics.model_label.as_deref(),
            Some("grok-4.20-0309-reasoning")
        );
        assert_eq!(metrics.prompt_tokens, Some(12));
        assert_eq!(metrics.completion_tokens, Some(66));
        assert_eq!(metrics.total_tokens, Some(78));
        assert_eq!(metrics.stop_reason.as_deref(), Some("EOSトークン検出"));
        assert_eq!(metrics.elapsed_ms, Some(430));
        assert!(metrics.tokens_per_second.unwrap_or_default() > 100.0);
    }

    #[test]
    fn ollama_response_extracts_eval_metrics() {
        let value: Value = serde_json::from_str(
            r#"{
                "message":{"content":"了解です"},
                "prompt_eval_count":42,
                "eval_count":66,
                "total_duration":430000000,
                "eval_duration":300000000,
                "done_reason":"stop"
            }"#,
        )
        .unwrap();

        let result =
            extract_ollama_chat_result(&value, "gemma-4-e4b-it", Duration::from_millis(430))
                .expect("result");

        assert_eq!(result.content, "了解です");
        let metrics = result.metrics.expect("metrics");
        assert_eq!(metrics.model_label.as_deref(), Some("gemma-4-e4b-it"));
        assert_eq!(metrics.prompt_tokens, Some(42));
        assert_eq!(metrics.completion_tokens, Some(66));
        assert_eq!(metrics.stop_reason.as_deref(), Some("EOSトークン検出"));
        assert_eq!(metrics.elapsed_ms, Some(430));
        assert!(metrics.tokens_per_second.unwrap_or_default() > 200.0);
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

    #[test]
    fn normalize_local_server_url_strips_paths() {
        assert_eq!(
            normalize_local_server_url("http://localhost:1234/v1"),
            "http://localhost:1234/v1"
        );
        assert_eq!(
            normalize_local_server_url("http://localhost:1234/v1/"),
            "http://localhost:1234/v1"
        );
        assert_eq!(
            normalize_local_server_url("http://localhost:1234/v1/chat/completions"),
            "http://localhost:1234/v1"
        );
        assert_eq!(
            normalize_local_server_url("http://localhost:1234/api/v1/chat"),
            "http://localhost:1234/api/v1"
        );
        assert_eq!(
            normalize_local_server_url("http://localhost:1234"),
            "http://localhost:1234"
        );
        assert_eq!(
            normalize_local_server_url("http://localhost:1234/"),
            "http://localhost:1234"
        );
    }

    #[test]
    fn lm_studio_url_resolves_as_openai_compatible() {
        let mut p = ApiKeyPrefs::default();
        p.set_entry("lm_studio_base_url", "http://127.0.0.1:1234/v1".into());
        let chat = ChatPrefs {
            source: ChatInferenceSource::Api,
            ..Default::default()
        };
        let b = resolve_chat_backend(&p, &chat, &[]).expect("backend");
        match b {
            ChatBackend::OpenAiCompatible { base_url, .. } => {
                // /v1 は残る — complete_chat_blocking が /chat/completions を付ける
                assert!(
                    base_url.contains("127.0.0.1:1234"),
                    "unexpected url: {base_url}"
                );
            }
            _ => panic!("expected OpenAiCompatible backend for LM Studio"),
        }
    }
}
