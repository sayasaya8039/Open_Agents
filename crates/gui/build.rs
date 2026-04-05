fn main() {
    // Compile C inference engine as static library
    let src_dir = "../../src";

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
        .define("OAG_VERSION", "\"0.2.28\"")
        .compile("oag_core");

    // Link Windows libraries (for C core)
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("windows") {
        println!("cargo:rustc-link-lib=dxgi");
        println!("cargo:rustc-link-lib=ws2_32");
    }
}
