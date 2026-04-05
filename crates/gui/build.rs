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
        ])
        .include(src_dir)
        .std("c11")
        .opt_level(3)
        .flag_if_supported("-mavx2")
        .flag_if_supported("-mfma")
        .flag_if_supported("-mf16c")
        .flag_if_supported("/arch:AVX2")
        .define("NDEBUG", None)
        .define("OAG_HAS_CPU_BACKEND", "1")
        .define("OAG_VERSION", "\"0.2.0\"")
        .compile("oag_core");

    // Link Windows libraries (for C core)
    println!("cargo:rustc-link-lib=ws2_32");
}
