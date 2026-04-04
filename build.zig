const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});

    // ========== Core C Sources ==========
    const c_sources = &[_][]const u8{
        "src/main.c",
        "src/core/gguf.c",
        "src/core/tensor.c",
        "src/core/model.c",
        "src/core/tokenizer.c",
        "src/core/sampler.c",
        "src/core/inference.c",
        "src/core/onnx_loader.c",
        "src/backend/cpu.c",
        "src/server/http_server.c",
        "src/net/multi_nic.c",
        "src/backend/gpu_detect.c",
        "src/backend/cuda_backend.c",
        "src/backend/directml_backend.c",
        "src/backend/npu_backend.c",
        "src/backend/mojo_backend.c",
        "src/backend/julia_backend.c",
        "src/render/vulkan_render.c",
        "src/provider/api_provider.c",
        "src/ui/tui.c",
        "src/gui/win32_app.c",
    };

    const c_flags = &[_][]const u8{
        "-std=c11",
        "-O3",
        "-mavx2",
        "-mfma",
        "-mf16c",
        "-DNDEBUG",
        "-DOAG_VERSION=\"0.2.0\"",
        "-DOAG_HAS_CPU_BACKEND=1",
    };

    // Create root module (Zig 0.15+ API)
    const root_module = b.createModule(.{
        .target = target,
        .optimize = optimize,
        .link_libc = true,
    });

    const exe = b.addExecutable(.{
        .name = "open_agents",
        .root_module = root_module,
    });

    exe.addCSourceFiles(.{
        .files = c_sources,
        .flags = c_flags,
    });

    exe.addIncludePath(b.path("src"));

    // Windows: link networking + IP helper libs
    exe.linkSystemLibrary("ws2_32");
    exe.linkSystemLibrary("iphlpapi");
    exe.linkSystemLibrary("winhttp");
    exe.linkSystemLibrary("dxgi");
    exe.linkSystemLibrary("d3d12");
    exe.linkSystemLibrary("ole32");
    exe.linkSystemLibrary("gdi32");
    exe.linkSystemLibrary("shell32");
    exe.linkSystemLibrary("user32");

    b.installArtifact(exe);

    // ========== Run step ==========
    const run_cmd = b.addRunArtifact(exe);
    run_cmd.step.dependOn(b.getInstallStep());
    if (b.args) |args| {
        run_cmd.addArgs(args);
    }

    const run_step = b.step("run", "Run Open_Agents");
    run_step.dependOn(&run_cmd.step);
}
