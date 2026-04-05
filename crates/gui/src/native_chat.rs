//! ネイティブ C コア（`oag_inference_*`）経由の Chat 推論（GGUF / ONNX）

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};
use std::path::Path;

use libc::{c_int, free};

#[repr(C)]
#[derive(Clone, Copy)]
struct oag_sampler_params_t {
    temperature: f32,
    top_p: f32,
    top_k: c_int,
    repeat_penalty: f32,
    repeat_window: c_int,
    seed: u64,
    min_p: f32,
}

#[repr(C)]
struct oag_chat_msg_t {
    role: *const c_char,
    content: *const c_char,
}

#[repr(C)]
struct oag_chat_params_t {
    messages: *mut oag_chat_msg_t,
    n_messages: c_int,
    sampler: oag_sampler_params_t,
    max_tokens: c_int,
    stream: bool,
    on_token: Option<extern "C" fn(*const c_char, *mut c_void)>,
    user_data: *mut c_void,
}

#[link(name = "oag_core", kind = "static")]
extern "C" {
    fn oag_inference_create(path: *const c_char) -> *mut c_void;
    fn oag_inference_free(inf: *mut c_void);
    fn oag_inference_chat(inf: *mut c_void, params: oag_chat_params_t) -> *mut c_char;
    fn gguf_get_last_error() -> *const c_char;
}

fn cstring_chat(s: &str) -> Result<CString, String> {
    if s.as_bytes().contains(&0) {
        let cleaned: String = s.chars().filter(|&c| c != '\0').collect();
        return CString::new(cleaned).map_err(|_| "文字列を変換できませんでした".to_string());
    }
    CString::new(s.as_bytes()).map_err(|_| "文字列を変換できませんでした".to_string())
}

struct InferenceGuard(*mut c_void);

impl InferenceGuard {
    fn ptr(&self) -> *mut c_void {
        self.0
    }
}

impl Drop for InferenceGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                oag_inference_free(self.0);
            }
        }
    }
}

/// `model_path` の GGUF/ONNX をロードしてチャット完成（同期・重い処理）
pub fn complete_native_chat_blocking(
    model_path: &Path,
    messages: &[(String, String)],
    temperature: f32,
    max_tokens: i32,
) -> Result<String, String> {
    let ext = model_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    if ext.eq_ignore_ascii_case("onnx") {
        return Err(
            "ネイティブ Chat は現状 GGUF のみ対応です。ONNX の場合は推論先を Ollama またはクラウド API に切り替えるか、GGUF に変換してください。"
                .into(),
        );
    }

    let path_str = model_path
        .to_str()
        .ok_or_else(|| "モデルパスが UTF-8 ではありません".to_string())?;
    let path_c = cstring_chat(path_str)?;

    let inf = unsafe { oag_inference_create(path_c.as_ptr()) };
    if inf.is_null() {
        let detail = unsafe {
            let p = gguf_get_last_error();
            if p.is_null() {
                String::new()
            } else {
                CStr::from_ptr(p).to_string_lossy().into_owned()
            }
        };
        if !detail.is_empty() {
            return Err(format!("ネイティブ推論の初期化に失敗: {detail}"));
        }
        return Err(
            "ネイティブ推論の初期化に失敗しました。GGUF のパス・破損・メモリを確認してください。"
                .into(),
        );
    }
    let guard = InferenceGuard(inf);

    let mut holders: Vec<(CString, CString)> = Vec::with_capacity(messages.len());
    for (role, content) in messages {
        holders.push((
            cstring_chat(role.as_str())?,
            cstring_chat(content.as_str())?,
        ));
    }

    let mut c_msgs: Vec<oag_chat_msg_t> = holders
        .iter()
        .map(|(r, c)| oag_chat_msg_t {
            role: r.as_ptr(),
            content: c.as_ptr(),
        })
        .collect();

    let sampler = oag_sampler_params_t {
        temperature,
        top_p: 0.9,
        top_k: 40,
        repeat_penalty: 1.1,
        repeat_window: 64,
        seed: 0,
        min_p: 0.05,
    };

    let params = oag_chat_params_t {
        messages: c_msgs.as_mut_ptr(),
        n_messages: c_msgs.len() as c_int,
        sampler,
        max_tokens: max_tokens.clamp(1, 8192),
        stream: false,
        on_token: None,
        user_data: std::ptr::null_mut(),
    };

    let out = unsafe { oag_inference_chat(guard.ptr(), params) };
    if out.is_null() {
        return Err("推論結果が NULL でした".to_string());
    }
    let text = unsafe { CStr::from_ptr(out) }
        .to_string_lossy()
        .into_owned();
    unsafe {
        free(out as *mut c_void);
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn onnx_path_rejected_before_loading() {
        let err = complete_native_chat_blocking(Path::new(r"C:\dummy\model.onnx"), &[], 0.7, 32)
            .unwrap_err();
        assert!(
            err.contains("GGUF"),
            "expected GGUF-only message, got: {err}"
        );
    }
}
