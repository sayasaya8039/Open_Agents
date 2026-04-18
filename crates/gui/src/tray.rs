//! Windows システムトレイ統合
//!
//! - 最小化時に自動的にウィンドウを隠しトレイアイコンに格納
//! - トレイアイコン左クリック/ダブルクリックでウィンドウ復元
//! - 右クリックで「表示」「終了」コンテキストメニュー
//! - Explorer 再起動時（TaskbarCreated）にアイコン再登録

#![cfg(windows)]

use std::sync::atomic::{AtomicU32, Ordering};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass, Shell_NotifyIconW, NIF_ICON,
    NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, IsIconic, LoadIconW, PostMessageW,
    SetForegroundWindow, ShowWindow, TrackPopupMenu, IDI_APPLICATION, MF_SEPARATOR, MF_STRING,
    SIZE_MINIMIZED, SW_HIDE, SW_RESTORE, SW_SHOW, TPM_LEFTALIGN, TPM_RIGHTBUTTON, WM_APP,
    WM_COMMAND, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_NCDESTROY, WM_RBUTTONUP, WM_SIZE,
};

const WM_TRAYICON: u32 = WM_APP + 1;
const ID_TRAY_SHOW: u32 = 1001;
const ID_TRAY_QUIT: u32 = 1002;
const TRAY_UID: u32 = 0xA9E9;
const SUBCLASS_ID: usize = 0xA9E9A1;

static TASKBAR_CREATED_MSG: AtomicU32 = AtomicU32::new(0);

/// 指定された HWND にサブクラスを仕込み、トレイアイコンを登録する。
pub fn install(hwnd: HWND) -> Result<(), String> {
    unsafe {
        let msg_id = register_taskbar_created_msg();
        TASKBAR_CREATED_MSG.store(msg_id, Ordering::Relaxed);

        if !SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, 0).as_bool() {
            return Err("SetWindowSubclass に失敗".to_string());
        }

        add_tray_icon(hwnd).map_err(|e| format!("tray add failed: {e}"))?;
    }
    Ok(())
}

unsafe fn register_taskbar_created_msg() -> u32 {
    use windows::Win32::UI::WindowsAndMessaging::RegisterWindowMessageW;
    let wide: Vec<u16> = "TaskbarCreated\0".encode_utf16().collect();
    RegisterWindowMessageW(PCWSTR(wide.as_ptr()))
}

unsafe fn add_tray_icon(hwnd: HWND) -> windows::core::Result<()> {
    // 実行ファイルに埋め込まれたアイコン（.rc の "1 ICON"）をロード
    let instance = GetModuleHandleW(PCWSTR::null())?;
    // リソース ID 1 の埋め込みアイコン。失敗時は標準アプリアイコンにフォールバック。
    let hicon = LoadIconW(instance, PCWSTR(1 as *const u16))
        .or_else(|_| LoadIconW(None, IDI_APPLICATION))?;

    let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_UID;
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    nid.uCallbackMessage = WM_TRAYICON;
    nid.hIcon = hicon;

    let tip: Vec<u16> = "Open Agents\0".encode_utf16().collect();
    let copy_len = tip.len().min(nid.szTip.len());
    nid.szTip[..copy_len].copy_from_slice(&tip[..copy_len]);

    if !Shell_NotifyIconW(NIM_ADD, &nid).as_bool() {
        return Err(windows::core::Error::from_win32());
    }
    Ok(())
}

unsafe fn remove_tray_icon(hwnd: HWND) {
    let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_UID;
    let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
}

unsafe fn restore_window(hwnd: HWND) {
    let _ = ShowWindow(hwnd, SW_SHOW);
    if IsIconic(hwnd).as_bool() {
        let _ = ShowWindow(hwnd, SW_RESTORE);
    }
    let _ = SetForegroundWindow(hwnd);
}

unsafe fn show_context_menu(hwnd: HWND) {
    let Ok(menu) = CreatePopupMenu() else {
        return;
    };

    let show_label: Vec<u16> = "表示\0".encode_utf16().collect();
    let quit_label: Vec<u16> = "終了\0".encode_utf16().collect();
    let _ = AppendMenuW(
        menu,
        MF_STRING,
        ID_TRAY_SHOW as usize,
        PCWSTR(show_label.as_ptr()),
    );
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(
        menu,
        MF_STRING,
        ID_TRAY_QUIT as usize,
        PCWSTR(quit_label.as_ptr()),
    );

    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);

    // Windows 標準: TrackPopupMenu 前にフォアグラウンドへ。
    let _ = SetForegroundWindow(hwnd);
    let _ = TrackPopupMenu(
        menu,
        TPM_RIGHTBUTTON | TPM_LEFTALIGN,
        pt.x,
        pt.y,
        0,
        hwnd,
        None,
    );
    let _ = DestroyMenu(menu);
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    _ref_data: usize,
) -> LRESULT {
    // 最小化 → ウィンドウを隠してトレイに格納
    if msg == WM_SIZE && wparam.0 as u32 == SIZE_MINIMIZED {
        let _ = ShowWindow(hwnd, SW_HIDE);
        return LRESULT(0);
    }

    // トレイアイコンからのコールバック
    if msg == WM_TRAYICON {
        let event = (lparam.0 as u32) & 0xFFFF;
        match event {
            WM_LBUTTONUP | WM_LBUTTONDBLCLK => {
                restore_window(hwnd);
                return LRESULT(0);
            }
            WM_RBUTTONUP => {
                show_context_menu(hwnd);
                return LRESULT(0);
            }
            _ => {}
        }
    }

    // トレイメニュー項目
    if msg == WM_COMMAND && ((wparam.0 >> 16) & 0xFFFF) == 0 {
        let id = (wparam.0 as u32) & 0xFFFF;
        match id {
            ID_TRAY_SHOW => {
                restore_window(hwnd);
                return LRESULT(0);
            }
            ID_TRAY_QUIT => {
                remove_tray_icon(hwnd);
                use windows::Win32::UI::WindowsAndMessaging::WM_CLOSE;
                let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
                return LRESULT(0);
            }
            _ => {}
        }
    }

    // Explorer 再起動時に通知エリアを再構築
    let taskbar_msg = TASKBAR_CREATED_MSG.load(Ordering::Relaxed);
    if taskbar_msg != 0 && msg == taskbar_msg {
        let _ = add_tray_icon(hwnd);
    }

    if msg == WM_NCDESTROY {
        remove_tray_icon(hwnd);
        let _ = RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID);
    }

    DefSubclassProc(hwnd, msg, wparam, lparam)
}
