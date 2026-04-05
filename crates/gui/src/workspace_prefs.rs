//! 前回開いたワークスペースフォルダの永続化（ローカル AppData 等）
//!
//! - 保存先は `target` や再ビルドの影響を受けない。
//! - 起動時は [`resolve_workspace_at_launch`] でパスを canonicalize して書き戻し、
//!   表記の揺れやシンボリックリンク解決後のパスで次回も開けるようにする。

use std::fs;
use std::path::{Path, PathBuf};

/// `%LOCALAPPDATA%\open_agents_gui\last_workspace.txt` 等
fn last_workspace_file() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("open_agents_gui")
        .join("last_workspace.txt")
}

pub fn save_last_workspace(path: &Path) {
    let file = last_workspace_file();
    let Some(parent) = file.parent() else {
        return;
    };
    if let Err(e) = fs::create_dir_all(parent) {
        eprintln!("workspace_prefs: 保存先ディレクトリ作成に失敗: {e}");
        return;
    }
    let line = format!("{}\n", path.to_string_lossy());
    if let Err(e) = fs::write(&file, line) {
        eprintln!("workspace_prefs: 保存に失敗: {e}");
    }
}

/// 保存パスが存在するディレクトリなら Some（canonicalize は失敗時は元パスで判定）
pub fn load_last_workspace() -> Option<PathBuf> {
    let file = last_workspace_file();
    let s = fs::read_to_string(&file).ok()?;
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    let p = PathBuf::from(trimmed);
    if let Ok(c) = p.canonicalize() {
        if c.is_dir() {
            return Some(c);
        }
    }
    if p.is_dir() {
        Some(p)
    } else {
        None
    }
}

/// 起動時: 保存済みフォルダ、なければカレントディレクトリを使い、解決できたら設定ファイルを更新する。
pub fn resolve_workspace_at_launch() -> PathBuf {
    let path = load_last_workspace().unwrap_or_else(|| {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    });
    let resolved = match path.canonicalize() {
        Ok(c) => c,
        Err(_) => path,
    };
    if resolved.is_dir() {
        save_last_workspace(&resolved);
    }
    resolved
}
