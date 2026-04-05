// エディタビュー — 描画・入力ハンドリング・IME対応

pub mod actions;
pub mod buffer;
pub mod cursor;

use buffer::{Position, TextBuffer};
use cursor::CursorState;

use gpui::*;
use gpui::prelude::*;
use std::ops::Range;

use crate::{hex, hex_a, BG, TEXT_DIM, TEXT_PRIMARY, TEXT_SECONDARY};

/// エディタビュー本体
pub struct EditorView {
    pub buffer: TextBuffer,
    pub cursor: CursorState,
    focus_handle: FocusHandle,
    scroll_handle: UniformListScrollHandle,
    /// IME 変換中テキスト
    ime_text: Option<String>,
    ime_range: Option<Range<usize>>,
}

impl EditorView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        Self {
            buffer: TextBuffer::from_string(
                "// Open Agents エディタへようこそ！\n// ここにコードを書いてください。\n\nfn main() {\n    println!(\"Hello, world!\");\n}\n",
            ),
            cursor: CursorState::new(),
            focus_handle,
            scroll_handle: UniformListScrollHandle::new(),
            ime_text: None,
            ime_range: None,
        }
    }

    /// ファイルからロード
    pub fn load_file(&mut self, path: &std::path::Path, cx: &mut Context<Self>) {
        match TextBuffer::from_file(path) {
            Ok(buf) => {
                self.buffer = buf;
                self.cursor = CursorState::new();
                cx.notify();
            }
            Err(e) => {
                eprintln!("ファイル読み込みエラー: {}", e);
            }
        }
    }

    /// カーソル位置をスクロール範囲内に維持（非strict: 画面内なら何もしない）
    fn ensure_cursor_visible(&self) {
        self.scroll_handle.scroll_to_item(
            self.cursor.position.line,
            gpui::ScrollStrategy::Top,
        );
    }

    // --- 行描画 ---

    fn render_line(
        editor: &EditorView,
        line_idx: usize,
    ) -> impl IntoElement {
        let is_current = editor.cursor.position.line == line_idx;
        let line_text = editor.buffer.line(line_idx).to_string();
        let line_num = format!("{:>4}", line_idx + 1);

        // カーソルが現在行にある場合、カーソル位置で行を分割して描画
        let text_element: AnyElement = if is_current && !editor.cursor.has_selection() {
            // 多バイト文字の途中を指す場合に備えて char_boundary にスナップ
            let col = (0..=line_text.len())
                .rev()
                .find(|&i| i <= editor.cursor.position.column && line_text.is_char_boundary(i))
                .unwrap_or(0);
            let before = line_text[..col].to_string();
            let after = line_text[col..].to_string();
            div()
                .flex_1()
                .flex()
                .text_size(px(13.))
                .font_family("Cascadia Code")
                .text_color(hex(TEXT_PRIMARY))
                .child(before)
                // カーソル（縦線）
                .child(
                    div()
                        .w(px(2.))
                        .h(px(18.))
                        .bg(hex(0x569cd6))
                        .flex_shrink_0(),
                )
                .child(after)
                .into_any_element()
        } else {
            div()
                .flex_1()
                .text_size(px(13.))
                .font_family("Cascadia Code")
                .text_color(hex(TEXT_PRIMARY))
                .child(line_text)
                .into_any_element()
        };

        div()
            .h(px(20.))
            .flex()
            .when(is_current, |d| d.bg(hex_a(0xffffff, 0.04)))
            // 行番号
            .child(
                div()
                    .w(px(48.))
                    .text_align(TextAlign::Right)
                    .pr(px(16.))
                    .text_color(if is_current {
                        hex(TEXT_SECONDARY)
                    } else {
                        hex(TEXT_DIM)
                    })
                    .text_size(px(13.))
                    .font_family("Cascadia Code")
                    .child(line_num),
            )
            // テキスト行
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

    fn handle_backspace(
        &mut self,
        _action: &actions::Backspace,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 選択範囲があれば削除
        if self.cursor.delete_selection(&mut self.buffer).is_some() {
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
        self.cursor.preferred_column = None;
        self.ensure_cursor_visible();
        cx.notify();
    }

    fn handle_tab(
        &mut self,
        _action: &actions::Tab,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cursor.delete_selection(&mut self.buffer);
        let new_pos = self.buffer.insert_text(self.cursor.position, "    ");
        self.cursor.position = new_pos;
        self.cursor.preferred_column = None;
        cx.notify();
    }

    fn handle_save(
        &mut self,
        _action: &actions::Save,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.buffer.file_path().is_some() {
            if let Err(e) = self.buffer.save() {
                eprintln!("保存エラー: {}", e);
            }
            cx.notify();
        } else {
            // 保存先未設定 → prompt_for_new_path
            let receiver = cx.prompt_for_new_path(&std::path::PathBuf::from("."), None);
            cx.spawn(async move |entity: WeakEntity<Self>, cx: &mut AsyncApp| {
                if let Ok(Ok(Some(path))) = receiver.await {
                    cx.update(|cx| {
                        entity.update(cx, |this: &mut Self, cx| {
                            if let Err(e) = this.buffer.save_as(&path) {
                                eprintln!("保存エラー: {}", e);
                            }
                            cx.notify();
                        }).ok();
                    }).ok();
                }
            })
            .detach();
        }
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
                        entity.update(cx, |this: &mut Self, cx| {
                            this.load_file(&path, cx);
                        }).ok();
                    }).ok();
                }
            }
        })
        .detach();
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

        // InputHandler 登録用の canvas
        let input_entity = entity.clone();
        let input_focus = focus.clone();

        div()
            .key_context("Editor")
            .track_focus(&focus)
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
            .on_action(cx.listener(Self::handle_backspace))
            .on_action(cx.listener(Self::handle_delete))
            .on_action(cx.listener(Self::handle_enter))
            .on_action(cx.listener(Self::handle_tab))
            .on_action(cx.listener(Self::handle_save))
            .on_action(cx.listener(Self::handle_open))
            .flex_1()
            .flex()
            .flex_col()
            .bg(hex(BG))
            .font_family("Cascadia Code")
            .child(
                // uniform_list で仮想スクロール
                uniform_list("editor-lines", line_count, {
                    let entity = entity.clone();
                    move |range: Range<usize>, _window: &mut Window, cx: &mut App| {
                        let editor = entity.read(cx);
                        range
                            .map(|ix| Self::render_line(editor, ix))
                            .collect()
                    }
                })
                .flex_1()
                .track_scroll(scroll_handle),
            )
            // InputHandler 登録用の不可視 canvas
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

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.ime_text = None;
        self.ime_range = None;
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
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let pos = self.buffer.offset_to_position(range_utf16.start);
        let char_width = px(8.); // monospace 概算
        let line_height = px(20.);
        let x = element_bounds.origin.x + px(48.) + char_width * pos.column as f32;
        let y = element_bounds.origin.y + line_height * pos.line as f32;
        Some(Bounds {
            origin: point(x, y),
            size: size(char_width, line_height),
        })
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let line_height = px(20.);
        let char_width = px(8.);
        let gutter_width = px(48.);
        let line = ((point.y / line_height) as usize)
            .min(self.buffer.line_count().saturating_sub(1));
        let adjusted_x = (point.x - gutter_width).max(px(0.));
        let col = ((adjusted_x / char_width) as usize)
            .min(self.buffer.line_len(line));
        Some(
            self.buffer
                .position_to_offset(Position { line, column: col }),
        )
    }
}
