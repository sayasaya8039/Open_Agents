use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    // Compile C inference engine as static library
    let src_dir = "../../src";
    let bundled_runtime_dir = Path::new("../../third_party/llama.cpp/windows-x64");

    // --- rerun-if-changed: C ソースファイル ---
    let c_sources = [
        format!("{}/core/gguf.c", src_dir),
        format!("{}/core/tensor.c", src_dir),
        format!("{}/core/model.c", src_dir),
        format!("{}/core/tokenizer.c", src_dir),
        format!("{}/core/sampler.c", src_dir),
        format!("{}/core/inference.c", src_dir),
        format!("{}/core/onnx_loader.c", src_dir),
        format!("{}/backend/cpu.c", src_dir),
        format!("{}/backend/gpu_detect.c", src_dir),
        format!("{}/backend/cuda_backend.c", src_dir),
        format!("{}/backend/directml_backend.c", src_dir),
        format!("{}/backend/npu_backend.c", src_dir),
    ];
    for src in &c_sources {
        println!("cargo:rerun-if-changed={}", src);
    }

    // --- rerun-if-changed: ヘッダファイル ---
    let headers = [
        format!("{}/core/gguf.h", src_dir),
        format!("{}/core/tensor.h", src_dir),
        format!("{}/core/model.h", src_dir),
        format!("{}/core/tokenizer.h", src_dir),
        format!("{}/core/sampler.h", src_dir),
        format!("{}/core/inference.h", src_dir),
        format!("{}/core/onnx_loader.h", src_dir),
        format!("{}/backend/backend.h", src_dir),
    ];
    for hdr in &headers {
        println!("cargo:rerun-if-changed={}", hdr);
    }

    println!("cargo:rerun-if-changed={}", bundled_runtime_dir.display());
    sync_bundled_llama_runtime(bundled_runtime_dir);

    let mut build = cc::Build::new();
    build
        .files(&c_sources)
        .include(src_dir)
        .std("c11")
        .opt_level(3)
        .warnings(false); // コンパイル高速化: 警告解析をスキップ

    // zig cc 対応: ZIG_CC 環境変数 or CC 環境変数で zig cc を指定可能
    // 例: ZIG_CC="zig cc" cargo zigbuild --release -p open-agents-gui
    if let Ok(zig_cc) = env::var("ZIG_CC") {
        build.compiler(&zig_cc);
    }

    build
        .flag_if_supported("-mavx2")
        .flag_if_supported("-mfma")
        .flag_if_supported("-mf16c")
        .flag_if_supported("/arch:AVX2")
        // UTF-8 ソース（gpu_detect 等の記号リテラル）を MSVC が正しく解釈する
        .flag_if_supported("/utf-8")
        .define("NDEBUG", None)
        .define("OAG_HAS_CPU_BACKEND", "1")
        .define("OAG_VERSION", "\"0.3.6\"")
        .compile("oag_core");

    // Link Windows libraries (for C core)
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("windows") {
        println!("cargo:rustc-link-lib=dxgi");
        println!("cargo:rustc-link-lib=ws2_32");

        // Embed application icon into the executable
        let mut res = winresource::WindowsResource::new();
        res.set_icon("open_agents.ico");
        res.compile().expect("failed to compile Windows resource (icon)");
    }
}

fn sync_bundled_llama_runtime(src_dir: &Path) {
    if !src_dir.is_dir() {
        panic!(
            "bundled llama.cpp runtime directory not found: {}",
            src_dir.display()
        );
    }
    let profile_dir = profile_output_dir();
    fs::create_dir_all(&profile_dir).expect("failed to create cargo profile output directory");

    copy_runtime_tree(src_dir, src_dir, &profile_dir);
}

fn copy_runtime_tree(root: &Path, current: &Path, dst_root: &Path) {
    for entry in fs::read_dir(current).expect("failed to read bundled llama runtime directory") {
        let entry = entry.expect("failed to read bundled llama runtime entry");
        let src_path = entry.path();
        println!("cargo:rerun-if-changed={}", src_path.display());
        if src_path.is_dir() {
            copy_runtime_tree(root, &src_path, dst_root);
            continue;
        }

        let relative = src_path
            .strip_prefix(root)
            .expect("failed to compute bundled runtime relative path");
        let dst_path = dst_root.join(relative);
        if let Some(parent) = dst_path.parent() {
            fs::create_dir_all(parent)
                .expect("failed to create bundled llama runtime destination directory");
        }
        match copy_or_link(&src_path, &dst_path) {
            Ok(()) => {}
            Err(err) => {
                panic!(
                    "failed to copy/link bundled llama runtime {} -> {}: {err}",
                    src_path.display(),
                    dst_path.display()
                )
            }
        }
    }
}

/// ハードリンク優先、失敗時コピーフォールバック。
/// 既存ファイルが src と一致していればスキップ。
fn copy_or_link(src: &Path, dst: &Path) -> Result<(), std::io::Error> {
    if dst.exists() {
        if files_match(src, dst).unwrap_or(false) {
            return Ok(());
        }
        // 古いファイルを除去（ロック時は警告のみ）
        if let Err(e) = fs::remove_file(dst) {
            // ファイルがロックされている場合、内容一致なら無視
            println!(
                "cargo:warning=bundled llama runtime: could not remove {}: {e}, skipping",
                dst.display()
            );
            return Ok(());
        }
    }
    // ハードリンク優先（同一ドライブならゼロコピー）
    match fs::hard_link(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => fs::copy(src, dst).map(|_| ()),
    }
}

fn files_match(src_path: &Path, dst_path: &Path) -> Result<bool, std::io::Error> {
    let src_meta = fs::metadata(src_path)?;
    let dst_meta = fs::metadata(dst_path)?;
    if src_meta.len() != dst_meta.len() {
        return Ok(false);
    }
    let src_modified = src_meta.modified()?;
    let dst_modified = dst_meta.modified()?;
    Ok(src_modified == dst_modified)
}

fn profile_output_dir() -> PathBuf {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is not set"));
    out_dir
        .ancestors()
        .nth(3)
        .expect("failed to resolve cargo profile output directory")
        .to_path_buf()
}
