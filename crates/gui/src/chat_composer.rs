//! Chat ページ用の複数行入力（Enter 送信 / Shift+Enter 改行）

use std::ops::Range;

use gpui::{
    actions, div, fill, point, prelude::*, px, relative, rgba, size, App, Bounds, ClipboardItem,
    Context, CursorStyle, ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, GlobalElementId, KeyBinding, KeyDownEvent, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, SharedString, Style,
    Subscription, TextAlign, TextRun, UTF16Selection, UnderlineStyle, Window, WrappedLine,
};
use unicode_segmentation::*;

use crate::{hex, BG, TEXT_MUTED, TEXT_PRIMARY};

actions!(
    composer,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        ShowCharacterPalette,
        Paste,
        Cut,
        Copy,
        ChatSubmit,
        InsertNewline,
    ]
);

/// 親（AppView）が購読して送信処理を行う
#[derive(Clone, Debug)]
pub struct SubmitChat;

pub struct ChatComposer {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    /// `shape_text` 結果（改行・折り返しあり）
    last_wrapped: Option<Vec<WrappedLine>>,
    last_bounds: Option<Bounds<Pixels>>,
    last_line_height: Option<Pixels>,
    is_selecting: bool,
}

/// ビュー座標から UTF-8 バイトインデックスへ（`WrappedLine` 列用）
fn byte_index_for_point(
    lines: &[WrappedLine],
    position: Point<Pixels>,
    bounds: &Bounds<Pixels>,
    line_height: Pixels,
) -> usize {
    if lines.is_empty() {
        return 0;
    }
    let mut line_origin = bounds.origin;
    let mut line_start_ix = 0usize;
    for line in lines {
        let line_h = line.size(line_height).height;
        let line_bottom = line_origin.y + line_h;
        if position.y > line_bottom {
            line_origin.y = line_bottom;
            line_start_ix += line.len() + 1;
            continue;
        }
        let pos_in_line = position - line_origin;
        let ix = match line.closest_index_for_position(pos_in_line, line_height) {
            Ok(i) | Err(i) => i,
        };
        return (line_start_ix + ix).min(line_start_ix + line.len());
    }
    let mut total = 0usize;
    for (i, line) in lines.iter().enumerate() {
        total += line.len();
        if i + 1 < lines.len() {
            total += 1;
        }
    }
    total
}

fn position_for_utf8_index(
    lines: &[WrappedLine],
    index: usize,
    bounds: &Bounds<Pixels>,
    line_height: Pixels,
) -> Option<Point<Pixels>> {
    if lines.is_empty() {
        return Some(bounds.origin);
    }
    let mut line_origin = bounds.origin;
    let mut line_start_ix = 0usize;
    for line in lines {
        let line_end_ix = line_start_ix + line.len();
        if index < line_start_ix {
            break;
        } else if index > line_end_ix {
            line_origin.y += line.size(line_height).height;
            line_start_ix = line_end_ix + 1;
            continue;
        } else {
            let ix_within = index - line_start_ix;
            return Some(line_origin + line.position_for_index(ix_within, line_height)?);
        }
    }
    None
}

impl ChatComposer {
    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx)
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx)
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx)
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_handle.focus(window);
        self.is_selecting = true;

        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), cx)
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    fn insert_newline(&mut self, _: &InsertNewline, window: &mut Window, cx: &mut Context<Self>) {
        self.replace_text_in_range(None, "\n", window, cx);
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }
    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx)
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        cx.notify()
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }

        let Some(bounds) = self.last_bounds.as_ref() else {
            return 0;
        };
        let lh = self.last_line_height.unwrap_or(px(20.));
        if let Some(lines) = self.last_wrapped.as_ref() {
            if !lines.is_empty() {
                return byte_index_for_point(lines, position, bounds, lh);
            }
        }
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        0
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset
        } else {
            self.selected_range.end = offset
        };
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify()
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;

        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }

        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;

        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }

        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .as_str()
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .as_str()
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }

    fn reset(&mut self) {
        self.content = "".into();
        self.selected_range = 0..0;
        self.selection_reversed = false;
        self.marked_range = None;
        self.last_wrapped = None;
        self.last_bounds = None;
        self.last_line_height = None;
        self.is_selecting = false;
    }

    pub fn new(cx: &mut Context<Self>, placeholder: impl Into<SharedString>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: "".into(),
            placeholder: placeholder.into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_wrapped: None,
            last_bounds: None,
            last_line_height: None,
            is_selecting: false,
        }
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.reset();
        cx.notify();
    }

    /// クリックやページ切替後に IME / 文字入力を受け取るにはフォーカスが必要（Editor と同様）
    pub fn focus(&self, window: &mut Window) {
        self.focus_handle.focus(window);
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        if key != "enter" && key != "return" {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
            return;
        }

        window.prevent_default();
        cx.stop_propagation();

        if modifiers.shift {
            self.insert_newline(&InsertNewline, window, cx);
        } else {
            self.submit_chat(&ChatSubmit, window, cx);
        }
    }

    fn submit_chat(&mut self, _: &ChatSubmit, _: &mut Window, cx: &mut Context<Self>) {
        if !self.content.trim().is_empty() {
            cx.emit(SubmitChat);
        }
    }
}

impl EventEmitter<SubmitChat> for ChatComposer {}

impl EntityInputHandler for ChatComposer {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.marked_range.take();
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());

        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        if !new_text.is_empty() {
            self.marked_range = Some(range.start..range.start + new_text.len());
        } else {
            self.marked_range = None;
        }
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .map(|new_range| new_range.start + range.start..new_range.end + range.end)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());

        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        let lines = self.last_wrapped.as_ref()?;
        let lb = self.last_bounds?;
        let lh = self
            .last_line_height
            .unwrap_or_else(|| window.line_height());
        let p0 = position_for_utf8_index(lines, range.start, &lb, lh)?;
        let p1 = position_for_utf8_index(lines, range.end, &lb, lh)?;
        let _ = bounds;
        Some(Bounds::from_corners(p0, p1))
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds.as_ref()?;
        let lh = self
            .last_line_height
            .unwrap_or_else(|| window.line_height());
        if let Some(lines) = self.last_wrapped.as_ref() {
            if !lines.is_empty() {
                let utf8_index = byte_index_for_point(lines, point, bounds, lh);
                return Some(self.offset_to_utf16(utf8_index));
            }
        }
        None
    }
}

struct TextElement {
    input: Entity<ChatComposer>,
}

struct PrepaintState {
    lines: Vec<WrappedLine>,
    cursor: Option<PaintQuad>,
    selection: Vec<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let input = self.input.read(cx);
        let content = input.content.as_str();
        let lh = window.line_height();
        let hard_lines = content.matches('\n').count() + 1;
        let est_wrap = content.len().saturating_div(96);
        let rows = (hard_lines + est_wrap).min(24).max(1);
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = (lh * rows as f32 + px(8.0)).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let style = window.text_style();

        let (display_text, text_color) = if content.is_empty() {
            (input.placeholder.clone(), hex(TEXT_MUTED))
        } else {
            (content, hex(TEXT_PRIMARY))
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if let Some(marked_range) = input.marked_range.as_ref() {
            vec![
                TextRun {
                    len: marked_range.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked_range.end - marked_range.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len() - marked_range.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![run]
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let lines: Vec<WrappedLine> = window
            .text_system()
            .shape_text(
                display_text.clone(),
                font_size,
                &runs,
                Some(bounds.size.width),
                None,
            )
            .map(|s| s.into_vec())
            .unwrap_or_default();

        let lh = window.line_height();

        let cursor_quad = if selected_range.is_empty() {
            position_for_utf8_index(&lines, cursor, &bounds, lh)
                .map(|p| fill(Bounds::new(point(p.x, p.y), size(px(2.), lh)), gpui::blue()))
        } else {
            None
        };

        let mut selection_quads = Vec::new();
        if !selected_range.is_empty() {
            let lo = selected_range.start.min(selected_range.end);
            let hi = selected_range.start.max(selected_range.end);
            let mut line_origin = bounds.origin;
            let mut line_start = 0usize;
            for line in &lines {
                let line_end = line_start + line.len();
                let seg_lo = lo.max(line_start);
                let seg_hi = hi.min(line_end);
                if seg_lo < seg_hi {
                    if let (Some(p0), Some(p1)) = (
                        line.position_for_index(seg_lo - line_start, lh),
                        line.position_for_index(seg_hi - line_start, lh),
                    ) {
                        let p0 = line_origin + p0;
                        let p1 = line_origin + p1;
                        let left = p0.x.min(p1.x);
                        let right = p0.x.max(p1.x).max(left + px(2.));
                        let top = p0.y.min(p1.y);
                        selection_quads.push(fill(
                            Bounds::from_corners(point(left, top), point(right, top + lh)),
                            rgba(0x3311ff30),
                        ));
                    }
                }
                line_origin.y += line.size(lh).height;
                line_start = line_end + 1;
            }
        }
        PrepaintState {
            lines,
            cursor: cursor_quad,
            selection: selection_quads,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        for q in prepaint.selection.drain(..) {
            window.paint_quad(q);
        }

        let lh = window.line_height();
        let mut y = bounds.origin.y;
        for line in &prepaint.lines {
            let h = line.size(lh).height;
            line.paint(
                point(bounds.origin.x, y),
                lh,
                TextAlign::Left,
                Some(Bounds::new(
                    point(bounds.origin.x, y),
                    size(bounds.size.width, h),
                )),
                window,
                cx,
            )
            .ok();
            y += h;
        }

        if focus_handle.is_focused(window) {
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        }

        self.input.update(cx, |input, _cx| {
            input.last_wrapped = Some(prepaint.lines.clone());
            input.last_bounds = Some(bounds);
            input.last_line_height = Some(lh);
        });
    }
}

impl Render for ChatComposer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .w_full()
            .min_w(px(0.))
            .key_context("ChatComposer")
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::show_character_palette))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::insert_newline))
            .on_action(cx.listener(Self::submit_chat))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .text_color(hex(TEXT_PRIMARY))
            .line_height(px(22.))
            .text_size(px(13.))
            .child(
                div()
                    .min_h(px(22. + 4. * 2.))
                    .w_full()
                    .p(px(4.))
                    .bg(hex(BG))
                    .child(TextElement { input: cx.entity() }),
            )
    }
}

impl Focusable for ChatComposer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// Chat 入力フォーカス時のキーマップ（`key_context("ChatComposer")` と対）
pub fn register_keybindings(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some("ChatComposer")),
        KeyBinding::new("delete", Delete, Some("ChatComposer")),
        KeyBinding::new("left", Left, Some("ChatComposer")),
        KeyBinding::new("right", Right, Some("ChatComposer")),
        KeyBinding::new("shift-left", SelectLeft, Some("ChatComposer")),
        KeyBinding::new("shift-right", SelectRight, Some("ChatComposer")),
        KeyBinding::new("ctrl-a", SelectAll, Some("ChatComposer")),
        KeyBinding::new("cmd-a", SelectAll, Some("ChatComposer")),
        KeyBinding::new("ctrl-v", Paste, Some("ChatComposer")),
        KeyBinding::new("cmd-v", Paste, Some("ChatComposer")),
        KeyBinding::new("ctrl-c", Copy, Some("ChatComposer")),
        KeyBinding::new("cmd-c", Copy, Some("ChatComposer")),
        KeyBinding::new("ctrl-x", Cut, Some("ChatComposer")),
        KeyBinding::new("cmd-x", Cut, Some("ChatComposer")),
        KeyBinding::new("home", Home, Some("ChatComposer")),
        KeyBinding::new("end", End, Some("ChatComposer")),
        KeyBinding::new("shift-enter", InsertNewline, Some("ChatComposer")),
        KeyBinding::new("enter", ChatSubmit, Some("ChatComposer")),
        KeyBinding::new("ctrl-cmd-space", ShowCharacterPalette, Some("ChatComposer")),
    ]);
}

pub fn install_enter_submit_interceptor(cx: &mut App) -> Subscription {
    cx.intercept_keystrokes(|event, window, cx| {
        if event.keystroke.key != "enter" {
            return;
        }
        if event.keystroke.modifiers.modified() {
            return;
        }
        if !window.is_action_available(&ChatSubmit, cx) {
            return;
        }
        let Some(focused) = window.focused(cx) else {
            return;
        };
        focused.dispatch_action(&ChatSubmit, window, cx);
        cx.stop_propagation();
    })
}
