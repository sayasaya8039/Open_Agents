//! Hugging Face モデル検索・詳細取得・ダウンロード機能
//!
//! - `search_hf_models`: HF の検索 API を叩いて GGUF モデル一覧を取得
//! - `fetch_model_detail`: 単一モデルの詳細（ファイル一覧 + README）を取得
//! - `DownloadManager`: 並列でないシンプルなダウンロードタスク管理
//!
//! すべてブロッキング ureq API を使い、呼び出し側 (`cx.spawn` + `smol::unblock`)
//! から実行する前提。

use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

// ============================================================
// 型定義
// ============================================================

/// 検索結果1件のサマリ
#[derive(Clone, Debug)]
pub struct HfModelSummary {
    pub id: String,                // 例: "bartowski/Meta-Llama-3-8B-Instruct-GGUF"
    pub author: String,            // "bartowski"
    pub name: String,              // "Meta-Llama-3-8B-Instruct-GGUF"
    pub downloads: u64,
    pub likes: u64,
    pub updated_at: String,        // ISO 8601
    /// 現在 UI 未表示だがフィルタ候補用に保持
    #[allow(dead_code)]
    pub pipeline_tag: Option<String>,
    /// caps 推定に使用（UI には detail 側でのみ表示）
    #[allow(dead_code)]
    pub tags: Vec<String>,
    pub caps: CapabilitySet,
}

/// タグから推定した能力セット
#[derive(Clone, Copy, Debug, Default)]
pub struct CapabilitySet {
    pub vision: bool,
    pub tool_use: bool,
    pub reasoning: bool,
}

impl CapabilitySet {
    pub fn from_tags(tags: &[String], pipeline: Option<&str>) -> Self {
        let joined: String = tags
            .iter()
            .map(|s| s.to_lowercase())
            .collect::<Vec<_>>()
            .join(",");
        let vision = joined.contains("multimodal")
            || joined.contains("vision")
            || joined.contains("image-text-to-text")
            || pipeline == Some("image-text-to-text")
            || pipeline == Some("visual-question-answering");
        let tool_use = joined.contains("tool")
            || joined.contains("function-calling")
            || joined.contains("agent");
        let reasoning = joined.contains("reasoning")
            || joined.contains("r1")
            || joined.contains("o1")
            || joined.contains("thinking")
            || joined.contains("math");
        Self {
            vision,
            tool_use,
            reasoning,
        }
    }
}

/// ソート順
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortOrder {
    Trending,
    Downloads,
    Likes,
    LastModified,
}

impl SortOrder {
    pub fn as_query(self) -> &'static str {
        match self {
            Self::Trending => "trendingScore",
            Self::Downloads => "downloads",
            Self::Likes => "likes",
            Self::LastModified => "lastModified",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Trending => "Trending",
            Self::Downloads => "Downloads",
            Self::Likes => "Likes",
            Self::LastModified => "Last Modified",
        }
    }
}

/// Discover ページの状態（AppView に保持）
pub struct HuggingFaceSearchState {
    pub query: String,
    pub sort: SortOrder,
    pub results: Vec<HfModelSummary>,
    pub loading: bool,
    pub error: Option<String>,
    pub selected_id: Option<String>,
    pub detail: Option<HfModelDetail>,
    pub detail_loading: bool,
    pub detail_error: Option<String>,
    /// 最終検索リクエストID（古いレスポンスを捨てるため）
    pub request_gen: u64,
}

impl Default for HuggingFaceSearchState {
    fn default() -> Self {
        Self {
            query: String::new(),
            sort: SortOrder::Trending,
            results: Vec::new(),
            loading: false,
            error: None,
            selected_id: None,
            detail: None,
            detail_loading: false,
            detail_error: None,
            request_gen: 0,
        }
    }
}

/// モデル詳細
#[derive(Clone, Debug)]
pub struct HfModelDetail {
    pub id: String,
    pub files: Vec<GgufFile>,
    pub readme: String,
    pub caps: CapabilitySet,
    pub tags: Vec<String>,
    pub downloads: u64,
    pub likes: u64,
}

/// GGUF ファイル1件
#[derive(Clone, Debug)]
pub struct GgufFile {
    pub filename: String,
    pub size_bytes: u64,
    /// Q4_K_M / f16 / bf16 など
    pub quant: Option<String>,
    pub fit_hint: GpuFitHint,
}

/// ファイルサイズから GPU 適合ヒント
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuFitHint {
    Small,   // <= 6GB
    Medium,  // <= 16GB
    Large,   // <= 32GB
    Huge,    // > 32GB
}

impl GpuFitHint {
    pub fn from_size(size: u64) -> Self {
        const GB: u64 = 1024 * 1024 * 1024;
        if size <= 6 * GB {
            Self::Small
        } else if size <= 16 * GB {
            Self::Medium
        } else if size <= 32 * GB {
            Self::Large
        } else {
            Self::Huge
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "~6GB VRAM",
            Self::Medium => "~16GB VRAM",
            Self::Large => "~32GB VRAM",
            Self::Huge => ">32GB VRAM",
        }
    }
}

// ============================================================
// 検索 API
// ============================================================

fn user_agent() -> String {
    format!("OpenAgents/{}", env!("CARGO_PKG_VERSION"))
}

fn build_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(30))
        .user_agent(&user_agent())
        .build()
}

fn build_request(url: &str, hf_token: Option<&str>) -> ureq::Request {
    let agent = build_agent();
    let mut req = agent
        .get(url)
        .set("Accept", "application/json");
    if let Some(token) = hf_token.filter(|s| !s.is_empty()) {
        req = req.set("Authorization", &format!("Bearer {token}"));
    }
    req
}

/// HF 検索 API を叩く（ブロッキング）
pub fn search_hf_models(
    query: &str,
    sort: SortOrder,
    hf_token: Option<&str>,
) -> Result<Vec<HfModelSummary>, String> {
    let q = urlencode(query);
    let url = format!(
        "https://huggingface.co/api/models?search={}&filter=gguf&sort={}&direction=-1&limit=50&full=true",
        q,
        sort.as_query()
    );
    let resp = build_request(&url, hf_token)
        .call()
        .map_err(|e| classify_http_error(&e))?;
    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| format!("JSON 解析に失敗: {e}"))?;

    let arr = json.as_array().ok_or("予期しないレスポンス形式")?;
    let mut out = Vec::new();
    for item in arr {
        let Some(id) = item.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let (author, name) = id
            .split_once('/')
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .unwrap_or_else(|| (String::new(), id.to_string()));
        let downloads = item.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0);
        let likes = item.get("likes").and_then(|v| v.as_u64()).unwrap_or(0);
        let updated_at = item
            .get("lastModified")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let pipeline_tag = item
            .get("pipeline_tag")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let tags = item
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let caps = CapabilitySet::from_tags(&tags, pipeline_tag.as_deref());
        out.push(HfModelSummary {
            id: id.to_string(),
            author,
            name,
            downloads,
            likes,
            updated_at,
            pipeline_tag,
            tags,
            caps,
        });
    }
    Ok(out)
}

/// 単一モデルの詳細情報を取得
pub fn fetch_model_detail(
    id: &str,
    hf_token: Option<&str>,
) -> Result<HfModelDetail, String> {
    // 1. 基本メタ
    let url = format!("https://huggingface.co/api/models/{id}");
    let meta: serde_json::Value = build_request(&url, hf_token)
        .call()
        .map_err(|e| classify_http_error(&e))?
        .into_json()
        .map_err(|e| format!("detail JSON 解析失敗: {e}"))?;
    let tags = meta
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let pipeline_tag = meta
        .get("pipeline_tag")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let caps = CapabilitySet::from_tags(&tags, pipeline_tag.as_deref());
    let downloads = meta.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0);
    let likes = meta.get("likes").and_then(|v| v.as_u64()).unwrap_or(0);

    // 2. ファイルツリー（ルートのみ）
    let tree_url = format!("https://huggingface.co/api/models/{id}/tree/main");
    let tree: serde_json::Value = build_request(&tree_url, hf_token)
        .call()
        .map_err(|e| classify_http_error(&e))?
        .into_json()
        .map_err(|e| format!("tree JSON 解析失敗: {e}"))?;
    let mut files = Vec::new();
    if let Some(arr) = tree.as_array() {
        for entry in arr {
            let path = entry
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if !path.to_lowercase().ends_with(".gguf") {
                continue;
            }
            let size = entry.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
            let quant = parse_quant_from_filename(path);
            files.push(GgufFile {
                filename: path.to_string(),
                size_bytes: size,
                quant,
                fit_hint: GpuFitHint::from_size(size),
            });
        }
    }
    files.sort_by_key(|f| f.size_bytes);

    // 3. README (失敗しても空で続行)
    let readme_url = format!("https://huggingface.co/{id}/raw/main/README.md");
    let readme = build_request(&readme_url, hf_token)
        .call()
        .ok()
        .and_then(|r| r.into_string().ok())
        .map(|s| markdown_to_plain(&s))
        .unwrap_or_default();

    Ok(HfModelDetail {
        id: id.to_string(),
        files,
        readme,
        caps,
        tags,
        downloads,
        likes,
    })
}

/// Markdown をプレーンテキストにざっくり変換（pulldown-cmark）
pub fn markdown_to_plain(md: &str) -> String {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};
    let parser = Parser::new(md);
    let mut out = String::new();
    for ev in parser {
        match ev {
            Event::Text(t) | Event::Code(t) => out.push_str(&t),
            Event::SoftBreak | Event::HardBreak => out.push('\n'),
            Event::Start(Tag::Heading { .. }) => out.push('\n'),
            Event::End(TagEnd::Heading(_)) => out.push_str("\n\n"),
            Event::End(TagEnd::Paragraph) => out.push_str("\n\n"),
            Event::End(TagEnd::Item) => out.push('\n'),
            _ => {}
        }
    }
    // 先頭 8KB 程度に制限
    if out.len() > 8192 {
        out.truncate(8192);
        out.push_str("\n…");
    }
    out
}

/// GGUF ファイル名から量子化名を抽出（例: `foo-Q4_K_M.gguf` → `Q4_K_M`）
pub fn parse_quant_from_filename(name: &str) -> Option<String> {
    let lower = name.to_lowercase();
    // Q*_*(K|M)* パターン
    let bytes = lower.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'q' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit() {
            let start = i;
            let mut end = i + 2;
            while end < bytes.len() {
                let c = bytes[end];
                if c.is_ascii_digit() || c == b'_' || c == b'k' || c == b'm' || c == b's' {
                    end += 1;
                } else {
                    break;
                }
            }
            return Some(name[start..end].to_ascii_uppercase());
        }
        i += 1;
    }
    // f16/f32/bf16 チェック
    for pat in ["bf16", "f16", "f32"] {
        if lower.contains(pat) {
            return Some(pat.to_ascii_uppercase());
        }
    }
    None
}

// ============================================================
// ダウンロードマネージャー
// ============================================================

/// ダウンロード1件の状態
#[derive(Clone, Debug)]
pub struct DownloadTask {
    pub id: u64,
    pub model_id: String,
    pub filename: String,
    pub url: String,
    pub dest_path: PathBuf,
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    pub status: DownloadStatus,
    pub error: Option<String>,
    pub cancel_flag: Arc<AtomicBool>,
    pub hf_token: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownloadStatus {
    Queued,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

/// ワーカーから UI に送る進捗メッセージ
#[derive(Clone, Debug)]
pub enum DownloadProgress {
    Started {
        id: u64,
        total: u64,
    },
    Progress {
        id: u64,
        downloaded: u64,
    },
    Completed {
        id: u64,
        final_path: PathBuf,
    },
    Failed {
        id: u64,
        error: String,
    },
    Cancelled {
        id: u64,
    },
}

static NEXT_DOWNLOAD_ID: AtomicU64 = AtomicU64::new(1);

/// ダウンロードマネージャ本体（AppView に保持する軽量コンテナ）
pub struct DownloadManager {
    pub tasks: Vec<DownloadTask>,
    pub panel_open: bool,
    pub tx: Sender<DownloadProgress>,
    pub rx: Receiver<DownloadProgress>,
    /// ワーカースレッドが走っているか（同時ダウンロード 1 に制限する）
    pub worker_running: Arc<AtomicBool>,
}

impl Default for DownloadManager {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tasks: Vec::new(),
            panel_open: false,
            tx,
            rx,
            worker_running: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl DownloadManager {
    pub fn enqueue(
        &mut self,
        model_id: String,
        file: &GgufFile,
        hf_token: Option<String>,
    ) -> DownloadTask {
        let id = NEXT_DOWNLOAD_ID.fetch_add(1, Ordering::Relaxed);
        let url = format!(
            "https://huggingface.co/{}/resolve/main/{}",
            model_id, file.filename
        );
        // 保存先: <models_dir>/<author>/<repo>/<filename>
        //   Windows 予約文字は各セグメントごとにサニタイズし、
        //   重複防止のため author/repo がない場合はフラットに退避
        let mut dest_path = models_dir();
        let (author, repo) = model_id
            .split_once('/')
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .unwrap_or_else(|| (String::new(), model_id.replace('/', "_")));
        if !author.is_empty() {
            dest_path.push(sanitize_file_name(&author));
        }
        dest_path.push(sanitize_file_name(&repo));
        dest_path.push(sanitize_file_name(&file.filename));
        let task = DownloadTask {
            id,
            model_id,
            filename: file.filename.clone(),
            url,
            dest_path,
            total_bytes: file.size_bytes,
            downloaded_bytes: 0,
            status: DownloadStatus::Queued,
            error: None,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            hf_token,
        };
        self.tasks.push(task.clone());
        task
    }

    /// 次に処理すべきキュー済みタスクを取り出し（存在すれば InProgress にはしないが、
    /// 呼び出し側のワーカーが DownloadProgress::Started を送ったタイミングで進捗反映される）。
    pub fn next_queued_task(&self) -> Option<DownloadTask> {
        self.tasks
            .iter()
            .find(|t| t.status == DownloadStatus::Queued)
            .cloned()
    }

    /// 完了 / 失敗 / キャンセル済みタスクを一括削除
    pub fn clear_completed(&mut self) {
        self.tasks.retain(|t| {
            !matches!(
                t.status,
                DownloadStatus::Completed
                    | DownloadStatus::Failed
                    | DownloadStatus::Cancelled
            )
        });
    }

    /// 非ブロッキングで進捗を全部受信して反映（UI 側から定期的に呼ぶ）
    pub fn drain_progress(&mut self) -> Vec<DownloadProgress> {
        let mut events = Vec::new();
        while let Ok(ev) = self.rx.try_recv() {
            events.push(ev);
        }
        for ev in &events {
            match ev {
                DownloadProgress::Started { id, total } => {
                    if let Some(t) = self.tasks.iter_mut().find(|t| t.id == *id) {
                        t.status = DownloadStatus::InProgress;
                        if *total > 0 {
                            t.total_bytes = *total;
                        }
                    }
                }
                DownloadProgress::Progress { id, downloaded } => {
                    if let Some(t) = self.tasks.iter_mut().find(|t| t.id == *id) {
                        t.downloaded_bytes = *downloaded;
                    }
                }
                DownloadProgress::Completed { id, .. } => {
                    if let Some(t) = self.tasks.iter_mut().find(|t| t.id == *id) {
                        t.status = DownloadStatus::Completed;
                        t.downloaded_bytes = t.total_bytes;
                    }
                }
                DownloadProgress::Failed { id, error } => {
                    if let Some(t) = self.tasks.iter_mut().find(|t| t.id == *id) {
                        t.status = DownloadStatus::Failed;
                        t.error = Some(error.clone());
                    }
                }
                DownloadProgress::Cancelled { id } => {
                    if let Some(t) = self.tasks.iter_mut().find(|t| t.id == *id) {
                        t.status = DownloadStatus::Cancelled;
                    }
                }
            }
        }
        events
    }

    pub fn cancel(&mut self, id: u64) {
        if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
            t.cancel_flag.store(true, Ordering::Relaxed);
        }
    }
}

/// ダウンロードを実行する（`smol::unblock` or `cx.background_executor().spawn` から呼ぶ）
pub fn run_download(
    task: DownloadTask,
    hf_token: Option<String>,
    tx: Sender<DownloadProgress>,
) {
    let id = task.id;
    let part_path = task.dest_path.with_extension("part");

    if let Some(parent) = part_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            let _ = tx.send(DownloadProgress::Failed {
                id,
                error: format!("フォルダ作成失敗: {e}"),
            });
            return;
        }
    }

    // リクエスト構築 - 大きな GGUF (5GB+) のダウンロードで途中切断しないよう
    // read timeout を 300 秒に延長
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(300))
        .user_agent(&user_agent())
        .build();
    let mut req = agent.get(&task.url);
    if let Some(tok) = hf_token.as_deref().filter(|s| !s.is_empty()) {
        req = req.set("Authorization", &format!("Bearer {tok}"));
    }

    let resp = match req.call() {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.send(DownloadProgress::Failed {
                id,
                error: classify_http_error(&e),
            });
            let _ = std::fs::remove_file(&part_path);
            return;
        }
    };

    let content_length = resp
        .header("Content-Length")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(task.total_bytes);
    let _ = tx.send(DownloadProgress::Started {
        id,
        total: content_length,
    });

    let mut reader = resp.into_reader();
    let mut file = match std::fs::File::create(&part_path) {
        Ok(f) => f,
        Err(e) => {
            let _ = tx.send(DownloadProgress::Failed {
                id,
                error: format!(".part 作成失敗: {e}"),
            });
            return;
        }
    };

    use std::io::Write;
    let mut buf = vec![0u8; 8 * 1024];
    let mut downloaded: u64 = 0;
    let mut last_report: u64 = 0;
    loop {
        if task.cancel_flag.load(Ordering::Relaxed) {
            drop(file);
            let _ = std::fs::remove_file(&part_path);
            let _ = tx.send(DownloadProgress::Cancelled { id });
            return;
        }
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                let _ = tx.send(DownloadProgress::Failed {
                    id,
                    error: format!("読み込みエラー: {e}"),
                });
                drop(file);
                let _ = std::fs::remove_file(&part_path);
                return;
            }
        };
        if let Err(e) = file.write_all(&buf[..n]) {
            let _ = tx.send(DownloadProgress::Failed {
                id,
                error: format!("書き込みエラー (ディスク容量不足?): {e}"),
            });
            drop(file);
            let _ = std::fs::remove_file(&part_path);
            return;
        }
        downloaded += n as u64;
        // 256KB ごとに進捗通知
        if downloaded - last_report >= 256 * 1024 {
            last_report = downloaded;
            let _ = tx.send(DownloadProgress::Progress { id, downloaded });
        }
    }
    drop(file);

    // リネーム
    if let Err(e) = std::fs::rename(&part_path, &task.dest_path) {
        let _ = tx.send(DownloadProgress::Failed {
            id,
            error: format!("ファイル移動失敗: {e}"),
        });
        return;
    }

    let _ = tx.send(DownloadProgress::Completed {
        id,
        final_path: task.dest_path.clone(),
    });
}

// ============================================================
// ユーティリティ
// ============================================================

pub fn models_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("open_agents_gui")
        .join("models")
}

fn urlencode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn sanitize_file_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' | '/' | '\\' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect()
}

fn classify_http_error(e: &ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, resp) => {
            let body = resp.status_text().to_string();
            match *code {
                401 => format!(
                    "認証エラー (401): HuggingFace トークンを設定画面で登録してください ({body})"
                ),
                403 => format!(
                    "アクセス拒否 (403): gated モデルかもしれません。HF 上でアクセス同意を行ってください ({body})"
                ),
                404 => format!("見つかりません (404): {body}"),
                500..=599 => format!("サーバーエラー ({code}): {body}"),
                _ => format!("HTTP エラー ({code}): {body}"),
            }
        }
        ureq::Error::Transport(t) => format!("通信エラー: {t}"),
    }
}

pub fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

pub fn format_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

// ============================================================
// テスト
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_quant_q4_k_m() {
        assert_eq!(
            parse_quant_from_filename("Meta-Llama-3-8B-Instruct.Q4_K_M.gguf").as_deref(),
            Some("Q4_K_M")
        );
    }

    #[test]
    fn parse_quant_f16() {
        assert_eq!(
            parse_quant_from_filename("model-f16.gguf").as_deref(),
            Some("F16")
        );
    }

    #[test]
    fn capability_detects_vision() {
        let caps = CapabilitySet::from_tags(
            &vec!["multimodal".to_string(), "llama".to_string()],
            Some("image-text-to-text"),
        );
        assert!(caps.vision);
    }

    #[test]
    fn capability_detects_reasoning() {
        let caps = CapabilitySet::from_tags(&vec!["reasoning".to_string()], None);
        assert!(caps.reasoning);
    }

    #[test]
    fn fit_hint_boundaries() {
        let gb = 1024u64 * 1024 * 1024;
        assert_eq!(GpuFitHint::from_size(4 * gb), GpuFitHint::Small);
        assert_eq!(GpuFitHint::from_size(12 * gb), GpuFitHint::Medium);
        assert_eq!(GpuFitHint::from_size(24 * gb), GpuFitHint::Large);
        assert_eq!(GpuFitHint::from_size(40 * gb), GpuFitHint::Huge);
    }

    #[test]
    fn human_size_formats() {
        assert_eq!(human_size(500), "500 B");
        assert_eq!(human_size(2048), "2 KB");
    }

    #[test]
    fn url_encode_basics() {
        assert_eq!(urlencode("hello world"), "hello%20world");
        assert_eq!(urlencode("llama-3"), "llama-3");
    }
}
