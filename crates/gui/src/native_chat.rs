//! ネイティブ C コア（`oag_inference_*`）経由の Chat 推論（GGUF / ONNX）

use std::ffi::{CStr, CString};
use std::fs;
use std::os::raw::{c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

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
    fn oag_inference_create_with_ctx(path: *const c_char, context_length: c_int) -> *mut c_void;
    fn oag_inference_free(inf: *mut c_void);
    fn oag_inference_chat(inf: *mut c_void, params: oag_chat_params_t) -> *mut c_char;
    fn gguf_get_last_error() -> *const c_char;
}

const NATIVE_CONTEXT_LENGTH_MIN: i32 = 512;
const NATIVE_CONTEXT_LENGTH_MAX: i32 = 8192;
const LARGE_FULL_PRECISION_GGUF_BYTES: u64 = 8 * 1024 * 1024 * 1024;

fn cstring_chat(s: &str) -> Result<CString, String> {
    if s.as_bytes().contains(&0) {
        let cleaned: String = s.chars().filter(|&c| c != '\0').collect();
        return CString::new(cleaned).map_err(|_| "文字列を変換できませんでした".to_string());
    }
    CString::new(s.as_bytes()).map_err(|_| "文字列を変換できませんでした".to_string())
}

struct CachedInference {
    model_path: PathBuf,
    context_length: i32,
    ptr: *mut c_void,
}

unsafe impl Send for CachedInference {}

impl Drop for CachedInference {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe {
                oag_inference_free(self.ptr);
            }
        }
    }
}

fn native_inference_cache() -> &'static Mutex<Option<CachedInference>> {
    static CACHE: OnceLock<Mutex<Option<CachedInference>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

fn last_gguf_error_detail() -> String {
    unsafe {
        let p = gguf_get_last_error();
        if p.is_null() {
            String::new()
        } else {
            CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }
}

fn clamp_native_context_length(context_length: i32) -> i32 {
    context_length.clamp(NATIVE_CONTEXT_LENGTH_MIN, NATIVE_CONTEXT_LENGTH_MAX)
}

fn looks_full_precision_gguf(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    ["bf16", "fp16", "fp32", "f32", "f16"]
        .iter()
        .any(|needle| name.contains(needle))
}

fn validate_native_model_profile(path: &Path, size_bytes: u64) -> Result<(), String> {
    let is_gguf = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("gguf"))
        .unwrap_or(false);
    if is_gguf
        && size_bytes >= LARGE_FULL_PRECISION_GGUF_BYTES
        && looks_full_precision_gguf(path)
    {
        return Err(
            "選択中の GGUF は BF16/F16/F32 系の大型フル精度モデルです。現行のネイティブ Chat 実装ではメモリ消費が大きすぎて応答不能になりやすいため、Q4_K_M / Q5_K_M などの量子化 GGUF を使うか、Ollama 側で推論してください。"
                .into(),
        );
    }
    Ok(())
}

fn validate_native_model_path(path: &Path) -> Result<(), String> {
    let metadata = fs::metadata(path)
        .map_err(|e| format!("モデルファイルを確認できませんでした: {e}"))?;
    validate_native_model_profile(path, metadata.len())
}

fn with_cached_inference<T>(
    model_path: &Path,
    context_length: i32,
    f: impl FnOnce(*mut c_void) -> Result<T, String>,
) -> Result<T, String> {
    validate_native_model_path(model_path)?;
    let context_length = clamp_native_context_length(context_length);
    let mut cache = native_inference_cache()
        .lock()
        .map_err(|_| "ネイティブ推論キャッシュのロック取得に失敗しました".to_string())?;

    let reload_required = cache
        .as_ref()
        .map(|cached| cached.model_path != model_path || cached.context_length != context_length)
        .unwrap_or(true);

    if reload_required {
        let path_str = model_path
            .to_str()
            .ok_or_else(|| "モデルパスが UTF-8 ではありません".to_string())?;
        let path_c = cstring_chat(path_str)?;
        let inf = unsafe { oag_inference_create_with_ctx(path_c.as_ptr(), context_length as c_int) };
        if inf.is_null() {
            let detail = last_gguf_error_detail();
            if !detail.is_empty() {
                return Err(format!("ネイティブ推論の初期化に失敗: {detail}"));
            }
            return Err(
                "ネイティブ推論の初期化に失敗しました。GGUF のパス・量子化形式・メモリを確認してください。"
                    .into(),
            );
        }
        *cache = Some(CachedInference {
            model_path: model_path.to_path_buf(),
            context_length,
            ptr: inf,
        });
    }

    let inf = cache
        .as_ref()
        .map(|cached| cached.ptr)
        .ok_or_else(|| "ネイティブ推論インスタンスの取得に失敗しました".to_string())?;
    f(inf)
}

/// `model_path` の GGUF/ONNX をロードしてチャット完成（同期・重い処理）
pub fn complete_native_chat_blocking(
    model_path: &Path,
    messages: &[(String, String)],
    temperature: f32,
    max_tokens: i32,
    context_length: i32,
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

    with_cached_inference(model_path, context_length, |inf| {
        let out = unsafe { oag_inference_chat(inf, params) };
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn onnx_path_rejected_before_loading() {
        let err =
            complete_native_chat_blocking(Path::new(r"C:\dummy\model.onnx"), &[], 0.7, 32, 4096)
                .unwrap_err();
        assert!(
            err.contains("GGUF"),
            "expected GGUF-only message, got: {err}"
        );
    }

    #[test]
    fn clamps_requested_native_context_length() {
        assert_eq!(clamp_native_context_length(0), 512);
        assert_eq!(clamp_native_context_length(4096), 4096);
        assert_eq!(clamp_native_context_length(999_999), 8192);
    }

    #[test]
    fn rejects_large_bf16_gguf_with_actionable_message() {
        let err = validate_native_model_profile(
            Path::new(r"D:\models\gemma-4-E4B-it-BF16.gguf"),
            14 * 1024 * 1024 * 1024,
        )
        .unwrap_err();
        assert!(
            err.contains("量子化"),
            "expected quantization guidance, got: {err}"
        );
    }

    #[test]
    fn accepts_quantized_gguf_profile() {
        let ok = validate_native_model_profile(
            Path::new(r"D:\models\qwen2.5-coder-7b-q4_k_m.gguf"),
            4 * 1024 * 1024 * 1024,
        );
        assert!(ok.is_ok(), "expected quantized gguf to be accepted: {ok:?}");
    }
}
