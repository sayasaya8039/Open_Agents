//! Chatセッション管理 — マルチセッション + JSON永続化
//!
//! 保存先: `%LOCALAPPDATA%\\open_agents_gui\\chat_sessions.json`

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::i18n;

/// 個別チャットメッセージ
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMsg {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<ChatMsgMetrics>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChatMsgMetrics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens_per_second: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
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
            title: i18n::new_conversation().into(),
            messages: vec![ChatMsg {
                role: "assistant".into(),
                content: i18n::greeting_message().into(),
                thinking: None,
                metrics: None,
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

    /// セッション名を更新
    pub fn rename_session(&mut self, id: u64, title: impl AsRef<str>) -> bool {
        let title = title.as_ref().trim();
        if title.is_empty() {
            return false;
        }
        let Some(session) = self.sessions.iter_mut().find(|session| session.id == id) else {
            return false;
        };
        session.title = title.to_string();
        session.touch();
        true
    }

    /// セッションを複製してアクティブにする
    pub fn duplicate_session(&mut self, id: u64) -> Option<u64> {
        let source = self
            .sessions
            .iter()
            .find(|session| session.id == id)?
            .clone();
        let now = unix_now();
        let duplicate_id = self.next_session_id();
        let duplicate = ChatSession {
            id: duplicate_id,
            title: format!("{}{}", source.title, i18n::copy_suffix()),
            messages: source.messages,
            created_at: now,
            updated_at: now,
        };
        self.sessions.push(duplicate);
        self.active_id = Some(duplicate_id);
        Some(duplicate_id)
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

    /// 日付グループ単位でセッションを削除
    pub fn delete_group(&mut self, label: &str) {
        let now = unix_now();
        let removed_active = self.active_id.is_some_and(|active_id| {
            self.sessions.iter().any(|session| {
                session.id == active_id
                    && group_label_for(session.updated_at, now) == label
            })
        });

        self.sessions
            .retain(|session| group_label_for(session.updated_at, now) != label);

        if self.sessions.is_empty() {
            let session = ChatSession::new();
            self.active_id = Some(session.id);
            self.sessions.push(session);
        } else if removed_active {
            self.sessions
                .sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
            self.active_id = Some(self.sessions[0].id);
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
        let sorted = self.sorted_sessions();
        let mut today = Vec::new();
        let mut yesterday = Vec::new();
        let mut earlier = Vec::new();

        for s in sorted {
            match group_index_for(s.updated_at, now) {
                0 => today.push(s),
                1 => yesterday.push(s),
                _ => earlier.push(s),
            }
        }

        let mut groups = Vec::new();
        if !today.is_empty() {
            groups.push((i18n::group_today(), today));
        }
        if !yesterday.is_empty() {
            groups.push((i18n::group_yesterday(), yesterday));
        }
        if !earlier.is_empty() {
            groups.push((i18n::group_earlier(), earlier));
        }
        groups
    }

    pub fn export_session_json(&self, id: u64) -> Option<String> {
        let session = self.sessions.iter().find(|session| session.id == id)?;
        serde_json::to_string(session).ok()
    }

    fn next_session_id(&self) -> u64 {
        let mut candidate = unix_now() as u64;
        while self.sessions.iter().any(|session| session.id == candidate) {
            candidate += 1;
        }
        candidate
    }
}

// ── 永続化 ──

fn sessions_file() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("open_agents_gui")
        .join("chat_sessions.json")
}

pub fn sessions_file_path() -> PathBuf {
    sessions_file()
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
    match serde_json::to_string(store) {
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

/// 日付グループ判定 — 0=today, 1=yesterday, 2=earlier
fn group_index_for(updated_at: i64, now: i64) -> usize {
    let today_start = now - (now % 86400);
    let yesterday_start = today_start - 86400;
    if updated_at >= today_start {
        0
    } else if updated_at >= yesterday_start {
        1
    } else {
        2
    }
}

fn group_label_for(updated_at: i64, now: i64) -> &'static str {
    match group_index_for(updated_at, now) {
        0 => i18n::group_today(),
        1 => i18n::group_yesterday(),
        _ => i18n::group_earlier(),
    }
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
            metrics: None,
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
    fn rename_session_updates_title_and_rejects_blank_names() {
        let mut store = SessionStore::default();
        let id = store.active_id.unwrap();
        assert!(store.rename_session(id, "  新しいタイトル  "));
        assert_eq!(store.active().unwrap().title, "新しいタイトル");
        assert!(!store.rename_session(id, "   "));
        assert_eq!(store.active().unwrap().title, "新しいタイトル");
    }

    #[test]
    fn duplicate_session_clones_messages_and_activates_copy() {
        let mut store = SessionStore::default();
        let id = store.active_id.unwrap();
        let original = store.active().unwrap().clone();

        let duplicate_id = store.duplicate_session(id).expect("session duplicated");
        let duplicate = store.active().expect("duplicate active");

        assert_eq!(store.active_id, Some(duplicate_id));
        assert_ne!(duplicate.id, original.id);
        assert_eq!(duplicate.messages.len(), original.messages.len());
        assert_eq!(duplicate.messages[0].content, original.messages[0].content);
        assert!(duplicate.title.ends_with("のコピー"));
    }

    #[test]
    fn export_session_json_contains_selected_session_title() {
        let store = SessionStore::default();
        let id = store.active_id.unwrap();
        let exported = store.export_session_json(id).expect("session exported");
        assert!(exported.contains("\"title\""));
        assert!(exported.contains("新しい会話"));
    }

    #[test]
    fn grouped_sessions_contain_today() {
        let store = SessionStore::default();
        let groups = store.grouped_sessions();
        assert!(!groups.is_empty());
        assert_eq!(groups[0].0, "Today");
    }

    #[test]
    fn delete_group_removes_only_target_group_sessions() {
        let now = unix_now();
        let today = ChatSession {
            id: 1,
            title: "today".into(),
            messages: vec![],
            created_at: now,
            updated_at: now,
        };
        let yesterday = ChatSession {
            id: 2,
            title: "yesterday".into(),
            messages: vec![],
            created_at: now - 86400,
            updated_at: now - 86400,
        };

        let mut store = SessionStore {
            sessions: vec![today, yesterday],
            active_id: Some(1),
        };

        store.delete_group("Today");

        assert_eq!(store.sessions.len(), 1);
        assert_eq!(store.sessions[0].id, 2);
        assert_eq!(store.active_id, Some(2));
    }

    #[test]
    fn delete_group_creates_fallback_session_when_last_group_removed() {
        let mut store = SessionStore::default();
        let deleted_id = store.active_id.unwrap();

        store.delete_group("Today");

        assert_eq!(store.sessions.len(), 1);
        assert_ne!(store.sessions[0].id, deleted_id);
        assert_eq!(store.active_id, Some(store.sessions[0].id));
    }
}
