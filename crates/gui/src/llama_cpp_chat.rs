//! ローカル GGUF を `llama-server` 経由で叩く最小ランタイム。

use std::io::{BufRead, BufReader, Read};
use std::net::TcpListener;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::{llama_cpp_runtime, model_prefs};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(90);
const POLL_INTERVAL: Duration = Duration::from_millis(250);
const MAX_LOG_BYTES: usize = 32 * 1024;
const DEFAULT_CONTEXT_LENGTH: i32 = 4096;
const MIN_CONTEXT_LENGTH: i32 = 512;
const CHAT_COMPLETIONS_PATH: &str = "/v1/chat/completions";

#[cfg(windows)]
use std::os::windows::process::CommandExt;

struct CachedServer {
    model_path: PathBuf,
    context_length: i32,
    base_url: String,
    model_id: String,
    child: Child,
}

pub struct LlamaCppChatResponse {
    pub content: String,
    pub thinking: Option<String>,
}

fn server_cache() -> &'static Mutex<Option<CachedServer>> {
    static CACHE: OnceLock<Mutex<Option<CachedServer>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

pub fn complete_llama_cpp_chat_blocking(
    model_path: &Path,
    messages: &[(String, String)],
    temperature: f32,
    max_tokens: i32,
    context_length: i32,
) -> Result<LlamaCppChatResponse, String> {
    let (base_url, model_id) = ensure_server(model_path, context_length)?;
    let max_tokens = normalize_max_tokens(max_tokens);
    let url = chat_completions_url(&base_url);
    log_chat_template_mode(model_path, false, max_tokens);
    let body = json!({
        "model": model_id,
        "messages": messages_to_openai_json(messages),
        "temperature": temperature,
        "max_tokens": max_tokens,
        "stream": false,
    });
    let resp = ureq::post(&url)
        .set("Content-Type", "application/json")
        .send_json(body)
        .map_err(|e| format!("llama.cpp リクエスト失敗: {e}"))?;
    let status = resp.status();
    let text = resp
        .into_string()
        .map_err(|e| format!("llama.cpp 応答本文の読み取りに失敗しました: {e}"))?;
    if status >= 400 {
        return Err(format!("llama.cpp HTTP {status}: {text}"));
    }
    extract_openai_message(&text)
}

pub fn stream_llama_cpp_chat_blocking<F, G>(
    model_path: &Path,
    messages: &[(String, String)],
    temperature: f32,
    max_tokens: i32,
    context_length: i32,
    mut on_content_delta: F,
    mut on_thinking_delta: G,
) -> Result<LlamaCppChatResponse, String>
where
    F: FnMut(&str),
    G: FnMut(&str),
{
    let (base_url, model_id) = ensure_server(model_path, context_length)?;
    let max_tokens = normalize_max_tokens(max_tokens);
    let url = chat_completions_url(&base_url);
    log_chat_template_mode(model_path, true, max_tokens);
    let body = json!({
        "model": model_id,
        "messages": messages_to_openai_json(messages),
        "temperature": temperature,
        "max_tokens": max_tokens,
        "stream": true,
    });
    let resp = ureq::post(&url)
        .set("Content-Type", "application/json")
        .set("Accept", "text/event-stream")
        .send_json(body)
        .map_err(|e| format!("llama.cpp ストリーミングリクエスト失敗: {e}"))?;
    let status = resp.status();
    if status >= 400 {
        let text = resp
            .into_string()
            .map_err(|e| format!("llama.cpp エラー本文の読み取りに失敗しました: {e}"))?;
        return Err(format!("llama.cpp HTTP {status}: {text}"));
    }
    let reader = resp.into_reader();
    read_streaming_response(reader, &mut on_content_delta, &mut on_thinking_delta)
}

pub fn server_ready_for(model_path: &Path, context_length: i32) -> bool {
    let normalized_ctx = normalize_context_length(context_length);
    let normalized_path = normalize_model_path(model_path);
    let Ok(mut cache) = server_cache().lock() else {
        return false;
    };
    let Some(server) = cache.as_mut() else {
        return false;
    };
    if server.model_path != normalized_path || server.context_length != normalized_ctx {
        return false;
    }
    server_is_alive(server).unwrap_or(false)
}

fn ensure_server(model_path: &Path, context_length: i32) -> Result<(String, String), String> {
    let normalized_ctx = normalize_context_length(context_length);
    let normalized_path = normalize_model_path(model_path);
    let mut cache = server_cache()
        .lock()
        .map_err(|_| "llama.cpp サーバキャッシュのロックに失敗しました".to_string())?;

    if let Some(server) = cache.as_mut() {
        if server.model_path == normalized_path && server.context_length == normalized_ctx {
            if server_is_alive(server)? {
                eprintln!(
                    "llama.cpp: warm server reused for {}",
                    normalized_path.display()
                );
                return Ok((server.base_url.clone(), server.model_id.clone()));
            }
            stop_server(server);
            *cache = None;
        } else {
            stop_server(server);
            *cache = None;
        }
    }

    let binary = find_llama_server_binary().ok_or_else(missing_bundled_runtime_message)?;
    let port = pick_free_port()?;
    let base_url = format!("http://127.0.0.1:{port}");
    let logs = Arc::new(Mutex::new(String::new()));
    let mut child = spawn_llama_server(
        &binary,
        &normalized_path,
        port,
        normalized_ctx,
        Arc::clone(&logs),
    )?;
    let model_id = wait_until_ready(&mut child, &base_url, &normalized_path, Arc::clone(&logs))?;
    eprintln!(
        "llama.cpp: started bundled server for {} on {}",
        normalized_path.display(),
        base_url
    );
    *cache = Some(CachedServer {
        model_path: normalized_path,
        context_length: normalized_ctx,
        base_url: base_url.clone(),
        model_id: model_id.clone(),
        child,
    });
    Ok((base_url, model_id))
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

fn extract_openai_message(text: &str) -> Result<LlamaCppChatResponse, String> {
    let v: Value = serde_json::from_str(text)
        .map_err(|e| format!("llama.cpp JSON 解析に失敗しました: {e} ({text})"))?;
    let content = v
        .pointer("/choices/0/message/content")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            v.get("content")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_default();
    let thinking = v
        .pointer("/choices/0/message/reasoning_content")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    if content.is_empty() && thinking.is_none() {
        return Err(format!(
            "choices[0].message.content / reasoning_content がありません: {text}"
        ));
    }
    Ok(LlamaCppChatResponse { content, thinking })
}

fn read_streaming_response<R, F, G>(
    reader: R,
    on_content_delta: &mut F,
    on_thinking_delta: &mut G,
) -> Result<LlamaCppChatResponse, String>
where
    R: Read,
    F: FnMut(&str),
    G: FnMut(&str),
{
    let mut out = String::new();
    let mut thinking = String::new();
    let mut saw_done = false;
    let mut saw_any_chunk = false;
    let mut buf = BufReader::new(reader);
    let mut line = String::new();

    loop {
        line.clear();
        let read = buf
            .read_line(&mut line)
            .map_err(|e| format!("llama.cpp ストリームの読み取りに失敗しました: {e}"))?;
        if read == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some(payload) = trimmed.strip_prefix("data:") else {
            continue;
        };
        let payload = payload.trim();
        if payload == "[DONE]" {
            saw_done = true;
            break;
        }
        let value: Value = serde_json::from_str(payload).map_err(|e| {
            format!("llama.cpp ストリーム JSON 解析に失敗しました: {e} ({payload})")
        })?;
        let chunk = extract_stream_chunk(&value);
        if let Some(delta) = chunk.content {
            saw_any_chunk = true;
            out.push_str(delta);
            on_content_delta(delta);
        }
        if let Some(delta) = chunk.thinking {
            saw_any_chunk = true;
            thinking.push_str(delta);
            on_thinking_delta(delta);
        }
    }

    if !saw_done && !saw_any_chunk {
        return Err("llama.cpp ストリームから本文を受け取れませんでした".to_string());
    }

    Ok(LlamaCppChatResponse {
        content: out,
        thinking: (!thinking.is_empty()).then_some(thinking),
    })
}

struct StreamChunk<'a> {
    content: Option<&'a str>,
    thinking: Option<&'a str>,
}

fn extract_stream_chunk(value: &Value) -> StreamChunk<'_> {
    StreamChunk {
        content: value
            .pointer("/choices/0/delta/content")
            .and_then(|x| x.as_str())
            .or_else(|| {
                value
                    .pointer("/choices/0/message/content")
                    .and_then(|x| x.as_str())
            }),
        thinking: value
            .pointer("/choices/0/delta/reasoning_content")
            .and_then(|x| x.as_str())
            .or_else(|| {
                value
                    .pointer("/choices/0/message/reasoning_content")
                    .and_then(|x| x.as_str())
            }),
    }
}

fn chat_completions_url(base_url: &str) -> String {
    format!("{}{}", base_url.trim_end_matches('/'), CHAT_COMPLETIONS_PATH)
}

fn log_chat_template_mode(model_path: &Path, streaming: bool, max_tokens: i32) {
    eprintln!(
        "llama.cpp: local GGUF request uses {CHAT_COMPLETIONS_PATH} only (GUI never uses raw /completion); GGUF chat_template metadata is expected to format messages and llama-server is launched with --jinja. model={} stream={} max_tokens={}",
        model_path.display(),
        streaming,
        max_tokens
    );
}

fn llama_server_args(model_path: &Path, port: u16, context_length: i32) -> Vec<OsString> {
    vec![
        OsString::from("--model"),
        model_path.as_os_str().to_os_string(),
        OsString::from("--host"),
        OsString::from("127.0.0.1"),
        OsString::from("--port"),
        OsString::from(port.to_string()),
        OsString::from("--ctx-size"),
        OsString::from(context_length.to_string()),
        OsString::from("--threads"),
        OsString::from(available_thread_count().to_string()),
        OsString::from("--jinja"),
    ]
}

fn spawn_llama_server(
    binary: &Path,
    model_path: &Path,
    port: u16,
    context_length: i32,
    logs: Arc<Mutex<String>>,
) -> Result<Child, String> {
    let mut command = Command::new(binary);
    command
        .args(llama_server_args(model_path, port, context_length))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = command
        .spawn()
        .map_err(|e| format!("llama-server の起動に失敗しました: {e}"))?;

    if let Some(stdout) = child.stdout.take() {
        spawn_log_drain(stdout, "stdout", Arc::clone(&logs));
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_log_drain(stderr, "stderr", logs);
    }

    Ok(child)
}

fn spawn_log_drain<R>(reader: R, label: &'static str, logs: Arc<Mutex<String>>)
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let reader = BufReader::new(reader);
        for line in reader.lines().map_while(Result::ok) {
            if line.trim().is_empty() {
                continue;
            }
            append_log(&logs, &format!("[{label}] {line}"));
        }
    });
}

fn append_log(logs: &Arc<Mutex<String>>, line: &str) {
    if let Ok(mut buf) = logs.lock() {
        buf.push_str(line);
        buf.push('\n');
        if buf.len() > MAX_LOG_BYTES {
            let start = buf.len().saturating_sub(MAX_LOG_BYTES);
            let trimmed = buf[start..].to_string();
            *buf = trimmed;
        }
    }
}

fn wait_until_ready(
    child: &mut Child,
    base_url: &str,
    model_path: &Path,
    logs: Arc<Mutex<String>>,
) -> Result<String, String> {
    let started = Instant::now();
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("llama-server の状態確認に失敗しました: {e}"))?
        {
            let tail = logs.lock().map(|x| x.clone()).unwrap_or_default();
            return Err(format!(
                "llama-server が起動直後に終了しました (status: {status})。モデル: {}{}{}",
                model_path.display(),
                if tail.is_empty() {
                    ""
                } else {
                    "\n\n直近ログ:\n"
                },
                tail
            ));
        }

        if let Some(model_id) = fetch_model_id(base_url) {
            return Ok(model_id);
        }

        if started.elapsed() >= STARTUP_TIMEOUT {
            let _ = child.kill();
            let tail = logs.lock().map(|x| x.clone()).unwrap_or_default();
            return Err(format!(
                "llama-server の起動待ちがタイムアウトしました。モデル: {}{}{}",
                model_path.display(),
                if tail.is_empty() {
                    ""
                } else {
                    "\n\n直近ログ:\n"
                },
                tail
            ));
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn fetch_model_id(base_url: &str) -> Option<String> {
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let resp = ureq::get(&url).call().ok()?;
    if resp.status() >= 400 {
        return None;
    }
    let text = resp.into_string().ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v.pointer("/data/0/id")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
}

fn server_is_alive(server: &mut CachedServer) -> Result<bool, String> {
    Ok(server
        .child
        .try_wait()
        .map_err(|e| format!("llama-server の状態確認に失敗しました: {e}"))?
        .is_none())
}

fn stop_server(server: &mut CachedServer) {
    let _ = server.child.kill();
    let _ = server.child.wait();
}

fn pick_free_port() -> Result<u16, String> {
    TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("空きポートの確保に失敗しました: {e}"))?
        .local_addr()
        .map_err(|e| format!("空きポートの取得に失敗しました: {e}"))
        .map(|addr| addr.port())
}

fn normalize_context_length(context_length: i32) -> i32 {
    if context_length > 0 {
        context_length.max(MIN_CONTEXT_LENGTH)
    } else {
        DEFAULT_CONTEXT_LENGTH
    }
}

fn normalize_max_tokens(max_tokens: i32) -> i32 {
    model_prefs::effective_local_max_output_tokens(max_tokens)
}

fn normalize_model_path(model_path: &Path) -> PathBuf {
    model_path
        .canonicalize()
        .unwrap_or_else(|_| model_path.to_path_buf())
}

fn available_thread_count() -> usize {
    thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8)
}

fn find_llama_server_binary() -> Option<PathBuf> {
    llama_cpp_runtime::bundled_server_binary()
}

fn missing_bundled_runtime_message() -> String {
    let search_roots = llama_cpp_runtime::bundled_runtime_search_dirs();
    format!(
        "内蔵 llama-server が見つかりません。`open_agents.exe` と同じフォルダに同梱 runtime が配置されている必要があります。`cargo build --release -p open-agents-gui` をやり直すか、配布物を再配置してください。探索先: {}",
        search_roots
            .iter()
            .map(|dir| dir.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_id_falls_back_to_stem_when_server_not_ready() {
        let path = Path::new(r"C:\models\gemma-4-27b-it-Q4_K_M.gguf");
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        assert_eq!(stem, "gemma-4-27b-it-Q4_K_M");
    }

    #[test]
    fn missing_runtime_message_mentions_bundled_runtime() {
        let msg = missing_bundled_runtime_message();
        assert!(msg.contains("内蔵 llama-server"));
        assert!(msg.contains("cargo build --release -p open-agents-gui"));
    }

    #[test]
    fn bundled_runtime_search_dirs_are_available() {
        let dirs = llama_cpp_runtime::bundled_runtime_search_dirs();
        assert!(!dirs.is_empty());
    }

    #[test]
    fn context_length_is_normalized_for_cache_key() {
        assert_eq!(normalize_context_length(0), DEFAULT_CONTEXT_LENGTH);
        assert_eq!(normalize_context_length(128), MIN_CONTEXT_LENGTH);
        assert_eq!(normalize_context_length(8192), 8192);
    }

    #[test]
    fn max_tokens_are_normalized_for_local_runtime() {
        assert_eq!(
            normalize_max_tokens(0),
            model_prefs::DEFAULT_MAX_OUTPUT_TOKENS
        );
        assert_eq!(
            normalize_max_tokens(2048),
            model_prefs::LOCAL_GGUF_MAX_OUTPUT_TOKENS_CAP
        );
    }

    #[test]
    fn local_runtime_targets_chat_completions_endpoint() {
        assert_eq!(
            chat_completions_url("http://127.0.0.1:8080/"),
            "http://127.0.0.1:8080/v1/chat/completions"
        );
    }

    #[test]
    fn llama_server_args_enable_jinja_templates() {
        let args = llama_server_args(Path::new("C:/models/gemma-4.gguf"), 8080, 8192);
        assert!(args.iter().any(|arg| arg == "--jinja"));
    }

    #[test]
    fn extracts_stream_chunk_text_from_delta_shape() {
        let value: Value =
            serde_json::from_str(r#"{"choices":[{"delta":{"content":"こん"}}]}"#).unwrap();
        let chunk = extract_stream_chunk(&value);
        assert_eq!(chunk.content, Some("こん"));
        assert_eq!(chunk.thinking, None);
    }

    #[test]
    fn extracts_stream_reasoning_chunk_from_delta_shape() {
        let value: Value =
            serde_json::from_str(r#"{"choices":[{"delta":{"reasoning_content":"考"}}]}"#)
                .unwrap();
        let chunk = extract_stream_chunk(&value);
        assert_eq!(chunk.content, None);
        assert_eq!(chunk.thinking, Some("考"));
    }

    #[test]
    fn reads_streaming_sse_payloads() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"こん\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"にちは\"}}]}\n\n",
            "data: [DONE]\n"
        );
        let mut chunks = Vec::new();
        let response = read_streaming_response(
            body.as_bytes(),
            &mut |delta| {
                chunks.push(delta.to_string());
            },
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(chunks, vec!["こん".to_string(), "にちは".to_string()]);
        assert_eq!(response.content, "こんにちは");
        assert_eq!(response.thinking, None);
    }

    #[test]
    fn reads_streaming_reasoning_payloads() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"考\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"える\"}}]}\n\n",
            "data: [DONE]\n"
        );
        let mut chunks = Vec::new();
        let response = read_streaming_response(
            body.as_bytes(),
            &mut |_| {},
            &mut |delta| {
                chunks.push(delta.to_string());
            },
        )
        .unwrap();
        assert_eq!(chunks, vec!["考".to_string(), "える".to_string()]);
        assert_eq!(response.content, "");
        assert_eq!(response.thinking, Some("考える".to_string()));
    }

    #[test]
    fn extracts_reasoning_content_from_openai_response() {
        let response = extract_openai_message(
            r#"{"choices":[{"message":{"role":"assistant","content":"","reasoning_content":"思考"}}]}"#,
        )
        .unwrap();
        assert_eq!(response.content, "");
        assert_eq!(response.thinking, Some("思考".to_string()));
    }
}
