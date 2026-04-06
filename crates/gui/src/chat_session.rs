//! Chatセッション管理 — マルチセッション + JSON永続化
//!
//! 保存先: `%LOCALAPPDATA%\\open_agents_gui\\chat_sessions.json`

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// 個別チャットメッセージ
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMsg {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
}

/// 1つのチャットセッション
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatSession {
    pub id: u64,
    pub title: String,
    pub messages: Vec<ChatMsg>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl ChatSession {
    pub fn new() -> Self {
        let now = unix_now();
        Self {
            id: now as u64,
            title: "新しい会話".into(),
            messages: vec![ChatMsg {
                role: "assistant".into(),
                content: "こんにちは！Open Agents AIコーディングアシスタントです。コードの作成、編集、リファクタ��ングなど、お手伝いします。".into(),
                thinking: None,
            }],
            created_at: now,
            updated_at: now,
        }
    }

    /// 最初のユーザーメッセージからタイトルを自動生成
    pub fn auto_title(&mut self) {
        if let Some(msg) = self.messages.iter().find(|m| m.role == "user") {
            let text = msg.content.trim();
            if !text.is_empty() {
                let title: String = text.chars().take(30).collect();
                self.title = if text.chars().count() > 30 {
                    format!("{title}…")
                } else {
                    title
                };
            }
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = unix_now();
    }
}

/// セッション一覧を管理
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionStore {
    pub sessions: Vec<ChatSession>,
    pub active_id: Option<u64>,
}

impl Default for SessionStore {
    fn default() -> Self {
        let session = ChatSession::new();
        let id = session.id;
        Self {
            sessions: vec![session],
            active_id: Some(id),
        }
    }
}

impl SessionStore {
    /// アクティブセッションへの参照
    pub fn active(&self) -> Option<&ChatSession> {
        let id = self.active_id?;
        self.sessions.iter().find(|s| s.id == id)
    }

    /// アクテ���ブセッションへの可変参照
    pub fn active_mut(&mut self) -> Option<&mut ChatSession> {
        let id = self.active_id?;
        self.sessions.iter_mut().find(|s| s.id == id)
    }

    /// 新しいセッションを作成しアクティブにする
    pub fn new_session(&mut self) -> u64 {
        let session = ChatSession::new();
        let id = session.id;
        self.sessions.push(session);
        self.active_id = Some(id);
        id
    }

    /// セッションを切り替え
    pub fn switch_to(&mut self, id: u64) -> bool {
        if self.sessions.iter().any(|s| s.id == id) {
            self.active_id = Some(id);
            true
        } else {
            false
        }
    }

    /// ��ッションを削除（最後の1つは削除不可、新規作成して切替）
    pub fn delete_session(&mut self, id: u64) {
        self.sessions.retain(|s| s.id != id);
        if self.sessions.is_empty() {
            let s = ChatSession::new();
            self.active_id = Some(s.id);
            self.sessions.push(s);
        } else if self.active_id == Some(id) {
            self.active_id = Some(self.sessions.last().unwrap().id);
        }
    }

    /// updated_at 降順でソート済みセッション一覧
    pub fn sorted_sessions(&self) -> Vec<&ChatSession> {
        let mut sorted: Vec<&ChatSession> = self.sessions.iter().collect();
        sorted.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        sorted
    }

    /// 日付グループ分け (Today / Yesterday / Earlier)
    pub fn grouped_sessions(&self) -> Vec<(&'static str, Vec<&ChatSession>)> {
        let now = unix_now();
        let today_start = now - (now % 86400);
        let yesterday_start = today_start - 86400;

        let sorted = self.sorted_sessions();
        let mut today = Vec::new();
        let mut yesterday = Vec::new();
        let mut earlier = Vec::new();

        for s in sorted {
            if s.updated_at >= today_start {
                today.push(s);
            } else if s.updated_at >= yesterday_start {
                yesterday.push(s);
            } else {
                earlier.push(s);
            }
        }

        let mut groups = Vec::new();
        if !today.is_empty() {
            groups.push(("Today", today));
        }
        if !yesterday.is_empty() {
            groups.push(("Yesterday", yesterday));
        }
        if !earlier.is_empty() {
            groups.push(("Earlier", earlier));
        }
        groups
    }
}

// ── 永続化 ──

fn sessions_file() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("open_agents_gui")
        .join("chat_sessions.json")
}

pub fn load_sessions() -> SessionStore {
    let path = sessions_file();
    let Ok(raw) = fs::read_to_string(&path) else {
        return SessionStore::default();
    };
    serde_json::from_str::<SessionStore>(&raw).unwrap_or_default()
}

pub fn save_sessions(store: &SessionStore) {
    let path = sessions_file();
    let Some(parent) = path.parent() else {
        return;
    };
    if let Err(e) = fs::create_dir_all(parent) {
        eprintln!("chat_sessions: ディレクトリ作成に失敗: {e}");
        return;
    }
    match serde_json::to_string_pretty(store) {
        Ok(json) => {
            if let Err(e) = fs::write(&path, json) {
                eprintln!("chat_sessions: 書き込み失敗: {e}");
            }
        }
        Err(e) => eprintln!("chat_sessions: JSON 生成失敗: {e}"),
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_session_creates_with_greeting() {
        let s = ChatSession::new();
        assert_eq!(s.messages.len(), 1);
        assert_eq!(s.messages[0].role, "assistant");
    }

    #[test]
    fn auto_title_from_first_user_message() {
        let mut s = ChatSession::new();
        s.messages.push(ChatMsg {
            role: "user".into(),
            content: "Rustでファイル読み込みのコードを書いて".into(),
            thinking: None,
        });
        s.auto_title();
        assert!(s.title.contains("Rust"));
        assert!(s.title.chars().count() <= 31); // 30 + "…"
    }

    #[test]
    fn store_new_and_switch() {
        let mut store = SessionStore::default();
        let first_id = store.active_id.unwrap();
        let second_id = store.new_session();
        assert_ne!(first_id, second_id);
        assert_eq!(store.active_id, Some(second_id));
        assert!(store.switch_to(first_id));
        assert_eq!(store.active_id, Some(first_id));
    }

    #[test]
    fn delete_last_session_creates_new() {
        let mut store = SessionStore::default();
        let id = store.active_id.unwrap();
        store.delete_session(id);
        assert_eq!(store.sessions.len(), 1);
        assert_ne!(store.active_id, Some(id));
    }

    #[test]
    fn grouped_sessions_contain_today() {
        let store = SessionStore::default();
        let groups = store.grouped_sessions();
        assert!(!groups.is_empty());
        assert_eq!(groups[0].0, "Today");
    }
}
