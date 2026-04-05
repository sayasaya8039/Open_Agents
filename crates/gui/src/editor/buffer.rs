// テキストバッファ — 行ベースの Vec<String> 実装

use std::io;
use std::path::{Path, PathBuf};

/// バッファ内の位置
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Position {
    pub line: usize,   // 0-based
    pub column: usize, // 0-based, UTF-8 バイトオフセット
}

impl Position {
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

impl PartialOrd for Position {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Position {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.line.cmp(&other.line).then(self.column.cmp(&other.column))
    }
}

/// 行ベースのテキストバッファ
#[allow(dead_code)]
pub struct TextBuffer {
    /// 各行のテキスト（改行文字を含まない）
    lines: Vec<String>,
    /// ファイルパス（None = 無題）
    file_path: Option<PathBuf>,
    /// 変更フラグ
    dirty: bool,
}

#[allow(dead_code)]
impl TextBuffer {
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            file_path: None,
            dirty: false,
        }
    }

    /// ファイルから読み込み
    pub fn from_file(path: &Path) -> io::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let lines: Vec<String> = if content.is_empty() {
            vec![String::new()]
        } else {
            content.lines().map(String::from).collect()
        };
        // 末尾改行で空行を追加
        let lines = if content.ends_with('\n') && !lines.is_empty() {
            let mut l = lines;
            l.push(String::new());
            l
        } else if lines.is_empty() {
            vec![String::new()]
        } else {
            lines
        };
        Ok(Self {
            lines,
            file_path: Some(path.to_path_buf()),
            dirty: false,
        })
    }

    /// 文字列からバッファ生成
    pub fn from_string(text: &str) -> Self {
        let lines: Vec<String> = if text.is_empty() {
            vec![String::new()]
        } else {
            text.lines().map(String::from).collect()
        };
        let lines = if lines.is_empty() {
            vec![String::new()]
        } else {
            lines
        };
        Self {
            lines,
            file_path: None,
            dirty: false,
        }
    }

    /// ファイル保存
    pub fn save(&mut self) -> io::Result<()> {
        if let Some(path) = &self.file_path {
            let content = self.lines.join("\n");
            std::fs::write(path, &content)?;
            self.dirty = false;
            Ok(())
        } else {
            Err(io::Error::new(io::ErrorKind::NotFound, "ファイルパスが未設定"))
        }
    }

    /// 名前を付けて保存
    pub fn save_as(&mut self, path: &Path) -> io::Result<()> {
        self.file_path = Some(path.to_path_buf());
        self.save()
    }

    // --- アクセサ ---

    pub fn line(&self, idx: usize) -> &str {
        if idx < self.lines.len() {
            &self.lines[idx]
        } else {
            ""
        }
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line_len(&self, idx: usize) -> usize {
        if idx < self.lines.len() {
            self.lines[idx].len()
        } else {
            0
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn file_path(&self) -> Option<&Path> {
        self.file_path.as_deref()
    }

    pub fn file_name(&self) -> String {
        self.file_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "無題".to_string())
    }

    pub fn set_dirty(&mut self) {
        self.dirty = true;
    }

    // --- テキスト操作 ---

    /// 指定位置に1文字挿入し、新しいカーソル位置を返す
    pub fn insert_char(&mut self, pos: Position, ch: char) -> Position {
        self.dirty = true;
        if ch == '\n' {
            return self.insert_newline(pos);
        }
        let line = pos.line.min(self.lines.len().saturating_sub(1));
        let col = pos.column.min(self.lines[line].len());
        let mut s = String::new();
        s.push(ch);
        self.lines[line].insert_str(col, &s);
        Position::new(line, col + ch.len_utf8())
    }

    /// 指定位置にテキスト挿入し、新しいカーソル位置を返す
    pub fn insert_text(&mut self, pos: Position, text: &str) -> Position {
        if text.is_empty() {
            return pos;
        }
        self.dirty = true;
        let mut current = pos;
        for ch in text.chars() {
            current = self.insert_char(current, ch);
        }
        current
    }

    /// 改行挿入
    fn insert_newline(&mut self, pos: Position) -> Position {
        let line = pos.line.min(self.lines.len().saturating_sub(1));
        let col = pos.column.min(self.lines[line].len());
        let rest = self.lines[line][col..].to_string();
        self.lines[line].truncate(col);
        self.lines.insert(line + 1, rest);
        Position::new(line + 1, 0)
    }

    /// 範囲削除し、削除後のカーソル位置を返す
    pub fn delete_range(&mut self, start: Position, end: Position) -> Position {
        if start == end {
            return start;
        }
        self.dirty = true;
        let (start, end) = if start <= end { (start, end) } else { (end, start) };
        let start_line = start.line.min(self.lines.len().saturating_sub(1));
        let end_line = end.line.min(self.lines.len().saturating_sub(1));
        let start_col = start.column.min(self.lines[start_line].len());
        let end_col = end.column.min(self.lines[end_line].len());

        if start_line == end_line {
            // 同一行内の削除
            self.lines[start_line].replace_range(start_col..end_col, "");
        } else {
            // 複数行の削除
            let rest = self.lines[end_line][end_col..].to_string();
            self.lines[start_line].truncate(start_col);
            self.lines[start_line].push_str(&rest);
            // 中間行を削除
            self.lines.drain(start_line + 1..=end_line);
        }
        Position::new(start_line, start_col)
    }

    // --- UTF-16 変換（InputHandler 用） ---

    /// UTF-16 オフセットからバッファ位置へ変換
    pub fn offset_to_position(&self, offset_utf16: usize) -> Position {
        let mut remaining = offset_utf16;
        for (line_idx, line) in self.lines.iter().enumerate() {
            let line_utf16_len = line.encode_utf16().count();
            if remaining <= line_utf16_len {
                // この行内のオフセット
                let col = utf16_offset_to_utf8(line, remaining);
                return Position::new(line_idx, col);
            }
            // +1 は改行文字分（UTF-16 でも 1）
            remaining -= line_utf16_len + 1;
        }
        // バッファ末尾
        let last = self.lines.len().saturating_sub(1);
        Position::new(last, self.lines[last].len())
    }

    /// バッファ位置から UTF-16 オフセットへ変換
    pub fn position_to_offset(&self, pos: Position) -> usize {
        let mut offset = 0;
        let line = pos.line.min(self.lines.len().saturating_sub(1));
        for i in 0..line {
            offset += self.lines[i].encode_utf16().count() + 1; // +1 は改行
        }
        let col = pos.column.min(self.lines[line].len());
        offset += utf8_to_utf16_offset(&self.lines[line], col);
        offset
    }

    /// UTF-16 範囲のテキストを返す
    pub fn text_in_range_utf16(&self, range: std::ops::Range<usize>) -> String {
        let start = self.offset_to_position(range.start);
        let end = self.offset_to_position(range.end);
        self.text_in_range(start, end)
    }

    /// バッファ位置範囲のテキストを返す
    fn text_in_range(&self, start: Position, end: Position) -> String {
        let (start, end) = if start <= end { (start, end) } else { (end, start) };
        if start.line == end.line {
            let line = &self.lines[start.line];
            let s = start.column.min(line.len());
            let e = end.column.min(line.len());
            return line[s..e].to_string();
        }
        let mut result = String::new();
        // 最初の行
        let first = &self.lines[start.line];
        result.push_str(&first[start.column.min(first.len())..]);
        // 中間行
        for i in (start.line + 1)..end.line {
            result.push('\n');
            result.push_str(&self.lines[i]);
        }
        // 最後の行
        result.push('\n');
        let last = &self.lines[end.line];
        result.push_str(&last[..end.column.min(last.len())]);
        result
    }

    /// バッファ全体の UTF-16 長さ
    pub fn total_utf16_len(&self) -> usize {
        let mut len = 0;
        for (i, line) in self.lines.iter().enumerate() {
            len += line.encode_utf16().count();
            if i < self.lines.len() - 1 {
                len += 1; // 改行
            }
        }
        len
    }
}

/// UTF-16 オフセットを UTF-8 バイトオフセットに変換
fn utf16_offset_to_utf8(s: &str, utf16_offset: usize) -> usize {
    let mut utf16_count = 0;
    for (byte_idx, ch) in s.char_indices() {
        if utf16_count >= utf16_offset {
            return byte_idx;
        }
        utf16_count += ch.len_utf16();
    }
    s.len()
}

/// UTF-8 バイトオフセットを UTF-16 オフセットに変換
fn utf8_to_utf16_offset(s: &str, byte_offset: usize) -> usize {
    let safe = byte_offset.min(s.len());
    s[..safe].encode_utf16().count()
}
