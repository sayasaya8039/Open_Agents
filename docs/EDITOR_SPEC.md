# Open Agents コードエディタ 製品仕様書

> gpui 0.2.2 ベースの本格的コードエディタ実装仕様

## 1. 現状分析

### 現在の実装（crates/gui/src/main.rs）

- `AppView` 構造体が `code: String` フィールドに静的テキストを保持
- `render_editor()` は `self.code.lines().enumerate()` で行を描画するのみ
- キー入力ハンドリングなし、カーソルなし、編集不可
- フォント: Cascadia Code、行番号表示あり（静的）
- ステータスバー: "Ln 1, Col 1" 固定表示

### gpui 0.2.2 で利用可能なテキスト入力 API

| API | 場所 | 用途 |
|-----|------|------|
| `EntityInputHandler` トレイト | `src/input.rs` | View にテキスト入力を実装する主要トレイト |
| `ElementInputHandler<V>` | `src/input.rs` | `EntityInputHandler` → `InputHandler` ブリッジ |
| `Window::handle_input()` | `src/window.rs:3400` | paint 時に InputHandler を登録 |
| `FocusHandle` | `src/window.rs:266` | フォーカス管理（`handle_input` に必須） |
| `InteractiveElement::on_key_down()` | `src/elements/div.rs:881` | KeyDown イベントリスナー |
| `InteractiveElement::on_action()` | `src/elements/div.rs:854` | Action ディスパッチ |
| `KeyDownEvent` | `src/interactive.rs:22` | キーストローク + is_held |
| `Keystroke` | `src/platform/keystroke.rs` | key, modifiers, ime_key |
| `ClipboardItem` / `App::read_from_clipboard()` | `src/app.rs:1053` | クリップボード読み書き |
| `App::prompt_for_paths()` | `src/app.rs:1116` | ファイル選択ダイアログ |
| `App::prompt_for_new_path()` | `src/app.rs:1129` | 保存ダイアログ |
| `uniform_list()` | `src/elements/uniform_list.rs` | 同一高さ要素の仮想スクロール |
| `UniformListScrollHandle` | `src/elements/uniform_list.rs:80` | スクロール位置の制御 |

### EntityInputHandler の実装が必要なメソッド

```rust
pub trait EntityInputHandler: 'static + Sized {
    // カーソル位置のテキスト範囲を返す（UTF-16）
    fn selected_text_range(&mut self, ignore_disabled_input: bool,
        window: &mut Window, cx: &mut Context<Self>) -> Option<UTF16Selection>;

    // IME マーク範囲（日本語入力の変換中範囲）
    fn marked_text_range(&self, window: &mut Window,
        cx: &mut Context<Self>) -> Option<Range<usize>>;

    // 指定範囲のテキストを返す
    fn text_for_range(&mut self, range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        window: &mut Window, cx: &mut Context<Self>) -> Option<String>;

    // IME マーク解除
    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>);

    // テキスト置換（IME 確定時に呼ばれる）
    fn replace_text_in_range(&mut self, range: Option<Range<usize>>,
        text: &str, window: &mut Window, cx: &mut Context<Self>);

    // IME 変換中テキストの設定
    fn replace_and_mark_text_in_range(&mut self,
        range: Option<Range<usize>>, new_text: &str,
        new_selected_range: Option<Range<usize>>,
        window: &mut Window, cx: &mut Context<Self>);

    // IME 候補ウィンドウ配置用の座標を返す
    fn bounds_for_range(&mut self, range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        window: &mut Window, cx: &mut Context<Self>) -> Option<Bounds<Pixels>>;

    // ポイント座標から文字インデックスへの変換
    fn character_index_for_point(&mut self, point: Point<Pixels>,
        window: &mut Window, cx: &mut Context<Self>) -> Option<usize>;
}
```

---

## 2. 機能一覧（優先度付き）

### P0（必須 — MVP）

| # | 機能 | 説明 |
|---|------|------|
| P0-1 | テキスト入力 | キーボードからの文字入力、IME（日本語入力）対応 |
| P0-2 | カーソル移動 | 矢印キー（上下左右）、Home/End、Ctrl+矢印（ワード単位） |
| P0-3 | バックスペース/デリート | 文字削除、Ctrl+Backspace（ワード削除） |
| P0-4 | 行番号 | 現在行ハイライト付き行番号ガター |
| P0-5 | スクロール | マウスホイール + uniform_list による仮想スクロール |
| P0-6 | ファイル開く/保存 | Ctrl+O / Ctrl+S、ファイルダイアログ連携 |

### P1（重要）

| # | 機能 | 説明 |
|---|------|------|
| P1-1 | シンタックスハイライト | Tree-sitter ベースの構文色分け |
| P1-2 | テキスト選択 | Shift+矢印、Shift+Ctrl+矢印、マウスドラッグ、ダブルクリック選択 |
| P1-3 | コピー/カット/ペースト | Ctrl+C/X/V、選択範囲対応 |
| P1-4 | Undo/Redo | Ctrl+Z/Y、操作履歴スタック |

### P2（将来）

| # | 機能 | 説明 |
|---|------|------|
| P2-1 | マルチカーソル | Ctrl+D、Alt+クリック |
| P2-2 | 自動補完 | ポップアップ候補リスト |
| P2-3 | 検索/置換 | Ctrl+F/H、正規表現対応 |
| P2-4 | ミニマップ | 右端のコード概観 |
| P2-5 | 括弧マッチング | 対応括弧のハイライト |

---

## 3. アーキテクチャ設計

### ファイル分割計画

```
crates/gui/src/
├── main.rs              # Application エントリポイント、AppView
├── editor/
│   ├── mod.rs           # EditorView（Render 実装、描画ロジック）
│   ├── buffer.rs        # TextBuffer（テキストデータ構造）
│   ├── cursor.rs        # Cursor / Selection 管理
│   ├── input_handler.rs # EntityInputHandler 実装
│   ├── actions.rs       # Action 定義（MoveUp, MoveDown, Backspace 等）
│   ├── keybindings.rs   # キーバインド定義
│   └── highlight.rs     # シンタックスハイライト（P1）
├── sidebar.rs           # サイドバー（既存コード分離）
├── titlebar.rs          # タイトルバー（既存コード分離）
├── statusbar.rs         # ステータスバー
├── chat.rs              # チャットページ
├── settings.rs          # 設定ページ
├── terminal.rs          # ターミナルページ
└── theme.rs             # カラー定数・テーマ
```

### テキストバッファのデータ構造

```rust
// buffer.rs

/// 行ベースのテキストバッファ
/// Zed は Rope を使うが、MVP では Vec<String> で十分
pub struct TextBuffer {
    /// 各行のテキスト（改行文字を含まない）
    lines: Vec<String>,
    /// ファイルパス（None = 無題）
    file_path: Option<PathBuf>,
    /// 変更フラグ
    dirty: bool,
}

impl TextBuffer {
    pub fn new() -> Self { ... }
    pub fn from_file(path: &Path) -> io::Result<Self> { ... }
    pub fn save(&mut self) -> io::Result<()> { ... }
    pub fn save_as(&mut self, path: &Path) -> io::Result<()> { ... }

    // --- テキスト操作 ---
    pub fn insert_char(&mut self, pos: Position, ch: char) -> Position { ... }
    pub fn insert_text(&mut self, pos: Position, text: &str) -> Position { ... }
    pub fn delete_range(&mut self, start: Position, end: Position) -> Position { ... }
    pub fn line(&self, idx: usize) -> &str { ... }
    pub fn line_count(&self) -> usize { ... }
    pub fn line_len(&self, idx: usize) -> usize { ... }

    // --- UTF-16 変換（InputHandler 用） ---
    pub fn offset_to_position(&self, offset_utf16: usize) -> Position { ... }
    pub fn position_to_offset(&self, pos: Position) -> usize { ... }
    pub fn text_in_range_utf16(&self, range: Range<usize>) -> String { ... }
}

/// バッファ内の位置
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    pub line: usize,   // 0-based
    pub column: usize, // 0-based, UTF-8 byte offset
}
```

### カーソル管理

```rust
// cursor.rs

/// カーソル状態
pub struct CursorState {
    /// カーソル位置（= 選択範囲の末端）
    pub position: Position,
    /// 選択範囲の開始点（None = 選択なし）
    pub anchor: Option<Position>,
    /// 上下移動時の目標カラム（タブ幅考慮）
    pub preferred_column: Option<usize>,
}

impl CursorState {
    pub fn selection_range(&self) -> Option<(Position, Position)> { ... }
    pub fn has_selection(&self) -> bool { self.anchor.is_some() }
    pub fn clear_selection(&mut self) { self.anchor = None; }

    // --- 移動 ---
    pub fn move_left(&mut self, buffer: &TextBuffer, extend_selection: bool) { ... }
    pub fn move_right(&mut self, buffer: &TextBuffer, extend_selection: bool) { ... }
    pub fn move_up(&mut self, buffer: &TextBuffer, extend_selection: bool) { ... }
    pub fn move_down(&mut self, buffer: &TextBuffer, extend_selection: bool) { ... }
    pub fn move_to_line_start(&mut self, extend_selection: bool) { ... }
    pub fn move_to_line_end(&mut self, buffer: &TextBuffer, extend_selection: bool) { ... }
    pub fn move_word_left(&mut self, buffer: &TextBuffer, extend_selection: bool) { ... }
    pub fn move_word_right(&mut self, buffer: &TextBuffer, extend_selection: bool) { ... }
}
```

### EditorView（メイン描画）

```rust
// editor/mod.rs

pub struct EditorView {
    buffer: TextBuffer,
    cursor: CursorState,
    focus_handle: FocusHandle,
    scroll_handle: UniformListScrollHandle,
    /// IME 変換中テキスト
    ime_text: Option<String>,
    ime_range: Option<Range<usize>>,
    /// Undo/Redo スタック（P1）
    undo_stack: Vec<EditOperation>,
    redo_stack: Vec<EditOperation>,
}

impl Render for EditorView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity().clone();
        let focus = self.focus_handle.clone();
        let line_count = self.buffer.line_count();
        let line_height = px(20.);

        div()
            .key_context("Editor")
            .track_focus(&focus)
            .on_action(cx.listener(Self::move_up))
            .on_action(cx.listener(Self::move_down))
            .on_action(cx.listener(Self::move_left))
            .on_action(cx.listener(Self::move_right))
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::open))
            .flex_1()
            .flex()
            .bg(hex(BG))
            .child(
                // uniform_list で仮想スクロール
                uniform_list(
                    view.clone(),
                    "editor-lines",
                    line_count,
                    move |editor, visible_range, window, cx| {
                        visible_range
                            .map(|ix| editor.render_line(ix, window, cx))
                            .collect()
                    },
                )
                .flex_1()
                .track_scroll(self.scroll_handle.clone()),
            )
    }
}

impl EditorView {
    fn render_line(&self, line_idx: usize, _window: &mut Window,
                   _cx: &mut Context<Self>) -> impl IntoElement {
        let is_current = self.cursor.position.line == line_idx;
        let line_text = self.buffer.line(line_idx).to_string();
        let line_num = format!("{:>4}", line_idx + 1);

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
                    .text_color(if is_current { hex(TEXT_SECONDARY) } else { hex(TEXT_DIM) })
                    .text_size(px(13.))
                    .child(line_num),
            )
            // テキスト行（カーソル描画含む）
            .child(
                div()
                    .flex_1()
                    .text_size(px(13.))
                    .font_family("Cascadia Code")
                    .text_color(hex(TEXT_PRIMARY))
                    .child(line_text),
            )
    }
}
```

### EntityInputHandler 実装パターン

```rust
// input_handler.rs

impl EntityInputHandler for EditorView {
    fn selected_text_range(&mut self, _ignore: bool,
        _window: &mut Window, _cx: &mut Context<Self>) -> Option<UTF16Selection> {
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

    fn marked_text_range(&self, _window: &mut Window,
        _cx: &mut Context<Self>) -> Option<Range<usize>> {
        self.ime_range.clone()
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.ime_text = None;
        self.ime_range = None;
    }

    fn replace_text_in_range(&mut self, range: Option<Range<usize>>,
        text: &str, _window: &mut Window, cx: &mut Context<Self>) {
        // IME 確定 or 通常文字入力
        let range = range.unwrap_or_else(|| {
            let offset = self.buffer.position_to_offset(self.cursor.position);
            offset..offset
        });
        let start = self.buffer.offset_to_position(range.start);
        let end = self.buffer.offset_to_position(range.end);
        self.buffer.delete_range(start, end);
        let new_pos = self.buffer.insert_text(start, text);
        self.cursor.position = new_pos;
        self.cursor.clear_selection();
        self.ime_text = None;
        self.ime_range = None;
        cx.notify();
    }

    fn replace_and_mark_text_in_range(&mut self,
        range: Option<Range<usize>>, new_text: &str,
        new_selected_range: Option<Range<usize>>,
        _window: &mut Window, cx: &mut Context<Self>) {
        // IME 変換中の表示更新
        let range = range.unwrap_or_else(|| {
            let offset = self.buffer.position_to_offset(self.cursor.position);
            offset..offset
        });
        let start = self.buffer.offset_to_position(range.start);
        let end = self.buffer.offset_to_position(range.end);
        self.buffer.delete_range(start, end);
        let new_pos = self.buffer.insert_text(start, new_text);
        self.cursor.position = new_pos;

        let mark_start = range.start;
        let mark_end = mark_start + new_text.encode_utf16().count();
        self.ime_text = Some(new_text.to_string());
        self.ime_range = Some(mark_start..mark_end);
        cx.notify();
    }

    fn text_for_range(&mut self, range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window, _cx: &mut Context<Self>) -> Option<String> {
        *adjusted_range = Some(range.clone());
        Some(self.buffer.text_in_range_utf16(range))
    }

    fn bounds_for_range(&mut self, range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window, _cx: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        // IME 候補ウィンドウの位置を返す
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

    fn character_index_for_point(&mut self, point: Point<Pixels>,
        _window: &mut Window, _cx: &mut Context<Self>) -> Option<usize> {
        // マウスクリック位置 → 文字オフセット変換
        let line_height = px(20.);
        let char_width = px(8.);
        let line = ((point.y / line_height).0 as usize)
            .min(self.buffer.line_count().saturating_sub(1));
        let col = ((point.x / char_width).0 as usize)
            .min(self.buffer.line_len(line));
        Some(self.buffer.position_to_offset(Position { line, column: col }))
    }
}
```

### Action 定義とキーバインド

```rust
// actions.rs
use gpui::actions;

// P0 Actions
actions!(editor, [
    MoveUp, MoveDown, MoveLeft, MoveRight,
    MoveToLineStart, MoveToLineEnd,
    MoveWordLeft, MoveWordRight,
    Backspace, Delete,
    Enter, Tab,
    Save, Open,
]);

// P1 Actions
actions!(editor, [
    SelectUp, SelectDown, SelectLeft, SelectRight,
    SelectWordLeft, SelectWordRight,
    SelectToLineStart, SelectToLineEnd,
    SelectAll,
    Copy, Cut, Paste,
    Undo, Redo,
]);
```

```rust
// keybindings.rs — Application::new() 後に登録
fn register_keybindings(cx: &mut App) {
    cx.bind_keys([
        // P0: カーソル移動
        KeyBinding::new("up", MoveUp, Some("Editor")),
        KeyBinding::new("down", MoveDown, Some("Editor")),
        KeyBinding::new("left", MoveLeft, Some("Editor")),
        KeyBinding::new("right", MoveRight, Some("Editor")),
        KeyBinding::new("home", MoveToLineStart, Some("Editor")),
        KeyBinding::new("end", MoveToLineEnd, Some("Editor")),
        KeyBinding::new("ctrl-left", MoveWordLeft, Some("Editor")),
        KeyBinding::new("ctrl-right", MoveWordRight, Some("Editor")),
        KeyBinding::new("backspace", Backspace, Some("Editor")),
        KeyBinding::new("delete", Delete, Some("Editor")),
        KeyBinding::new("enter", Enter, Some("Editor")),
        KeyBinding::new("tab", Tab, Some("Editor")),
        KeyBinding::new("ctrl-s", Save, Some("Editor")),
        KeyBinding::new("ctrl-o", Open, Some("Editor")),

        // P1: 選択
        KeyBinding::new("shift-up", SelectUp, Some("Editor")),
        KeyBinding::new("shift-down", SelectDown, Some("Editor")),
        KeyBinding::new("shift-left", SelectLeft, Some("Editor")),
        KeyBinding::new("shift-right", SelectRight, Some("Editor")),
        KeyBinding::new("ctrl-shift-left", SelectWordLeft, Some("Editor")),
        KeyBinding::new("ctrl-shift-right", SelectWordRight, Some("Editor")),
        KeyBinding::new("ctrl-a", SelectAll, Some("Editor")),

        // P1: クリップボード
        KeyBinding::new("ctrl-c", Copy, Some("Editor")),
        KeyBinding::new("ctrl-x", Cut, Some("Editor")),
        KeyBinding::new("ctrl-v", Paste, Some("Editor")),

        // P1: Undo/Redo
        KeyBinding::new("ctrl-z", Undo, Some("Editor")),
        KeyBinding::new("ctrl-y", Redo, Some("Editor")),
    ]);
}
```

### paint 時の InputHandler 登録

```rust
// EditorView の Element 実装（カスタム Element を使う場合）
// または render() 内で canvas を使って登録

// 方法1: render() 内で canvas 要素を使う
fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let view = cx.entity().clone();
    let focus = self.focus_handle.clone();

    div()
        .track_focus(&focus)
        // ... 他の child ...
        .child(
            canvas(
                |bounds, _window, _cx| bounds,
                move |bounds, _bounds, window, cx| {
                    // paint phase で InputHandler を登録
                    if focus.is_focused(window) {
                        let handler = ElementInputHandler::new(bounds, view.clone());
                        window.handle_input(&focus, handler, cx);
                    }
                },
            )
            .absolute()
            .size_full(),
        )
}
```

---

## 4. スプリント計画

### Sprint 1: TextBuffer + カーソル + テキスト入力（P0-1, P0-2, P0-3）

**目標**: キーボードで文字を入力・削除でき、カーソルが動くエディタ

#### タスク

1. **ファイル分割**: main.rs からテーマ定数・サイドバー・タイトルバーを分離
2. **buffer.rs 実装**: `TextBuffer` (Vec<String> ベース)
   - `insert_char`, `insert_text`, `delete_range`
   - `offset_to_position`, `position_to_offset`（UTF-16 変換）
   - `text_in_range_utf16`
3. **cursor.rs 実装**: `CursorState`
   - 矢印キー移動（上下左右）
   - Home/End、Ctrl+矢印（ワード移動）
   - `preferred_column` で上下移動時のカラム維持
4. **actions.rs**: Action 定義 + キーバインド登録
5. **input_handler.rs**: `EntityInputHandler` 実装
   - `replace_text_in_range` で文字入力受付
   - `replace_and_mark_text_in_range` で IME 対応
   - `selected_text_range` でカーソル位置通知
6. **editor/mod.rs**: `EditorView` 実装
   - `FocusHandle` 生成・管理
   - `track_focus` でフォーカス取得
   - canvas による `handle_input` 登録
   - カーソル描画（点滅する縦線）
   - `on_action` で各アクションをハンドリング
7. **main.rs 更新**: `AppView` が `EditorView` を Entity として保持・描画

#### 検証方法

- [ ] アプリ起動後、エディタ領域クリックでフォーカス取得
- [ ] 英字・数字・記号の入力が画面に反映される
- [ ] 日本語 IME で変換・確定が正常に動作する
- [ ] 矢印キーでカーソルが移動する（上下左右）
- [ ] Backspace/Delete で文字が削除される
- [ ] Enter で改行が挿入される
- [ ] Ctrl+矢印でワード単位移動
- [ ] カーソル位置にブリンクする縦線が表示される

---

### Sprint 2: 行番号 + スクロール（P0-4, P0-5）

**目標**: 大量のテキストでも快適にスクロールでき、行番号が表示される

#### タスク

1. **uniform_list 導入**: 固定行高さ（20px）で仮想スクロール
   - `UniformListScrollHandle` でスクロール位置管理
   - 可視行のみレンダリング（1000行以上でもスムーズ）
2. **行番号ガター**: 現在行ハイライト、右揃え行番号
3. **カーソル追従スクロール**: カーソルが画面外に出たら自動スクロール
   - `scroll_handle.scroll_to_item(line_index)` を利用
4. **マウスホイールスクロール**: uniform_list が標準対応

#### 検証方法

- [ ] 1000行のファイルが遅延なく表示される
- [ ] マウスホイールでスムーズスクロール
- [ ] 行番号が常に正しく表示される
- [ ] 現在行の行番号がハイライトされる
- [ ] 矢印キーで画面外に移動するとスクロールが追従する

---

### Sprint 3: ファイル開く/保存（P0-6）

**目標**: ファイルの読み込みと保存ができる

#### タスク

1. **Ctrl+O（Open）**: `cx.prompt_for_paths()` でファイル選択 → `TextBuffer::from_file()`
2. **Ctrl+S（Save）**:
   - ファイルパスあり → 上書き保存
   - ファイルパスなし → `cx.prompt_for_new_path()` で保存先選択
3. **ダーティフラグ**: 未保存変更がある場合、タブに `●` マーク表示
4. **タイトルバー更新**: ファイル名をタイトルに反映
5. **サイドバー連携**: 開いたファイルをファイルリストに反映

#### 検証方法

- [ ] Ctrl+O でファイル選択ダイアログが開く
- [ ] 選択したファイルの内容がエディタに表示される
- [ ] Ctrl+S で保存ダイアログ（新規）/ 上書き保存（既存）
- [ ] 保存後にダーティフラグがリセットされる
- [ ] タイトルバーにファイル名が表示される

---

### Sprint 4: テキスト選択 + コピー/ペースト（P1-2, P1-3）

**目標**: テキスト選択とクリップボード操作ができる

#### タスク

1. **Shift+矢印選択**: `CursorState.anchor` を設定して選択範囲管理
2. **選択範囲描画**: 選択部分に半透明ハイライト背景
3. **マウスドラッグ選択**: MouseDown → MouseMove で選択
4. **ダブルクリック**: ワード選択
5. **Ctrl+A**: 全選択
6. **Ctrl+C（Copy）**: `cx.write_to_clipboard(ClipboardItem::new(selected_text))`
7. **Ctrl+X（Cut）**: コピー + 選択範囲削除
8. **Ctrl+V（Paste）**: `cx.read_from_clipboard()` → 選択範囲を置換挿入
9. **選択中の入力**: 選択範囲を削除して新しい文字を挿入

#### 検証方法

- [ ] Shift+矢印で選択範囲が青くハイライトされる
- [ ] マウスドラッグで選択できる
- [ ] ダブルクリックでワード選択
- [ ] Ctrl+C → Ctrl+V でテキストがコピペされる
- [ ] Ctrl+X で選択テキストが切り取られる
- [ ] 選択状態で文字入力すると選択範囲が置換される
- [ ] Ctrl+A で全選択される

---

### Sprint 5: Undo/Redo（P1-4）

**目標**: 編集操作を取り消し/やり直しできる

#### タスク

1. **EditOperation 構造体**: 変更前テキスト・変更後テキスト・範囲・カーソル位置を記録
2. **Undo スタック**: 各編集操作を push
3. **Redo スタック**: Undo 実行時に push、新規編集時にクリア
4. **操作グループ化**: 連続入力をまとめて1つの Undo 単位にする
   - タイマー（500ms 無操作で区切り）or ワード境界で区切り

```rust
pub struct EditOperation {
    /// 変更前のテキスト（range 内の元テキスト）
    old_text: String,
    /// 変更後のテキスト
    new_text: String,
    /// 変更範囲（UTF-8 Position ベース）
    range_start: Position,
    range_end: Position,
    /// Undo 後のカーソル位置
    cursor_before: Position,
    cursor_after: Position,
}
```

#### 検証方法

- [ ] 文字入力後に Ctrl+Z で入力が取り消される
- [ ] Ctrl+Y で取り消した入力が復元される
- [ ] 連続入力がまとめて1つの Undo 単位になる
- [ ] 削除操作も Undo/Redo できる
- [ ] Undo 後に新しい入力をすると Redo スタックがクリアされる

---

### Sprint 6: シンタックスハイライト（P1-1）

**目標**: コードに色が付く

#### タスク

1. **tree-sitter 導入**: `tree-sitter` + `tree-sitter-typescript` (等) を Cargo.toml に追加
2. **highlight.rs**: Tree-sitter パーサーで AST を構築
3. **ハイライトテーマ**: VS Code Dark 風のカラーマッピング
   - keyword → 紫 (`PURPLE`)
   - string → オレンジ (`ACCENT_ORANGE`)
   - comment → グレー (`TEXT_DIM`)
   - function → 青 (`ACCENT_BLUE`)
   - type → ティール
   - number → ライトグリーン
4. **インクリメンタル解析**: 編集時に差分だけ再解析
5. **render_line の更新**: ハイライト情報に基づいて文字ごとに色分け

#### 検証方法

- [ ] .ts/.tsx ファイルでキーワードに色が付く
- [ ] コメント、文字列、関数名が異なる色で表示される
- [ ] 編集中にリアルタイムでハイライトが更新される
- [ ] 1000行のファイルでもハイライトが遅延しない

---

## 5. 依存関係

### Cargo.toml 追加（段階的）

```toml
[dependencies]
gpui = "0.2.2"

# Sprint 6 で追加
# tree-sitter = "0.24"
# tree-sitter-typescript = "0.23"
# tree-sitter-rust = "0.23"
# tree-sitter-python = "0.23"
```

### Zed エディタからの参考ポイント

| Zed のコンポーネント | 参考にする部分 | 簡略化の方針 |
|---------------------|---------------|-------------|
| `crates/editor/src/editor.rs` | Action ハンドリング、キーバインド構造 | 基本的な操作のみ |
| `crates/editor/src/display_map/` | テキスト → 表示座標変換 | 単純な行×カラム計算 |
| `crates/editor/src/selections_collection.rs` | 選択範囲管理 | シングルカーソルのみ（MVP） |
| `crates/language/src/buffer.rs` | Rope ベースバッファ | Vec<String> で代用 |
| `crates/editor/src/element/` | gpui Element でのエディタ描画 | uniform_list + canvas |

---

## 6. リスクと対策

| リスク | 影響 | 対策 |
|--------|------|------|
| **IME 入力がうまく動かない** | 高（日本語入力必須） | EntityInputHandler を忠実に実装、Windows IME の挙動を早期テスト |
| **UTF-16 ↔ UTF-8 変換ミス** | 中（カーソル位置ずれ） | 絵文字・CJK文字を含むテストケースで検証 |
| **カーソル描画位置のずれ** | 中（UX低下） | monospace フォント前提で char_width 固定、プロポーショナル対応は P2 |
| **大ファイルでの性能劣化** | 中 | uniform_list で仮想スクロール、10000行テストで検証 |
| **Undo/Redo の状態不整合** | 低 | EditOperation に十分な情報を記録、プロパティテスト |

---

## 7. 成功基準（全体）

- [ ] **P0 完了**: テキスト入力・カーソル移動・削除・行番号・スクロール・ファイル開く/保存
- [ ] **IME 対応**: 日本語入力（変換・確定）が正常動作
- [ ] **パフォーマンス**: 10000行ファイルで 60fps 維持
- [ ] **ファイル数**: main.rs が 200行以下（モジュール分割済み）
- [ ] **P1 完了**: シンタックスハイライト・選択・コピペ・Undo/Redo
