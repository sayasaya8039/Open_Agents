// エディタビュー — 描画・入力ハンドリング・IME対応

pub mod actions;
pub mod buffer;
pub mod cursor;
pub mod grid_renderer;
mod syntax_highlight;

use buffer::{Position, TextBuffer};
use cursor::CursorState;
use grid_renderer::{GridCell, GridRenderer};
use syntax_highlight::{highlight_buffer, SyntaxColorRole, SyntaxSpan};

use gpui::prelude::*;
use gpui::*;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use crate::i18n;
use crate::model_prefs::{AppearancePrefs, UiTheme};
use crate::{hex, hex_a, BG, TEXT_DIM, TEXT_PRIMARY, TEXT_SECONDARY};

const SYNTAX_COMMENT: u32 = 0x5c6370;
const SYNTAX_KEYWORD: u32 = 0xc678dd;
const SYNTAX_STRING: u32 = 0x98c379;
const SYNTAX_NUMBER: u32 = 0xd19a66;
const SYNTAX_TYPE: u32 = 0xe5c07b;
const SYNTAX_FUNCTION: u32 = 0x61afef;
const SYNTAX_PROPERTY: u32 = 0x56b6c2;
const SYNTAX_MACRO: u32 = 0xe06c75;
const SYNTAX_HEADING: u32 = 0x61afef;
const SYNTAX_ACCENT: u32 = 0xc678dd;

/// エディタビュー本体
pub struct EditorView {
    pub buffer: TextBuffer,
    highlighted_lines: Vec<Vec<SyntaxSpan>>,
    pub cursor: CursorState,
    focus_handle: FocusHandle,
    scroll_handle: UniformListScrollHandle,
    /// IME 変換中テキスト
    ime_text: Option<String>,
    ime_range: Option<Range<usize>>,
    /// マウスドラッグ中フラグ
    dragging: bool,
    /// エディタ content 領域の bounds（マウス座標変換用）
    editor_bounds: Bounds<Pixels>,
    font_size_px: i32,
    show_line_numbers: bool,
    ui_theme: UiTheme,
    /// GPU グリッドレンダラ（オプション — フォールバックは uniform_list）
    grid_renderer: Option<GridRenderer>,
    /// グリッドレンダラを使用するかどうか
    use_grid_renderer: bool,
}

impl EditorView {
    pub fn new(cx: &mut Context<Self>, appearance: &AppearancePrefs) -> Self {
        let focus_handle = cx.focus_handle();
        let buffer = TextBuffer::from_string(
            i18n::editor_welcome(),
        );
        let highlighted_lines = highlight_buffer(buffer.file_path(), buffer.lines());
        let ap = appearance.clone().sanitize();
        Self {
            buffer,
            highlighted_lines,
            cursor: CursorState::new(),
            focus_handle,
            scroll_handle: UniformListScrollHandle::new(),
            ime_text: None,
            ime_range: None,
            dragging: false,
            editor_bounds: Bounds::default(),
            font_size_px: ap.font_size_px,
            show_line_numbers: ap.show_line_numbers,
            ui_theme: ap.theme,
            grid_renderer: None,
            // グリッドは VGA 8×16 の ASCII のみ。日本語・記号は欠けて表示が壊れるため既定はベクタテキスト経路。
            use_grid_renderer: false,
        }
    }

    pub fn apply_appearance(&mut self, appearance: &AppearancePrefs, cx: &mut Context<Self>) {
        let ap = appearance.clone().sanitize();
        self.font_size_px = ap.font_size_px;
        self.show_line_numbers = ap.show_line_numbers;
        self.ui_theme = ap.theme;
        cx.notify();
    }

    fn is_editor_light(&self) -> bool {
        matches!(self.ui_theme, UiTheme::Light)
    }

    fn line_height_px(&self) -> Pixels {
        px((self.font_size_px as f32 * 20.0 / 13.0).max(16.0))
    }

    fn gutter_width_px(&self) -> Pixels {
        if self.show_line_numbers {
            px(48.)
        } else {
            px(0.)
        }
    }

    fn editor_bg(&self) -> Hsla {
        if self.is_editor_light() {
            hex(0xf3f3f3)
        } else {
            hex(BG)
        }
    }

    fn plain_text_color(&self) -> Hsla {
        if self.is_editor_light() {
            hex(0x1e1e1e)
        } else {
            hex(TEXT_PRIMARY)
        }
    }

    fn current_line_bg(&self) -> Hsla {
        if self.is_editor_light() {
            hex_a(0x000000, 0.06)
        } else {
            hex_a(0xffffff, 0.04)
        }
    }

    fn selection_block_colors(&self) -> (Hsla, Hsla) {
        if self.is_editor_light() {
            (hex_a(0x0066cc, 0.22), hex(0x111111))
        } else {
            (hex_a(0x264f78, 0.7), hex(0xffffff))
        }
    }

    fn gutter_label_color(&self, is_current: bool) -> Hsla {
        if self.is_editor_light() {
            if is_current {
                hex(0x555555)
            } else {
                hex(0x9a9a9a)
            }
        } else if is_current {
            hex(TEXT_SECONDARY)
        } else {
            hex(TEXT_DIM)
        }
    }

    /// ファイルからロード
    pub fn load_file(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        self.reset_list_scroll_to_top();
        match TextBuffer::from_file(path) {
            Ok(buf) => {
                self.buffer = buf;
                self.refresh_highlights();
                self.cursor = CursorState::new();
                cx.notify();
            }
            Err(_) => {
                // 不正 UTF-8 などはロスレスに読めないので lossy で表示
                match std::fs::read(path) {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes);
                        self.buffer = TextBuffer::from_string_with_path(
                            text.as_ref(),
                            Some(path.to_path_buf()),
                        );
                        self.refresh_highlights();
                        self.cursor = CursorState::new();
                        cx.notify();
                    }
                    Err(e) => {
                        eprintln!("ファイル読み込みエラー: {}", e);
                    }
                }
            }
        }
    }

    pub fn focus_editor(&self, window: &mut Window) {
        self.focus_handle.focus(window);
    }

    /// ワークスペース相対パスを開く。実ファイルがあれば読み込み、なければプレースホルダーを表示
    pub fn open_project_path(
        &mut self,
        workspace_root: &std::path::Path,
        segments: &[String],
        cx: &mut Context<Self>,
    ) {
        let path: std::path::PathBuf = segments
            .iter()
            .fold(workspace_root.to_path_buf(), |a, s| a.join(s));
        let path = path.canonicalize().unwrap_or(path);
        if path.is_file() {
            self.load_file(&path, cx);
            return;
        }
        self.reset_list_scroll_to_top();
        let stub = format!(
            "// {}\n// （プレースホルダー: ディスク上にファイルがないか読み込めませんでした）\n\n",
            path.display()
        );
        self.buffer = TextBuffer::from_string_with_path(&stub, Some(path));
        self.refresh_highlights();
        self.cursor = CursorState::new();
        cx.notify();
    }

    /// Chat 用のフォルダ（開いているファイルがあればその親、プレースホルダーは対象パスの親、なければワークスペースルート）
    pub fn chat_working_directory(&self, workspace_root: &std::path::Path) -> PathBuf {
        let root = workspace_root
            .canonicalize()
            .unwrap_or_else(|_| workspace_root.to_path_buf());
        let Some(path) = self.buffer.file_path() else {
            return root;
        };
        if path.is_file() {
            return path.parent().map(|p| p.to_path_buf()).unwrap_or(root);
        }
        if path.is_dir() {
            return path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        }
        path.parent()
            .map(|p| p.to_path_buf())
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(root)
    }

    /// カーソル行が画面外に出た場合のみスクロール（非strict: 既に見えていれば何もしない）
    fn ensure_cursor_visible(&self) {
        self.scroll_handle
            .scroll_to_item(self.cursor.position.line, gpui::ScrollStrategy::Top);
    }

    /// バッファ差し替え後に古いスクロール位置が残ると、可視行範囲が空になりテキストが一切描画されない。
    fn reset_list_scroll_to_top(&self) {
        let mut st = self.scroll_handle.0.borrow_mut();
        st.base_handle.set_offset(point(px(0.), px(0.)));
        st.deferred_scroll_to_item = None;
    }

    fn refresh_highlights(&mut self) {
        self.highlighted_lines = highlight_buffer(self.buffer.file_path(), self.buffer.lines());
    }

    /// SyntaxColorRole → RGB u32 (grid renderer 用)
    fn color_for_role(role: SyntaxColorRole, default_fg: u32) -> u32 {
        match role {
            SyntaxColorRole::Plain => default_fg,
            SyntaxColorRole::Comment => SYNTAX_COMMENT,
            SyntaxColorRole::Keyword => SYNTAX_KEYWORD,
            SyntaxColorRole::String => SYNTAX_STRING,
            SyntaxColorRole::Number => SYNTAX_NUMBER,
            SyntaxColorRole::Type => SYNTAX_TYPE,
            SyntaxColorRole::Function => SYNTAX_FUNCTION,
            SyntaxColorRole::Property => SYNTAX_PROPERTY,
            SyntaxColorRole::Macro => SYNTAX_MACRO,
            SyntaxColorRole::Heading => SYNTAX_HEADING,
            SyntaxColorRole::Accent => SYNTAX_ACCENT,
        }
    }

    /// バッファ内容を GridCell の2D配列に変換（syntax highlight 色付き）
    fn buffer_to_grid_cells(&self, cols: u32, rows: u32, scroll_y: usize) -> Vec<Vec<GridCell>> {
        let default_fg = if self.is_editor_light() {
            0x1E1E1Eu32
        } else {
            0xE5E5E5u32
        };
        let default_bg = if self.is_editor_light() {
            0xF3F3F3u32
        } else {
            0x1E1E1Eu32
        };
        let line_count = self.buffer.line_count();

        (0..rows as usize)
            .map(|screen_row| {
                let line_idx = scroll_y + screen_row;
                let mut row_cells = vec![GridCell::default(); cols as usize];

                // デフォルト bg を設定
                for cell in row_cells.iter_mut() {
                    cell.bg = default_bg;
                    cell.fg = default_fg;
                }

                if line_idx < line_count {
                    let spans = self.line_spans(line_idx);
                    let mut col = 0usize;
                    for span in &spans {
                        let fg = Self::color_for_role(span.role, default_fg);
                        for ch in span.text.chars() {
                            if col >= cols as usize {
                                break;
                            }
                            row_cells[col] = GridCell {
                                ch,
                                fg,
                                bg: default_bg,
                                flags: 0,
                            };
                            col += 1;
                        }
                    }
                }

                row_cells
            })
            .collect()
    }

    fn line_spans(&self, line_idx: usize) -> Vec<SyntaxSpan> {
        self.highlighted_lines
            .get(line_idx)
            .cloned()
            .unwrap_or_else(|| {
                vec![SyntaxSpan {
                    text: self.buffer.line(line_idx).to_string(),
                    role: SyntaxColorRole::Plain,
                }]
            })
    }

    fn role_color(&self, role: SyntaxColorRole) -> Hsla {
        match role {
            SyntaxColorRole::Plain => self.plain_text_color(),
            SyntaxColorRole::Comment => hex(SYNTAX_COMMENT),
            SyntaxColorRole::Keyword => hex(SYNTAX_KEYWORD),
            SyntaxColorRole::String => hex(SYNTAX_STRING),
            SyntaxColorRole::Number => hex(SYNTAX_NUMBER),
            SyntaxColorRole::Type => hex(SYNTAX_TYPE),
            SyntaxColorRole::Function => hex(SYNTAX_FUNCTION),
            SyntaxColorRole::Property => hex(SYNTAX_PROPERTY),
            SyntaxColorRole::Macro => hex(SYNTAX_MACRO),
            SyntaxColorRole::Heading => hex(SYNTAX_HEADING),
            SyntaxColorRole::Accent => hex(SYNTAX_ACCENT),
        }
    }

    fn slice_spans(spans: &[SyntaxSpan], start: usize, end: usize) -> Vec<SyntaxSpan> {
        if start >= end {
            return Vec::new();
        }

        let mut result = Vec::new();
        let mut offset = 0;
        for span in spans {
            let span_start = offset;
            let span_end = span_start + span.text.len();
            offset = span_end;

            if span_end <= start {
                continue;
            }
            if span_start >= end {
                break;
            }

            let local_start = start.saturating_sub(span_start).min(span.text.len());
            let local_end = end.min(span_end) - span_start;
            if local_start >= local_end {
                continue;
            }

            result.push(SyntaxSpan {
                text: span.text[local_start..local_end].to_string(),
                role: span.role,
            });
        }

        result
    }

    fn render_spans(
        editor: &EditorView,
        spans: &[SyntaxSpan],
        override_color: Option<Hsla>,
    ) -> AnyElement {
        let fs = px(editor.font_size_px as f32);
        div()
            .flex()
            .children(
                spans
                    .iter()
                    .filter(|span| !span.text.is_empty())
                    .map(|span| {
                        div()
                            .text_size(fs)
                            .font_family("Cascadia Code")
                            .text_color(
                                override_color.unwrap_or_else(|| editor.role_color(span.role)),
                            )
                            .child(span.text.clone())
                            .into_any_element()
                    }),
            )
            .into_any_element()
    }

    // --- 行描画 ---

    fn render_line(editor: &EditorView, line_idx: usize) -> impl IntoElement {
        let is_current = editor.cursor.position.line == line_idx;
        let line_text = editor.buffer.line(line_idx).to_string();
        let line_num = format!("{:>4}", line_idx + 1);
        let spans = editor.line_spans(line_idx);
        let lh = editor.line_height_px();
        let fs = px(editor.font_size_px as f32);
        let cursor_h = px(editor.font_size_px as f32 * (18.0 / 13.0));
        let (sel_bg, sel_fg) = editor.selection_block_colors();

        // カーソルが現在行にある場合、カーソル位置で行を分割して描画
        let text_element: AnyElement = if is_current && !editor.cursor.has_selection() {
            let col = (0..=line_text.len())
                .rev()
                .find(|&i| i <= editor.cursor.position.column && line_text.is_char_boundary(i))
                .unwrap_or(0);
            let before = line_text[..col].to_string();
            let after = line_text[col..].to_string();
            div()
                .flex_1()
                .flex()
                .child(Self::render_spans(
                    editor,
                    &Self::slice_spans(&spans, 0, before.len()),
                    None,
                ))
                .child(
                    div()
                        .w(px(2.))
                        .h(cursor_h)
                        .bg(hex(0x569cd6))
                        .flex_shrink_0(),
                )
                .child(Self::render_spans(
                    editor,
                    &Self::slice_spans(&spans, line_text.len() - after.len(), line_text.len()),
                    None,
                ))
                .into_any_element()
        } else if editor.cursor.has_selection() {
            let (sel_start, sel_end) = editor.cursor.selection_range().unwrap();
            let line_len = line_text.len();
            let line_in_selection = line_idx >= sel_start.line && line_idx <= sel_end.line;

            if line_in_selection {
                let sel_col_start = if line_idx == sel_start.line {
                    sel_start.column.min(line_len)
                } else {
                    0
                };
                let sel_col_end = if line_idx == sel_end.line {
                    sel_end.column.min(line_len)
                } else {
                    line_len
                };

                let sc = (0..=line_len)
                    .rev()
                    .find(|&i| i <= sel_col_start && line_text.is_char_boundary(i))
                    .unwrap_or(0);
                let ec = (0..=line_len)
                    .rev()
                    .find(|&i| i <= sel_col_end && line_text.is_char_boundary(i))
                    .unwrap_or(0);

                let before_len = sc;
                div()
                    .flex_1()
                    .flex()
                    .child(Self::render_spans(
                        editor,
                        &Self::slice_spans(&spans, 0, before_len),
                        None,
                    ))
                    .child(div().bg(sel_bg).child(Self::render_spans(
                        editor,
                        &Self::slice_spans(&spans, sc, ec),
                        Some(sel_fg),
                    )))
                    .child(Self::render_spans(
                        editor,
                        &Self::slice_spans(&spans, ec, line_text.len()),
                        None,
                    ))
                    .into_any_element()
            } else {
                div()
                    .flex_1()
                    .child(Self::render_spans(editor, &spans, None))
                    .into_any_element()
            }
        } else {
            div()
                .flex_1()
                .child(Self::render_spans(editor, &spans, None))
                .into_any_element()
        };

        // ガターは行高いっぱいに取り、行番号だけ縦中央（本文行とのズレ防止）
        let gutter = div()
            .w(px(48.))
            .h(lh)
            .flex()
            .items_center()
            .justify_end()
            .pr(px(16.))
            .text_color(editor.gutter_label_color(is_current))
            .text_size(fs)
            .font_family("Cascadia Code")
            .text_align(TextAlign::Right)
            .child(line_num);

        div()
            .h(lh)
            .flex()
            .items_center()
            .when(is_current, |d| d.bg(editor.current_line_bg()))
            .when(editor.show_line_numbers, |d| d.child(gutter))
            .child(text_element)
    }

    // --- アクションハンドラ ---

    fn handle_move_up(
        &mut self,
        _action: &actions::MoveUp,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cursor.move_up(&self.buffer, false);
        self.ensure_cursor_visible();
        cx.notify();
    }

    fn handle_move_down(
        &mut self,
        _action: &actions::MoveDown,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cursor.move_down(&self.buffer, false);
        self.ensure_cursor_visible();
        cx.notify();
    }

    fn handle_move_left(
        &mut self,
        _action: &actions::MoveLeft,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cursor.move_left(&self.buffer, false);
        self.ensure_cursor_visible();
        cx.notify();
    }

    fn handle_move_right(
        &mut self,
        _action: &actions::MoveRight,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cursor.move_right(&self.buffer, false);
        self.ensure_cursor_visible();
        cx.notify();
    }

    fn handle_move_to_line_start(
        &mut self,
        _action: &actions::MoveToLineStart,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cursor.move_to_line_start(false);
        self.ensure_cursor_visible();
        cx.notify();
    }

    fn handle_move_to_line_end(
        &mut self,
        _action: &actions::MoveToLineEnd,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cursor.move_to_line_end(&self.buffer, false);
        self.ensure_cursor_visible();
        cx.notify();
    }

    fn handle_move_word_left(
        &mut self,
        _action: &actions::MoveWordLeft,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cursor.move_word_left(&self.buffer, false);
        self.ensure_cursor_visible();
        cx.notify();
    }

    fn handle_move_word_right(
        &mut self,
        _action: &actions::MoveWordRight,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cursor.move_word_right(&self.buffer, false);
        self.ensure_cursor_visible();
        cx.notify();
    }

    // --- 選択ハンドラ（Shift+矢印） ---

    fn handle_select_up(
        &mut self,
        _action: &actions::SelectUp,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cursor.move_up(&self.buffer, true);
        self.ensure_cursor_visible();
        cx.notify();
    }

    fn handle_select_down(
        &mut self,
        _action: &actions::SelectDown,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cursor.move_down(&self.buffer, true);
        self.ensure_cursor_visible();
        cx.notify();
    }

    fn handle_select_left(
        &mut self,
        _action: &actions::SelectLeft,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cursor.move_left(&self.buffer, true);
        self.ensure_cursor_visible();
        cx.notify();
    }

    fn handle_select_right(
        &mut self,
        _action: &actions::SelectRight,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cursor.move_right(&self.buffer, true);
        self.ensure_cursor_visible();
        cx.notify();
    }

    fn handle_select_to_line_start(
        &mut self,
        _action: &actions::SelectToLineStart,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cursor.move_to_line_start(true);
        self.ensure_cursor_visible();
        cx.notify();
    }

    fn handle_select_to_line_end(
        &mut self,
        _action: &actions::SelectToLineEnd,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cursor.move_to_line_end(&self.buffer, true);
        self.ensure_cursor_visible();
        cx.notify();
    }

    fn handle_select_word_left(
        &mut self,
        _action: &actions::SelectWordLeft,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cursor.move_word_left(&self.buffer, true);
        self.ensure_cursor_visible();
        cx.notify();
    }

    fn handle_select_word_right(
        &mut self,
        _action: &actions::SelectWordRight,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cursor.move_word_right(&self.buffer, true);
        self.ensure_cursor_visible();
        cx.notify();
    }

    fn handle_backspace(
        &mut self,
        _action: &actions::Backspace,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 選択範囲があれば削除
        if self.cursor.delete_selection(&mut self.buffer).is_some() {
            self.refresh_highlights();
            self.ensure_cursor_visible();
            cx.notify();
            return;
        }
        let pos = self.cursor.position;
        if pos.column > 0 {
            // 前の文字を削除
            let line = self.buffer.line(pos.line);
            let mut prev_col = pos.column - 1;
            while prev_col > 0 && !line.is_char_boundary(prev_col) {
                prev_col -= 1;
            }
            let start = Position::new(pos.line, prev_col);
            self.buffer.delete_range(start, pos);
            self.cursor.position = start;
        } else if pos.line > 0 {
            // 行頭 → 前の行末に結合
            let prev_line_len = self.buffer.line_len(pos.line - 1);
            let start = Position::new(pos.line - 1, prev_line_len);
            self.buffer.delete_range(start, pos);
            self.cursor.position = start;
        }
        self.refresh_highlights();
        self.cursor.preferred_column = None;
        self.ensure_cursor_visible();
        cx.notify();
    }

    fn handle_delete(
        &mut self,
        _action: &actions::Delete,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.cursor.delete_selection(&mut self.buffer).is_some() {
            self.refresh_highlights();
            self.ensure_cursor_visible();
            cx.notify();
            return;
        }
        let pos = self.cursor.position;
        let line_len = self.buffer.line_len(pos.line);
        if pos.column < line_len {
            let line = self.buffer.line(pos.line);
            let mut next_col = pos.column + 1;
            while next_col < line.len() && !line.is_char_boundary(next_col) {
                next_col += 1;
            }
            let end = Position::new(pos.line, next_col);
            self.buffer.delete_range(pos, end);
        } else if pos.line < self.buffer.line_count() - 1 {
            let end = Position::new(pos.line + 1, 0);
            self.buffer.delete_range(pos, end);
        }
        self.refresh_highlights();
        self.ensure_cursor_visible();
        cx.notify();
    }

    fn handle_enter(
        &mut self,
        _action: &actions::Enter,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cursor.delete_selection(&mut self.buffer);
        let new_pos = self.buffer.insert_char(self.cursor.position, '\n');
        self.cursor.position = new_pos;
        self.refresh_highlights();
        self.cursor.preferred_column = None;
        self.ensure_cursor_visible();
        cx.notify();
    }

    fn handle_tab(&mut self, _action: &actions::Tab, _window: &mut Window, cx: &mut Context<Self>) {
        self.cursor.delete_selection(&mut self.buffer);
        let new_pos = self.buffer.insert_text(self.cursor.position, "    ");
        self.cursor.position = new_pos;
        self.refresh_highlights();
        self.cursor.preferred_column = None;
        cx.notify();
    }

    /// ツールバーの保存ボタンと Ctrl+S の共通処理  
    /// `save_dialog_dir`: 無題のとき「名前を付けて保存」ダイアログの開始ディレクトリ（`None` はカレント）
    pub fn perform_save(&mut self, cx: &mut Context<Self>, save_dialog_dir: Option<PathBuf>) {
        if self.buffer.file_path().is_some() {
            if let Err(e) = self.buffer.save() {
                eprintln!("保存エラー: {}", e);
            }
            cx.notify();
        } else {
            // 保存先未設定 → prompt_for_new_path
            let start = save_dialog_dir.unwrap_or_else(|| PathBuf::from("."));
            let receiver = cx.prompt_for_new_path(&start, None);
            cx.spawn(async move |entity: WeakEntity<Self>, cx: &mut AsyncApp| {
                if let Ok(Ok(Some(path))) = receiver.await {
                    cx.update(|cx| {
                        entity
                            .update(cx, |this: &mut Self, cx| {
                                if let Err(e) = this.buffer.save_as(&path) {
                                    eprintln!("保存エラー: {}", e);
                                }
                                cx.notify();
                            })
                            .ok();
                    })
                    .ok();
                }
            })
            .detach();
        }
    }

    fn handle_save(
        &mut self,
        _action: &actions::Save,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.perform_save(cx, None);
    }

    fn handle_open(
        &mut self,
        _action: &actions::Open,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |entity: WeakEntity<Self>, cx: &mut AsyncApp| {
            if let Ok(Ok(Some(paths))) = receiver.await {
                if let Some(path) = paths.first() {
                    let path = path.clone();
                    cx.update(|cx| {
                        entity
                            .update(cx, |this: &mut Self, cx| {
                                this.load_file(&path, cx);
                            })
                            .ok();
                    })
                    .ok();
                }
            }
        })
        .detach();
    }

    // --- マウス操作 ---

    /// 行テキストのカラム幅を測定（描画と同じフォント・サイズ）
    fn editor_text_runs(&self, byte_len: usize) -> [TextRun; 1] {
        [TextRun {
            len: byte_len,
            font: font("Cascadia Code"),
            color: self.plain_text_color(),
            background_color: None,
            underline: None,
            strikethrough: None,
        }]
    }

    /// ピクセル座標からバッファ位置を計算
    /// point はウィンドウ座標。`editor_bounds` は editor-content のレイアウト境界（フルサイズ canvas で更新）
    fn position_from_point(&self, point: Point<Pixels>, window: &Window) -> Position {
        let line_height = self.line_height_px();
        let gutter_width = self.gutter_width_px();

        let local_x = point.x - self.editor_bounds.origin.x;
        let local_y = point.y - self.editor_bounds.origin.y;

        let scroll_offset = self.scroll_handle.0.borrow().base_handle.offset();
        let scrolled_y = local_y - scroll_offset.y;

        let line = if scrolled_y < px(0.) {
            0
        } else {
            ((scrolled_y / line_height) as usize).min(self.buffer.line_count().saturating_sub(1))
        };

        let line_text: SharedString = self.buffer.line(line).to_string().into();
        let rel_x = (local_x - gutter_width).max(px(0.));
        let runs = self.editor_text_runs(line_text.len());
        let shaped =
            window
                .text_system()
                .shape_line(line_text, px(self.font_size_px as f32), &runs, None);
        let mut col = shaped
            .closest_index_for_x(rel_x)
            .min(self.buffer.line_len(line));

        let line_str = self.buffer.line(line);
        while col > 0 && !line_str.is_char_boundary(col) {
            col -= 1;
        }
        Position::new(line, col)
    }

    /// マウスクリック — カーソル移動
    fn handle_mouse_down(
        &mut self,
        ev: &MouseDownEvent,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        if ev.button == MouseButton::Left {
            self.focus_handle.focus(window);
            let pos = self.position_from_point(ev.position, window);
            self.cursor.anchor = None;
            self.cursor.position = pos;
            self.cursor.preferred_column = None;
            self.dragging = true;
            _cx.notify();
        }
    }

    /// マウスドラッグ — 選択範囲拡張
    fn handle_mouse_move(
        &mut self,
        ev: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dragging && ev.pressed_button == Some(MouseButton::Left) {
            let pos = self.position_from_point(ev.position, window);
            if self.cursor.anchor.is_none() {
                self.cursor.anchor = Some(self.cursor.position);
            }
            self.cursor.position = pos;
            cx.notify();
        }
    }

    /// マウスアップ — ドラッグ終了
    fn handle_mouse_up(&mut self, ev: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if ev.button == MouseButton::Left {
            self.dragging = false;
            // anchor と position が同じなら選択解除
            if let Some(anchor) = self.cursor.anchor {
                if anchor == self.cursor.position {
                    self.cursor.anchor = None;
                }
            }
            cx.notify();
        }
    }

    // --- クリップボード ---

    fn handle_copy(
        &mut self,
        _action: &actions::Copy,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((start, end)) = self.cursor.selection_range() {
            let text = self.buffer.text_in_range(start, end);
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn handle_cut(&mut self, _action: &actions::Cut, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some((start, end)) = self.cursor.selection_range() {
            let text = self.buffer.text_in_range(start, end);
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            self.cursor.delete_selection(&mut self.buffer);
            self.refresh_highlights();
            self.ensure_cursor_visible();
            cx.notify();
        }
    }

    fn handle_paste(
        &mut self,
        _action: &actions::Paste,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(item) = cx.read_from_clipboard() {
            if let Some(text) = item.text() {
                if !text.is_empty() {
                    self.cursor.delete_selection(&mut self.buffer);
                    let new_pos = self.buffer.insert_text(self.cursor.position, &text);
                    self.cursor.position = new_pos;
                    self.refresh_highlights();
                    self.cursor.preferred_column = None;
                    self.ensure_cursor_visible();
                    cx.notify();
                }
            }
        }
    }

    fn handle_select_all(
        &mut self,
        _action: &actions::SelectAll,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cursor.anchor = Some(Position::new(0, 0));
        let last_line = self.buffer.line_count().saturating_sub(1);
        self.cursor.position = Position::new(last_line, self.buffer.line_len(last_line));
        cx.notify();
    }

    /// ファイル名を取得（タブ表示用）
    pub fn tab_title(&self) -> String {
        let name = self.buffer.file_name();
        if self.buffer.is_dirty() {
            format!("● {}", name)
        } else {
            name
        }
    }

    /// エディタバッファのテキスト内容を返す
    pub fn text_content(&self) -> String {
        self.buffer.lines().join("\n")
    }
}

// ============================================================
// Render 実装
// ============================================================

impl Render for EditorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity().clone();
        let focus = self.focus_handle.clone();
        let line_count = self.buffer.line_count();
        let scroll_handle = self.scroll_handle.clone();

        // グリッドレンダラパス: render tree 構築前に画像を生成
        let grid_image: Option<Arc<RenderImage>> = if self.use_grid_renderer {
            let cols = 120u32;
            let rows = 40u32;
            if self.grid_renderer.is_none() {
                self.grid_renderer = Some(GridRenderer::new(cols, rows));
            }
            let scroll_y = 0usize; // TODO: scroll_handle から取得
            let cells = self.buffer_to_grid_cells(cols, rows, scroll_y);
            if let Some(ref mut gr) = self.grid_renderer {
                gr.update_cells(&cells);
                Some(gr.render_image())
            } else {
                None
            }
        } else {
            None
        };

        // InputHandler 登録用の canvas
        let input_entity = entity.clone();
        let input_focus = focus.clone();

        div()
            .key_context("Editor")
            .track_focus(&focus)
            .min_h(px(0.))
            .on_action(cx.listener(Self::handle_move_up))
            .on_action(cx.listener(Self::handle_move_down))
            .on_action(cx.listener(Self::handle_move_left))
            .on_action(cx.listener(Self::handle_move_right))
            .on_action(cx.listener(Self::handle_move_to_line_start))
            .on_action(cx.listener(Self::handle_move_to_line_end))
            .on_action(cx.listener(Self::handle_move_word_left))
            .on_action(cx.listener(Self::handle_move_word_right))
            .on_action(cx.listener(Self::handle_select_up))
            .on_action(cx.listener(Self::handle_select_down))
            .on_action(cx.listener(Self::handle_select_left))
            .on_action(cx.listener(Self::handle_select_right))
            .on_action(cx.listener(Self::handle_select_to_line_start))
            .on_action(cx.listener(Self::handle_select_to_line_end))
            .on_action(cx.listener(Self::handle_select_word_left))
            .on_action(cx.listener(Self::handle_select_word_right))
            .on_action(cx.listener(Self::handle_backspace))
            .on_action(cx.listener(Self::handle_delete))
            .on_action(cx.listener(Self::handle_enter))
            .on_action(cx.listener(Self::handle_tab))
            .on_action(cx.listener(Self::handle_save))
            .on_action(cx.listener(Self::handle_open))
            .on_action(cx.listener(Self::handle_copy))
            .on_action(cx.listener(Self::handle_cut))
            .on_action(cx.listener(Self::handle_paste))
            .on_action(cx.listener(Self::handle_select_all))
            .flex_1()
            .flex()
            .flex_col()
            .relative()
            .bg(self.editor_bg())
            .font_family("Cascadia Code")
            // IME: 先に描画し、エディタ内容より手前にスタックされないようにする
            .child(
                canvas(
                    |bounds, _window, _cx| bounds,
                    move |_bounds, prepaint_bounds, window, cx| {
                        if input_focus.is_focused(window) {
                            let handler =
                                ElementInputHandler::new(prepaint_bounds, input_entity.clone());
                            window.handle_input(&input_focus, handler, cx);
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(
                // マウスイベント + bounds 記録は uniform_list を含む div に限定
                div()
                    .id("editor-content")
                    .flex_1()
                    .flex()
                    .flex_col()
                    .min_h(px(0.))
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::handle_mouse_down))
                    .on_mouse_move(cx.listener(Self::handle_mouse_move))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::handle_mouse_up))
                    // editor-content 全体の境界（1×1 canvas では誤った原寸になっていた）
                    .child({
                        let bounds_entity = entity.clone();
                        canvas(
                            |b, _, _| b,
                            move |layout_bounds, _, _, cx| {
                                bounds_entity.update(cx, |this, _| {
                                    this.editor_bounds = layout_bounds;
                                });
                            },
                        )
                        .absolute()
                        .size_full()
                    })
                    .child(if let Some(render_image) = grid_image {
                        // グリッドレンダラパス: CPU bitmap → img()
                        img(render_image)
                            .w_full()
                            .h_full()
                            .object_fit(ObjectFit::Fill)
                            .flex_1()
                            .into_any_element()
                    } else {
                        // フォールバック: 既存の uniform_list パス
                        uniform_list("editor-lines", line_count, {
                            let entity = entity.clone();
                            move |range: Range<usize>, _window: &mut Window, cx: &mut App| {
                                let editor = entity.read(cx);
                                range.map(|ix| Self::render_line(editor, ix)).collect()
                            }
                        })
                        .with_sizing_behavior(ListSizingBehavior::Infer)
                        .flex_1()
                        .min_h(px(0.))
                        .track_scroll(scroll_handle)
                        .into_any_element()
                    }),
            )
    }
}

// ============================================================
// EntityInputHandler 実装（IME 対応）
// ============================================================

impl EntityInputHandler for EditorView {
    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let offset = self.buffer.position_to_offset(self.cursor.position);
        if let Some(anchor) = self.cursor.anchor {
            let anchor_offset = self.buffer.position_to_offset(anchor);
            let (start, end) = if anchor_offset < offset {
                (anchor_offset, offset)
            } else {
                (offset, anchor_offset)
            };
            Some(UTF16Selection {
                range: start..end,
                reversed: anchor_offset > offset,
            })
        } else {
            Some(UTF16Selection {
                range: offset..offset,
                reversed: false,
            })
        }
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.ime_range.clone()
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        // IME キャンセル時: 変換中テキストをバッファから削除
        if let Some(ime_range) = self.ime_range.take() {
            let start = self.buffer.offset_to_position(ime_range.start);
            let end = self.buffer.offset_to_position(ime_range.end);
            if start != end {
                let pos = self.buffer.delete_range(start, end);
                self.cursor.position = pos;
                self.refresh_highlights();
                cx.notify();
            }
        }
        self.ime_text = None;
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // IME 確定 or 通常文字入力
        let range = range.unwrap_or_else(|| {
            let offset = self.buffer.position_to_offset(self.cursor.position);
            offset..offset
        });
        let start = self.buffer.offset_to_position(range.start);
        let end = self.buffer.offset_to_position(range.end);
        if start != end {
            self.buffer.delete_range(start, end);
        }
        let new_pos = self.buffer.insert_text(start, text);
        self.cursor.position = new_pos;
        self.cursor.clear_selection();
        self.refresh_highlights();
        self.ime_text = None;
        self.ime_range = None;
        self.ensure_cursor_visible();
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // IME 変換中
        let range = range.unwrap_or_else(|| {
            let offset = self.buffer.position_to_offset(self.cursor.position);
            offset..offset
        });
        let start = self.buffer.offset_to_position(range.start);
        let end = self.buffer.offset_to_position(range.end);
        if start != end {
            self.buffer.delete_range(start, end);
        }
        let new_pos = self.buffer.insert_text(start, new_text);
        self.cursor.position = new_pos;
        self.refresh_highlights();

        let mark_start = range.start;
        let mark_end = mark_start + new_text.encode_utf16().count();
        self.ime_text = Some(new_text.to_string());
        self.ime_range = Some(mark_start..mark_end);
        cx.notify();
    }

    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        *adjusted_range = Some(range.clone());
        Some(self.buffer.text_in_range_utf16(range))
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _element_bounds: Bounds<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let pos = self.buffer.offset_to_position(range_utf16.start);
        let line_height = self.line_height_px();
        let gutter = self.gutter_width_px();
        let line_text: SharedString = self.buffer.line(pos.line).to_string().into();
        let runs = self.editor_text_runs(line_text.len());
        let shaped =
            window
                .text_system()
                .shape_line(line_text, px(self.font_size_px as f32), &runs, None);
        let scroll_y = self.scroll_handle.0.borrow().base_handle.offset().y;
        let x = self.editor_bounds.origin.x + gutter + shaped.x_for_index(pos.column);
        let y = self.editor_bounds.origin.y + scroll_y + line_height * pos.line as f32;
        Some(Bounds {
            origin: point(x, y),
            size: size(px(2.), line_height),
        })
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let pos = self.position_from_point(point, window);
        Some(self.buffer.position_to_offset(pos))
    }
}
