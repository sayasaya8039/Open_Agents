// アクション定義 — キーバインドと紐づくコマンド

use gpui::{actions, KeyBinding};

// P0: 基本操作
actions!(
    editor,
    [
        MoveUp,
        MoveDown,
        MoveLeft,
        MoveRight,
        MoveToLineStart,
        MoveToLineEnd,
        MoveWordLeft,
        MoveWordRight,
        Backspace,
        Delete,
        Enter,
        Tab,
        Save,
        Open,
    ]
);

// 選択操作（Shift+矢印）
actions!(
    editor,
    [
        SelectUp,
        SelectDown,
        SelectLeft,
        SelectRight,
        SelectToLineStart,
        SelectToLineEnd,
        SelectWordLeft,
        SelectWordRight,
    ]
);

/// キーバインド登録
pub fn register_keybindings(cx: &mut gpui::App) {
    cx.bind_keys([
        // カーソル移動
        KeyBinding::new("up", MoveUp, Some("Editor")),
        KeyBinding::new("down", MoveDown, Some("Editor")),
        KeyBinding::new("left", MoveLeft, Some("Editor")),
        KeyBinding::new("right", MoveRight, Some("Editor")),
        KeyBinding::new("home", MoveToLineStart, Some("Editor")),
        KeyBinding::new("end", MoveToLineEnd, Some("Editor")),
        KeyBinding::new("ctrl-left", MoveWordLeft, Some("Editor")),
        KeyBinding::new("ctrl-right", MoveWordRight, Some("Editor")),
        // 選択（Shift+矢印）
        KeyBinding::new("shift-up", SelectUp, Some("Editor")),
        KeyBinding::new("shift-down", SelectDown, Some("Editor")),
        KeyBinding::new("shift-left", SelectLeft, Some("Editor")),
        KeyBinding::new("shift-right", SelectRight, Some("Editor")),
        KeyBinding::new("shift-home", SelectToLineStart, Some("Editor")),
        KeyBinding::new("shift-end", SelectToLineEnd, Some("Editor")),
        KeyBinding::new("ctrl-shift-left", SelectWordLeft, Some("Editor")),
        KeyBinding::new("ctrl-shift-right", SelectWordRight, Some("Editor")),
        // 編集
        KeyBinding::new("backspace", Backspace, Some("Editor")),
        KeyBinding::new("delete", Delete, Some("Editor")),
        KeyBinding::new("enter", Enter, Some("Editor")),
        KeyBinding::new("tab", Tab, Some("Editor")),
        // ファイル操作
        KeyBinding::new("ctrl-s", Save, Some("Editor")),
        KeyBinding::new("ctrl-o", Open, Some("Editor")),
    ]);
}
