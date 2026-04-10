// カーソル管理 — 移動・選択ロジック

use super::buffer::{Position, TextBuffer};

/// カーソル状態
pub struct CursorState {
    /// カーソル位置
    pub position: Position,
    /// 選択範囲の開始点（None = 選択なし）
    pub anchor: Option<Position>,
    /// 上下移動時の目標カラム
    pub preferred_column: Option<usize>,
}

impl CursorState {
    pub fn new() -> Self {
        Self {
            position: Position::default(),
            anchor: None,
            preferred_column: None,
        }
    }

    /// 選択範囲（開始, 終了）を正規化して返す
    pub fn selection_range(&self) -> Option<(Position, Position)> {
        self.anchor.map(|a| {
            if a <= self.position {
                (a, self.position)
            } else {
                (self.position, a)
            }
        })
    }

    pub fn has_selection(&self) -> bool {
        self.anchor.is_some()
    }

    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }

    /// 選択開始（extend_selection = true で Shift キー）
    fn begin_move(&mut self, extend_selection: bool) {
        if extend_selection {
            if self.anchor.is_none() {
                self.anchor = Some(self.position);
            }
        } else {
            self.anchor = None;
        }
    }

    // --- 移動 ---

    pub fn move_left(&mut self, buffer: &TextBuffer, extend_selection: bool) {
        self.begin_move(extend_selection);
        self.preferred_column = None;
        if self.position.column > 0 {
            // 前の文字の先頭バイトを探す
            let line = buffer.line(self.position.line);
            let prev = prev_char_boundary(line, self.position.column);
            self.position.column = prev;
        } else if self.position.line > 0 {
            self.position.line -= 1;
            self.position.column = buffer.line_len(self.position.line);
        }
    }

    pub fn move_right(&mut self, buffer: &TextBuffer, extend_selection: bool) {
        self.begin_move(extend_selection);
        self.preferred_column = None;
        let line_len = buffer.line_len(self.position.line);
        if self.position.column < line_len {
            let line = buffer.line(self.position.line);
            let next = next_char_boundary(line, self.position.column);
            self.position.column = next;
        } else if self.position.line < buffer.line_count() - 1 {
            self.position.line += 1;
            self.position.column = 0;
        }
    }

    pub fn move_up(&mut self, buffer: &TextBuffer, extend_selection: bool) {
        self.begin_move(extend_selection);
        if self.position.line > 0 {
            let target_col = self.preferred_column.unwrap_or(self.position.column);
            self.preferred_column = Some(target_col);
            self.position.line -= 1;
            self.position.column = target_col.min(buffer.line_len(self.position.line));
            // 文字境界に合わせる
            self.position.column =
                snap_to_char_boundary(buffer.line(self.position.line), self.position.column);
        }
    }

    pub fn move_down(&mut self, buffer: &TextBuffer, extend_selection: bool) {
        self.begin_move(extend_selection);
        if self.position.line < buffer.line_count() - 1 {
            let target_col = self.preferred_column.unwrap_or(self.position.column);
            self.preferred_column = Some(target_col);
            self.position.line += 1;
            self.position.column = target_col.min(buffer.line_len(self.position.line));
            self.position.column =
                snap_to_char_boundary(buffer.line(self.position.line), self.position.column);
        }
    }

    pub fn move_to_line_start(&mut self, extend_selection: bool) {
        self.begin_move(extend_selection);
        self.preferred_column = None;
        self.position.column = 0;
    }

    pub fn move_to_line_end(&mut self, buffer: &TextBuffer, extend_selection: bool) {
        self.begin_move(extend_selection);
        self.preferred_column = None;
        self.position.column = buffer.line_len(self.position.line);
    }

    pub fn move_word_left(&mut self, buffer: &TextBuffer, extend_selection: bool) {
        self.begin_move(extend_selection);
        self.preferred_column = None;
        if self.position.column == 0 {
            if self.position.line > 0 {
                self.position.line -= 1;
                self.position.column = buffer.line_len(self.position.line);
            }
            return;
        }
        let line = buffer.line(self.position.line);
        let col = self.position.column;
        // スペースをスキップ
        let mut i = col;
        while i > 0 {
            let prev = prev_char_boundary(line, i);
            let ch = line[prev..i].chars().next().unwrap_or(' ');
            if !ch.is_whitespace() {
                break;
            }
            i = prev;
        }
        // 単語をスキップ
        while i > 0 {
            let prev = prev_char_boundary(line, i);
            let ch = line[prev..i].chars().next().unwrap_or(' ');
            if ch.is_whitespace() || is_word_separator(ch) {
                break;
            }
            i = prev;
        }
        self.position.column = i;
    }

    pub fn move_word_right(&mut self, buffer: &TextBuffer, extend_selection: bool) {
        self.begin_move(extend_selection);
        self.preferred_column = None;
        let line_len = buffer.line_len(self.position.line);
        if self.position.column >= line_len {
            if self.position.line < buffer.line_count() - 1 {
                self.position.line += 1;
                self.position.column = 0;
            }
            return;
        }
        let line = buffer.line(self.position.line);
        let mut i = self.position.column;
        // 単語をスキップ
        while i < line.len() {
            let next = next_char_boundary(line, i);
            let ch = line[i..next].chars().next().unwrap_or(' ');
            if ch.is_whitespace() || is_word_separator(ch) {
                break;
            }
            i = next;
        }
        // スペースをスキップ
        while i < line.len() {
            let next = next_char_boundary(line, i);
            let ch = line[i..next].chars().next().unwrap_or(' ');
            if !ch.is_whitespace() {
                break;
            }
            i = next;
        }
        self.position.column = i;
    }

    /// 選択範囲を削除してカーソル位置を返す
    pub fn delete_selection(&mut self, buffer: &mut TextBuffer) -> Option<Position> {
        if let Some((start, end)) = self.selection_range() {
            let pos = buffer.delete_range(start, end);
            self.position = pos;
            self.clear_selection();
            Some(pos)
        } else {
            None
        }
    }
}

// --- ユーティリティ ---

fn is_word_separator(ch: char) -> bool {
    matches!(
        ch,
        '(' | ')'
            | '{'
            | '}'
            | '['
            | ']'
            | '<'
            | '>'
            | '.'
            | ','
            | ';'
            | ':'
            | '"'
            | '\''
            | '='
            | '+'
            | '-'
            | '*'
            | '/'
            | '\\'
            | '|'
            | '&'
            | '!'
            | '?'
            | '@'
            | '#'
            | '$'
            | '%'
            | '^'
            | '~'
            | '`'
    )
}

/// 前の文字境界
fn prev_char_boundary(s: &str, pos: usize) -> usize {
    let mut i = pos.min(s.len());
    if i == 0 {
        return 0;
    }
    i -= 1;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// 次の文字境界
fn next_char_boundary(s: &str, pos: usize) -> usize {
    let mut i = pos;
    if i >= s.len() {
        return s.len();
    }
    i += 1;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// 指定バイト位置を最も近い文字境界にスナップ
fn snap_to_char_boundary(s: &str, pos: usize) -> usize {
    let p = pos.min(s.len());
    if s.is_char_boundary(p) {
        return p;
    }
    // 後方に探索
    let mut i = p;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}
