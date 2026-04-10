//! 内蔵 HTTP API サーバー — OpenAI 互換 `/v1/chat/completions` プロキシ。
//!
//! `tiny_http` で別スレッド起動し、受け取ったリクエストを現在アクティブな
//! バックエンド（クラウド API / Ollama / llama-server）にそのまま転送する。
//! localhost のみリッスン。Bearer トークン認証。

use std::io::Read as IoRead;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::Duration;

/// リクエストボディ上限（10MB）
const MAX_REQUEST_BODY: u64 = 10 * 1024 * 1024;

/// API キープレフィックス
const KEY_PREFIX: &str = "oag-";

/// ランダム API キーを生成（`oag-<uuid>` 形式）
pub fn generate_api_key() -> String {
    format!(
        "{}{}",
        KEY_PREFIX,
        uuid::Uuid::new_v4().to_string().replace('-', "")
    )
}

/// API サーバー起動時に渡す設定
#[derive(Clone, Debug)]
pub struct ApiServerConfig {
    pub port: u16,
    pub api_key: String,
    /// 転送先 URL（例: "http://localhost:8080/v1", "https://api.openai.com/v1"）
    pub proxy_target_url: String,
    /// 転送先の API キー（クラウドの場合）
    pub proxy_target_key: String,
}

/// 別スレッドで動く tiny_http ベースの API サーバー
pub struct ApiServer {
    handle: Option<thread::JoinHandle<()>>,
    shutdown: Arc<AtomicBool>,
    port: u16,
}

impl ApiServer {
    /// サーバーを起動。ポートが使用中の場合は `Err` を返す。
    pub fn start(config: ApiServerConfig) -> Result<Self, String> {
        let addr = format!("127.0.0.1:{}", config.port);
        let server = tiny_http::Server::http(&addr).map_err(|e| {
            format!(
                "ポート {} でサーバーを起動できません: {}",
                config.port, e
            )
        })?;

        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = shutdown.clone();
        let port = config.port;

        let handle = thread::spawn(move || {
            run_server(server, config, shutdown_clone);
        });

        Ok(Self {
            handle: Some(handle),
            shutdown,
            port,
        })
    }

    /// サーバーを停止
    pub fn stop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        // シャットダウン通知のためにダミーリクエストを送信して accept() を解除
        let _ = ureq::get(&format!("http://127.0.0.1:{}/health", self.port))
            .timeout(Duration::from_millis(500))
            .call();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }

    pub fn is_running(&self) -> bool {
        self.handle.as_ref().map_or(false, |h| !h.is_finished())
    }

    #[allow(dead_code)]
    pub fn base_url(&self) -> String {
        format!("http://localhost:{}", self.port)
    }
}

impl Drop for ApiServer {
    fn drop(&mut self) {
        if self.handle.is_some() {
            self.stop();
        }
    }
}

// ── サーバーメインループ ──

fn run_server(server: tiny_http::Server, config: ApiServerConfig, shutdown: Arc<AtomicBool>) {
    while !shutdown.load(Ordering::SeqCst) {
        // 500ms タイムアウトで accept — シャットダウンを素早く検知
        let req = match server.recv_timeout(Duration::from_millis(500)) {
            Ok(Some(r)) => r,
            Ok(None) => continue,
            Err(_) => break,
        };

        if shutdown.load(Ordering::SeqCst) {
            let _ = respond_cors(req, 200, r#"{"status":"shutting_down"}"#);
            break;
        }

        handle_request(req, &config);
    }
}

fn handle_request(req: tiny_http::Request, config: &ApiServerConfig) {
    let method = req.method().to_string();
    let url = req.url().to_string();

    // CORS preflight
    if method == "OPTIONS" {
        let _ = respond_cors(req, 204, "");
        return;
    }

    // ルーティング
    match (method.as_str(), url.as_str()) {
        ("GET", "/health") => {
            let _ = respond_cors(req, 200, r#"{"status":"ok"}"#);
        }
        ("GET", "/v1/models") => {
            if !check_auth(&req, &config.api_key) {
                let _ = respond_cors(
                    req,
                    401,
                    r#"{"error":{"message":"Invalid API key","type":"invalid_request_error","code":"invalid_api_key"}}"#,
                );
                return;
            }
            let body = r#"{"object":"list","data":[{"id":"open-agents","object":"model","created":0,"owned_by":"open-agents"}]}"#;
            let _ = respond_cors(req, 200, body);
        }
        ("POST", "/v1/chat/completions") => {
            if !check_auth(&req, &config.api_key) {
                let _ = respond_cors(
                    req,
                    401,
                    r#"{"error":{"message":"Invalid API key","type":"invalid_request_error","code":"invalid_api_key"}}"#,
                );
                return;
            }
            handle_chat_completions(req, config);
        }
        _ => {
            let body = format!(
                r#"{{"error":{{"message":"Unknown endpoint: {} {}","type":"invalid_request_error"}}}}"#,
                method, url
            );
            let _ = respond_cors(req, 404, &body);
        }
    }
}

fn check_auth(req: &tiny_http::Request, expected_key: &str) -> bool {
    for header in req.headers() {
        let field = header.field.as_str().as_str();
        if field.eq_ignore_ascii_case("authorization") {
            let val = header.value.as_str();
            if let Some(token) = val.strip_prefix("Bearer ") {
                return token.trim() == expected_key;
            }
        }
    }
    false
}

fn handle_chat_completions(mut req: tiny_http::Request, config: &ApiServerConfig) {
    // リクエストサイズ制限チェック
    if let Some(len) = req.body_length() {
        if len as u64 > MAX_REQUEST_BODY {
            let _ = respond_cors(
                req,
                413,
                r#"{"error":{"message":"Request body too large (max 10MB)","type":"invalid_request_error"}}"#,
            );
            return;
        }
    }

    // リクエストボディ読み取り
    let mut body = String::new();
    if let Err(e) = req.as_reader().read_to_string(&mut body) {
        let err = format!(
            r#"{{"error":{{"message":"リクエスト読み取り失敗: {}","type":"server_error"}}}}"#,
            e
        );
        let _ = respond_cors(req, 400, &err);
        return;
    }

    if config.proxy_target_url.is_empty() {
        let _ = respond_cors(
            req,
            503,
            r#"{"error":{"message":"バックエンドが未設定です。設定画面でモデルを選択してください。","type":"server_error"}}"#,
        );
        return;
    }

    // 転送先 URL を構築
    let target_base = config.proxy_target_url.trim_end_matches('/');
    let target_url = if target_base.ends_with("/v1")
        || target_base.ends_with("/v1beta/openai")
        || target_base.ends_with("/v2")
    {
        format!("{}/chat/completions", target_base)
    } else if target_base.contains("/api/chat") {
        // Ollama 形式はそのまま
        target_base.to_string()
    } else {
        format!("{}/v1/chat/completions", target_base)
    };

    // 転送
    let mut upstream_req = ureq::post(&target_url).set("Content-Type", "application/json");

    if !config.proxy_target_key.is_empty() {
        upstream_req = upstream_req.set(
            "Authorization",
            &format!("Bearer {}", config.proxy_target_key),
        );
    }

    match upstream_req.send_string(&body) {
        Ok(resp) => {
            let status = resp.status();
            let resp_body = resp.into_string().unwrap_or_default();
            let _ = respond_cors(req, status, &resp_body);
        }
        Err(ureq::Error::Status(code, resp)) => {
            let resp_body = resp.into_string().unwrap_or_default();
            let _ = respond_cors(req, code, &resp_body);
        }
        Err(e) => {
            let err = format!(
                r#"{{"error":{{"message":"バックエンド転送失敗: {}","type":"server_error"}}}}"#,
                e
            );
            let _ = respond_cors(req, 502, &err);
        }
    }
}

// ── レスポンスヘルパー ──

fn cors_headers() -> [tiny_http::Header; 4] {
    static HEADERS: OnceLock<[tiny_http::Header; 4]> = OnceLock::new();
    HEADERS
        .get_or_init(|| {
            [
                tiny_http::Header::from_bytes(b"Content-Type" as &[u8], b"application/json" as &[u8]).unwrap(),
                tiny_http::Header::from_bytes(b"Access-Control-Allow-Origin" as &[u8], b"*" as &[u8]).unwrap(),
                tiny_http::Header::from_bytes(b"Access-Control-Allow-Methods" as &[u8], b"GET, POST, OPTIONS" as &[u8]).unwrap(),
                tiny_http::Header::from_bytes(b"Access-Control-Allow-Headers" as &[u8], b"Content-Type, Authorization" as &[u8]).unwrap(),
            ]
        })
        .clone()
}

fn respond_cors(req: tiny_http::Request, status: u16, body: &str) -> Result<(), ()> {
    let [ct, origin, methods, headers] = cors_headers();
    let response = tiny_http::Response::from_string(body.to_string())
        .with_status_code(status)
        .with_header(ct)
        .with_header(origin)
        .with_header(methods)
        .with_header(headers);
    req.respond(response).map_err(|_| ())
}
