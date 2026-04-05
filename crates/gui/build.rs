use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    // Compile C inference engine as static library
    let src_dir = "../../src";
    let bundled_runtime_dir = Path::new("../../third_party/llama.cpp/windows-x64");

    println!("cargo:rerun-if-changed={}", bundled_runtime_dir.display());
    sync_bundled_llama_runtime(bundled_runtime_dir);

    cc::Build::new()
        .files(&[
            format!("{}/core/gguf.c", src_dir),
            format!("{}/core/tensor.c", src_dir),
            format!("{}/core/model.c", src_dir),
            format!("{}/core/tokenizer.c", src_dir),
            format!("{}/core/sampler.c", src_dir),
            format!("{}/core/inference.c", src_dir),
            format!("{}/core/onnx_loader.c", src_dir),
            format!("{}/backend/cpu.c", src_dir),
            // cpu.c レジストリが参照するシンボル（GPU 検出・各 vtable）
            format!("{}/backend/gpu_detect.c", src_dir),
            format!("{}/backend/cuda_backend.c", src_dir),
            format!("{}/backend/directml_backend.c", src_dir),
            format!("{}/backend/npu_backend.c", src_dir),
        ])
        .include(src_dir)
        .std("c11")
        .opt_level(3)
        .flag_if_supported("-mavx2")
        .flag_if_supported("-mfma")
        .flag_if_supported("-mf16c")
        .flag_if_supported("/arch:AVX2")
        // UTF-8 ソース（gpu_detect 等の記号リテラル）を MSVC が正しく解釈する
        .flag_if_supported("/utf-8")
        .define("NDEBUG", None)
        .define("OAG_HAS_CPU_BACKEND", "1")
        .define("OAG_VERSION", "\"0.2.46\"")
        .compile("oag_core");

    // Link Windows libraries (for C core)
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("windows") {
        println!("cargo:rustc-link-lib=dxgi");
        println!("cargo:rustc-link-lib=ws2_32");
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

    for entry in fs::read_dir(src_dir).expect("failed to read bundled llama runtime directory") {
        let entry = entry.expect("failed to read bundled llama runtime entry");
        let src_path = entry.path();
        if !src_path.is_file() {
            continue;
        }
        println!("cargo:rerun-if-changed={}", src_path.display());
        let dst_path = profile_dir.join(entry.file_name());
        fs::copy(&src_path, &dst_path).unwrap_or_else(|err| {
            panic!(
                "failed to copy bundled llama runtime {} -> {}: {err}",
                src_path.display(),
                dst_path.display()
            )
        });
    }
}

fn profile_output_dir() -> PathBuf {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is not set"));
    out_dir
        .ancestors()
        .nth(3)
        .expect("failed to resolve cargo profile output directory")
        .to_path_buf()
}
