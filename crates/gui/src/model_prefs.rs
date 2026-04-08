//! モデル・ハードウェア・外観・AI 設定を永続化する。
//! 保存先: `%LOCALAPPDATA%\\open_agents_gui\\model_params.json`
//!
//! JSON はルートにモデル用キー（flatten）とオプションの `hardware` / `appearance`
//! / `ai` オブジェクト、および `model_paths`（読み込み済みモデル一覧）を持つ。
//! 旧形式の単一 `model_path` は読み込み時に `model_paths` へ移行される。
//! 以前のフラットな `model_params.json`（モデルのみ）も `load_local_llm_prefs` で読み込み可能。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ModelParams {
    /// サンプリング温度（`oag_sampler_params_t::temperature` に相当）0.0〜2.0
    pub temperature: f32,
    /// 1 応答あたりの最大生成トークン（`oag_chat_params_t::max_tokens`）
    pub max_output_tokens: i32,
    /// プロンプト・会話で確保するコンテキスト長（トークン；KV・メモリ設計の目安）
    pub context_length: i32,
}

pub const DEFAULT_MAX_OUTPUT_TOKENS: i32 = 4096;
pub const LOCAL_GGUF_MAX_OUTPUT_TOKENS_CAP: i32 = 8192;

/// ローカル GGUF 推論で推奨されるコンテキスト長上限（KV cache メモリ / レイテンシ最適化）
/// 256K 対応モデルでも KV cache が VRAM を圧迫し推論速度が低下するため 16384 に制限する。
pub const LOCAL_CONTEXT_LENGTH_CAP: i32 = 16384;
/// ローカル推論のデフォルトコンテキスト長（8192 が速度と品質のバランス点）
pub const LOCAL_CONTEXT_LENGTH_DEFAULT: i32 = 8192;

pub fn effective_local_max_output_tokens(max_output_tokens: i32) -> i32 {
    if max_output_tokens <= 0 {
        return DEFAULT_MAX_OUTPUT_TOKENS;
    }
    max_output_tokens.clamp(DEFAULT_MAX_OUTPUT_TOKENS, LOCAL_GGUF_MAX_OUTPUT_TOKENS_CAP)
}

impl Default for ModelParams {
    fn default() -> Self {
        Self {
            temperature: 0.7,
            max_output_tokens: DEFAULT_MAX_OUTPUT_TOKENS,
            context_length: LOCAL_CONTEXT_LENGTH_DEFAULT,
        }
    }
}

impl ModelParams {
    pub fn clamp(&mut self) {
        self.temperature = self.temperature.clamp(0.0, 2.0);
        self.max_output_tokens = self.max_output_tokens.clamp(256, 8192);
        self.context_length = self.context_length.clamp(512, LOCAL_CONTEXT_LENGTH_CAP);
    }

    pub fn sanitize(mut self) -> Self {
        if !self.temperature.is_finite() {
            self.temperature = Self::default().temperature;
        }
        if self.max_output_tokens <= 0 {
            self.max_output_tokens = Self::default().max_output_tokens;
        }
        if self.context_length <= 0 {
            self.context_length = Self::default().context_length;
        }
        self.clamp();
        self
    }
}

/// KV キャッシュ量子化モード（TurboQuant 対応 llama-server 用）
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum KvCacheType {
    /// 圧縮なし（FP16、従来動作）
    #[default]
    None,
    /// q8_0: 2x 圧縮、品質劣化ほぼゼロ
    Q8,
    /// q4_0: 4x 圧縮、軽微な品質劣化
    Q4,
    /// TurboQuant turbo3: 4.9x 圧縮、+1% perplexity（推奨）
    Turbo3,
    /// TurboQuant turbo4: 3.8x 圧縮、+0.2% perplexity（高品質）
    Turbo4,
    /// TurboQuant turbo2: 6.4x 圧縮、+6.5% perplexity（最大圧縮）
    Turbo2,
}

impl KvCacheType {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "FP16（標準）",
            Self::Q8 => "Q8（2x圧縮）",
            Self::Q4 => "Q4（4x圧縮）",
            Self::Turbo3 => "Turbo3（4.9x圧縮・推奨）",
            Self::Turbo4 => "Turbo4（3.8x圧縮・高品質）",
            Self::Turbo2 => "Turbo2（6.4x圧縮・最大）",
        }
    }
}

/// ローカル GGUF 実行に使う runtime プリセット
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LlamaRuntimePreset {
    /// 起動時に GPU を自動検出し、最適な runtime を選択する（推奨）
    #[default]
    Auto,
    /// CUDA 版を使う通常モード（NVIDIA GPU 向け）
    #[serde(alias = "high_performance_4090")]
    NvidiaCuda,
    /// Vulkan 版で複数 GPU の混成を試す実験モード
    #[serde(alias = "experimental_hybrid_4090_arc")]
    VulkanHybrid,
    /// CPU runtime 優先の省電力モード（GPU なし / NPU 環境向け）
    #[serde(alias = "intel_npu_efficient")]
    CpuOnly,
}

impl LlamaRuntimePreset {
    pub const ALL: [Self; 4] = [
        Self::Auto,
        Self::NvidiaCuda,
        Self::VulkanHybrid,
        Self::CpuOnly,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "自動検出（推奨）",
            Self::NvidiaCuda => "NVIDIA CUDA",
            Self::VulkanHybrid => "Vulkan 混成（実験）",
            Self::CpuOnly => "CPU のみ",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Self::Auto => "起動時に GPU を検出して最適な設定を自動適用",
            Self::NvidiaCuda => "CUDA 単独で速度を優先（NVIDIA GPU 必須）",
            Self::VulkanHybrid => "Vulkan で複数 GPU の混成を試す",
            Self::CpuOnly => "GPU を使わず CPU のみで推論する省電力モード",
        }
    }

    pub fn is_experimental(self) -> bool {
        matches!(self, Self::VulkanHybrid)
    }
}

// ── GPU 自動検出プロファイル ──

/// 検出された GPU のベンダー
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Unknown,
}

/// 検出された GPU 情報（DXGI 由来）
#[derive(Clone, Debug)]
pub struct DetectedGpu {
    pub name: String,
    pub vendor: GpuVendor,
    pub vram_bytes: u64,
    pub is_discrete: bool,
}

impl DetectedGpu {
    pub fn vram_gb(&self) -> f64 {
        self.vram_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
    }
}

/// GPU 検出結果に基づく最適化プロファイル
#[derive(Clone, Debug)]
pub struct GpuProfile {
    pub gpus: Vec<DetectedGpu>,
    pub best_gpu: Option<DetectedGpu>,
    pub resolved_preset: LlamaRuntimePreset,
    pub recommended_gpu_layers: i32,
    pub recommended_threads: i32,
    pub recommended_batch_size: i32,
    pub summary: String,
}

/// DXGI で GPU を列挙し、最適なハードウェア設定を決定する
#[cfg(windows)]
pub fn detect_gpu_profile() -> GpuProfile {
    // DXGI COM 経由で GPU 列挙
    let gpus = enumerate_dxgi_gpus();
    build_profile_from_gpus(gpus)
}

#[cfg(not(windows))]
pub fn detect_gpu_profile() -> GpuProfile {
    build_profile_from_gpus(Vec::new())
}

fn build_profile_from_gpus(gpus: Vec<DetectedGpu>) -> GpuProfile {
    let cpu_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8) as i32;

    // ベスト GPU を選択（VRAM 最大の discrete GPU、なければ integrated）
    let best_gpu = gpus
        .iter()
        .filter(|g| g.is_discrete)
        .max_by_key(|g| g.vram_bytes)
        .or_else(|| gpus.iter().max_by_key(|g| g.vram_bytes))
        .cloned();

    let (resolved_preset, gpu_layers, threads, batch_size, summary) = match &best_gpu {
        Some(gpu) if gpu.vendor == GpuVendor::Nvidia => {
            let vram_gb = gpu.vram_gb();
            // VRAM 量に応じてレイヤー数を調整
            let layers = if vram_gb >= 20.0 { 99 }      // RTX 4090 / 3090 (24GB)
                else if vram_gb >= 10.0 { 60 }           // RTX 4080 / 3070 Ti (12-16GB)
                else if vram_gb >= 6.0 { 35 }            // RTX 3060 / 4060 (8GB)
                else { 20 };                              // GTX 1660 / low-end (4-6GB)
            let batch = if vram_gb >= 16.0 { 1024 } else { 512 };
            let threads = (cpu_count / 2).max(4).min(16);
            (
                LlamaRuntimePreset::NvidiaCuda,
                layers,
                threads,
                batch,
                format!("{} ({:.0}GB VRAM) — CUDA, {} layers", gpu.name, vram_gb, layers),
            )
        }
        Some(gpu) if gpu.vendor == GpuVendor::Amd => {
            let vram_gb = gpu.vram_gb();
            // AMD は Vulkan 経由（DirectML は llama-server 非対応のため）
            let layers = if vram_gb >= 12.0 { 50 }
                else if vram_gb >= 8.0 { 35 }
                else { 20 };
            let batch = 512;
            let threads = (cpu_count / 2).max(4).min(16);
            (
                LlamaRuntimePreset::VulkanHybrid,
                layers,
                threads,
                batch,
                format!("{} ({:.0}GB VRAM) — Vulkan, {} layers", gpu.name, vram_gb, layers),
            )
        }
        Some(gpu) if gpu.vendor == GpuVendor::Intel && gpu.is_discrete => {
            let vram_gb = gpu.vram_gb();
            let layers = if vram_gb >= 12.0 { 40 } else { 25 };
            let batch = 512;
            let threads = (cpu_count / 2).max(4).min(16);
            (
                LlamaRuntimePreset::VulkanHybrid,
                layers,
                threads,
                batch,
                format!("{} ({:.0}GB VRAM) — Vulkan, {} layers", gpu.name, vram_gb, layers),
            )
        }
        _ => {
            // GPU なしまたは integrated のみ → CPU モード
            let threads = cpu_count.max(4).min(16);
            let batch = 512;
            (
                LlamaRuntimePreset::CpuOnly,
                0,
                threads,
                batch,
                "GPU 未検出 — CPU のみ".to_string(),
            )
        }
    };

    GpuProfile {
        gpus,
        best_gpu,
        resolved_preset,
        recommended_gpu_layers: gpu_layers,
        recommended_threads: threads,
        recommended_batch_size: batch_size,
        summary,
    }
}

/// DXGI で GPU を列挙する（Windows 専用）
#[cfg(windows)]
fn enumerate_dxgi_gpus() -> Vec<DetectedGpu> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    #[link(name = "dxgi")]
    extern "system" {
        fn CreateDXGIFactory1(riid: *const [u8; 16], factory: *mut *mut u8) -> i32;
    }

    // IDXGIFactory1 の IID: {770aae78-f26f-4dba-a829-253c83d1b387}
    const IID_IDXGI_FACTORY1: [u8; 16] = [
        0x78, 0xae, 0x0a, 0x77, 0x6f, 0xf2, 0xba, 0x4d,
        0xa8, 0x29, 0x25, 0x3c, 0x83, 0xd1, 0xb3, 0x87,
    ];

    let mut factory: *mut u8 = std::ptr::null_mut();
    let hr = unsafe { CreateDXGIFactory1(&IID_IDXGI_FACTORY1, &mut factory) };
    if hr < 0 || factory.is_null() {
        eprintln!("[GPU Detect] DXGI Factory 作成失敗 (hr={hr:#x})");
        return Vec::new();
    }

    // COM vtable: IUnknown(3) + IDXGIObject(4) + IDXGIFactory(4) + IDXGIFactory1(1)
    // EnumAdapters1 は IDXGIFactory1 の最初のメソッド = vtable[12]
    let vtable = unsafe { *(factory as *const *const usize) };

    let mut gpus = Vec::new();

    for i in 0u32.. {
        let mut adapter: *mut u8 = std::ptr::null_mut();
        let enum_fn: extern "system" fn(*mut u8, u32, *mut *mut u8) -> i32 =
            unsafe { std::mem::transmute(*vtable.add(12)) };
        let hr = enum_fn(factory, i, &mut adapter);
        if hr < 0 || adapter.is_null() {
            break;
        }

        // IDXGIAdapter1::GetDesc1 は vtable[10] (IUnknown(3) + IDXGIObject(4) + IDXGIAdapter(3))
        // DXGI_ADAPTER_DESC1: Description[128 wchar] + VendorId + DeviceId + SubSysId + Revision
        //   + DedicatedVideoMemory + DedicatedSystemMemory + SharedSystemMemory + AdapterLuid + Flags
        #[repr(C)]
        struct DxgiAdapterDesc1 {
            description: [u16; 128],
            vendor_id: u32,
            device_id: u32,
            sub_sys_id: u32,
            revision: u32,
            dedicated_video_memory: usize,
            dedicated_system_memory: usize,
            shared_system_memory: usize,
            adapter_luid_low: u32,
            adapter_luid_high: i32,
            flags: u32,
        }

        let adapter_vtable = unsafe { *(adapter as *const *const usize) };
        let get_desc1: extern "system" fn(*mut u8, *mut DxgiAdapterDesc1) -> i32 =
            unsafe { std::mem::transmute(*adapter_vtable.add(10)) };

        let mut desc = unsafe { std::mem::zeroed::<DxgiAdapterDesc1>() };
        let hr = get_desc1(adapter, &mut desc);

        // Release adapter
        let release: extern "system" fn(*mut u8) -> u32 =
            unsafe { std::mem::transmute(*adapter_vtable.add(2)) };
        release(adapter);

        if hr < 0 {
            continue;
        }

        // ソフトウェアアダプタはスキップ
        const DXGI_ADAPTER_FLAG_SOFTWARE: u32 = 2;
        if desc.flags & DXGI_ADAPTER_FLAG_SOFTWARE != 0 {
            continue;
        }

        let name_len = desc.description.iter().position(|&c| c == 0).unwrap_or(128);
        let name = OsString::from_wide(&desc.description[..name_len])
            .to_string_lossy()
            .to_string();

        // Virtual / Basic Render をスキップ
        if name.contains("Microsoft Basic") || name.contains("Virtual") || name.contains("Parsec") {
            continue;
        }

        let vram_bytes = desc.dedicated_video_memory as u64;
        let vendor = match desc.vendor_id {
            0x10DE => GpuVendor::Nvidia,
            0x1002 => GpuVendor::Amd,
            0x8086 => GpuVendor::Intel,
            _ => GpuVendor::Unknown,
        };
        let is_discrete = match vendor {
            GpuVendor::Intel => vram_bytes >= 2 * 1024 * 1024 * 1024, // Intel Arc
            _ => vram_bytes >= 1024 * 1024 * 1024,
        };

        gpus.push(DetectedGpu {
            name,
            vendor,
            vram_bytes,
            is_discrete,
        });
    }

    // Release factory
    let factory_vtable = unsafe { *(factory as *const *const usize) };
    let release: extern "system" fn(*mut u8) -> u32 =
        unsafe { std::mem::transmute(*factory_vtable.add(2)) };
    release(factory);

    gpus
}

/// GpuProfile に基づいて HardwareParams を自動調整する（Auto モード時のみ）
pub fn apply_gpu_profile(hw: &mut HardwareParams, profile: &GpuProfile) {
    hw.gpu_layers = profile.recommended_gpu_layers;
    hw.n_threads = profile.recommended_threads;
    hw.batch_size = profile.recommended_batch_size;
    if profile.recommended_gpu_layers > 0 {
        hw.gpu_acceleration = true;
    } else {
        hw.gpu_acceleration = false;
    }
    eprintln!("[GPU Auto] {}", profile.summary);
}

/// 電源モード（ラップトップ特有: TGP 最大化のための設定）
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PowerMode {
    /// バランスモード（デフォルト: バッテリ寿命と性能のバランス）
    #[default]
    Balanced,
    /// 最大性能モード（外部電源接続 + 冷却強化前提）
    /// スレッド数・バッチサイズを最大化し、TGP をフルに活用する
    MaxPerformance,
}

impl PowerMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Balanced => "バランス",
            Self::MaxPerformance => "最大性能（AC接続推奨）",
        }
    }
}

/// ローカル推論のハードウェア関連（内蔵 llama-server 起動時の CLI にそのまま反映）
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct HardwareParams {
    /// llama.cpp runtime の実行プリセット
    pub llama_runtime_preset: LlamaRuntimePreset,
    /// オフのとき常に `--n-gpu-layers 0`。オンかつ GPU 対応同梱時は `gpu_layers` が渡る。
    #[serde(default = "default_gpu_acceleration")]
    pub gpu_acceleration: bool,
    /// `--n-gpu-layers`（Vulkan / CUDA / DirectML 等の llama-server 同梱時に有効）
    pub gpu_layers: i32,
    /// CPU スレッド数（`--threads`）
    pub n_threads: i32,
    /// 推論バッチサイズ（バックエンドが参照する場合の目安）
    pub batch_size: i32,
    /// KV キャッシュ量子化（TurboQuant 対応版で有効）
    pub kv_cache_type: KvCacheType,
    /// 電源モード（MaxPerformance で全コア・大バッチを使用）
    #[serde(default)]
    pub power_mode: PowerMode,
}

impl Default for HardwareParams {
    fn default() -> Self {
        Self {
            llama_runtime_preset: LlamaRuntimePreset::default(),
            gpu_acceleration: default_gpu_acceleration(),
            gpu_layers: 99,
            n_threads: 8,
            batch_size: 512,
            kv_cache_type: KvCacheType::Q8,
            power_mode: PowerMode::default(),
        }
    }
}

impl HardwareParams {
    pub fn clamp(&mut self) {
        self.gpu_layers = self.gpu_layers.clamp(0, 80);
        self.n_threads = self.n_threads.clamp(1, 32);
        self.batch_size = self.batch_size.clamp(128, 2048);
    }

    pub fn sanitize(self) -> Self {
        let mut s = self;
        s.clamp();
        s
    }

    /// `--cache-type-v` に渡す文字列（空文字列 = 圧縮なし）
    pub fn kv_cache_type_str(&self) -> &'static str {
        match self.kv_cache_type {
            KvCacheType::None => "",
            KvCacheType::Q8 => "q8_0",
            KvCacheType::Q4 => "q4_0",
            KvCacheType::Turbo3 => "turbo3",
            KvCacheType::Turbo4 => "turbo4",
            KvCacheType::Turbo2 => "turbo2",
        }
    }
}

const fn default_gpu_acceleration() -> bool {
    true
}

/// エディタ UI テーマ（`Auto` は当面 Dark と同じ配色）
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum UiTheme {
    #[default]
    Dark,
    Light,
    Auto,
}

impl UiTheme {
    pub fn label(self) -> &'static str {
        match self {
            Self::Dark => "ダーク",
            Self::Light => "ライト",
            Self::Auto => "自動",
        }
    }
}

/// 許容フォントサイズ（px）
pub const APPEARANCE_FONT_SIZES: [i32; 4] = [12, 14, 16, 18];

/// エディタ外観（`model_params.json` の `appearance`）
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AppearancePrefs {
    #[serde(default)]
    pub theme: UiTheme,
    #[serde(default = "default_font_size_px")]
    pub font_size_px: i32,
    #[serde(default = "default_show_line_numbers")]
    pub show_line_numbers: bool,
}

fn default_font_size_px() -> i32 {
    14
}

fn default_show_line_numbers() -> bool {
    true
}

impl Default for AppearancePrefs {
    fn default() -> Self {
        Self {
            theme: UiTheme::default(),
            font_size_px: default_font_size_px(),
            show_line_numbers: default_show_line_numbers(),
        }
    }
}

impl AppearancePrefs {
    pub fn sanitize(mut self) -> Self {
        if !APPEARANCE_FONT_SIZES.contains(&self.font_size_px) {
            self.font_size_px = APPEARANCE_FONT_SIZES
                .iter()
                .copied()
                .min_by(|a, b| {
                    let da = (a - self.font_size_px).abs();
                    let db = (b - self.font_size_px).abs();
                    da.cmp(&db).then_with(|| b.cmp(a))
                })
                .unwrap_or(14);
        }
        self
    }

    pub fn clamp(&mut self) {
        *self = self.clone().sanitize();
    }

    /// ステップ `delta`（-1 / +1）で許容サイズ一覧上を移動
    pub fn step_font_size(current: i32, delta: i32) -> i32 {
        let idx = APPEARANCE_FONT_SIZES
            .iter()
            .position(|&s| s == current)
            .unwrap_or(1);
        let n = idx as i32 + delta;
        let clamped = n.clamp(0, APPEARANCE_FONT_SIZES.len() as i32 - 1);
        APPEARANCE_FONT_SIZES[clamped as usize]
    }

    pub fn cycle_theme(theme: UiTheme) -> UiTheme {
        match theme {
            UiTheme::Dark => UiTheme::Light,
            UiTheme::Light => UiTheme::Auto,
            UiTheme::Auto => UiTheme::Dark,
        }
    }
}

/// Chain-of-Thought (CoT) 推論モード
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CoTMode {
    /// CoT 無効（通常応答）
    Off,
    /// 基本 CoT（ステップバイステップで考えてから回答）
    #[default]
    Basic,
    /// 詳細 CoT（分解→仮説→検証→結論の4段階）
    Detailed,
}

impl CoTMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "無効",
            Self::Basic => "基本（推奨）",
            Self::Detailed => "詳細（4段階推論）",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Self::Off => "CoT なし — 直接回答する",
            Self::Basic => "ステップバイステップで考えてから回答する",
            Self::Detailed => "分解→仮説→検証→結論の4段階で推論する",
        }
    }

    /// システムプロンプトに注入する CoT 指示文を返す（Off なら空文字列）
    pub fn system_instruction(self) -> &'static str {
        match self {
            Self::Off => "",
            Self::Basic => "\n\n## 思考プロセス\n回答する際は、まずステップバイステップで考えを整理してから、最後に明確な結論をまとめてください。",
            Self::Detailed => "\n\n## 思考プロセス（Chain-of-Thought）\n問題を解決する際は、必ず以下のステップで考えてください：\n1. **分解**: 問題を小さな部分に分解して理解する\n2. **推論**: 各ステップごとに論理的に考える\n3. **検証**: 仮説を立てて矛盾がないか検証する\n4. **結論**: 最終結論を明確に述べる\n\n考えをステップバイステップで出力してから、最後に答えをまとめてください。",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AiPrefs {
    #[serde(default = "default_true")]
    pub auto_complete: bool,
    #[serde(default = "default_true")]
    pub code_suggestions: bool,
    #[serde(default = "default_true")]
    pub streaming_responses: bool,
    /// Chain-of-Thought 推論モード（ローカル LLM の推論品質を向上）
    #[serde(default)]
    pub cot_mode: CoTMode,
    /// Self-Consistency: 同じ質問を複数回投げて多数決で最良回答を採用
    #[serde(default)]
    pub self_consistency: SelfConsistencyMode,
    /// Tree-of-Thoughts: 複数の思考経路を探索して最良を合成
    #[serde(default)]
    pub tot_mode: ToTMode,
}

/// Tree-of-Thoughts (ToT) 推論モード
///
/// 1つの質問に対して複数の思考経路を生成・評価・合成するループ型推論。
/// LLM を「思考者」「評価者」「合成者」として 3フェーズで呼び出す。
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToTMode {
    /// 無効
    #[default]
    Off,
    /// 2経路探索 — 高速 ToT（分岐2 → 評価 → 合成）
    Branch2,
    /// 3経路探索 — 標準 ToT（分岐3 → 評価 → 合成）
    Branch3,
}

impl ToTMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "無効",
            Self::Branch2 => "2経路（高速）",
            Self::Branch3 => "3経路（標準）",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Self::Off => "ToT なし — 通常の 1 経路推論",
            Self::Branch2 => "2つの思考経路を探索して最良を合成",
            Self::Branch3 => "3つの思考経路を探索して最良を合成",
        }
    }

    pub fn branch_count(self) -> usize {
        match self {
            Self::Off => 0,
            Self::Branch2 => 2,
            Self::Branch3 => 3,
        }
    }

    pub fn is_enabled(self) -> bool {
        !matches!(self, Self::Off)
    }
}

/// Self-Consistency 多数決モード
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SelfConsistencyMode {
    /// 無効（1回の推論で回答）
    #[default]
    Off,
    /// 3回推論して多数決
    Vote3,
    /// 5回推論して多数決
    Vote5,
}

impl SelfConsistencyMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "無効",
            Self::Vote3 => "3回投票",
            Self::Vote5 => "5回投票",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Self::Off => "1回の推論で回答（高速）",
            Self::Vote3 => "3回推論して最も一致する回答を採用",
            Self::Vote5 => "5回推論して最も一致する回答を採用（高品質）",
        }
    }

    pub fn vote_count(self) -> usize {
        match self {
            Self::Off => 1,
            Self::Vote3 => 3,
            Self::Vote5 => 5,
        }
    }

    pub fn is_enabled(self) -> bool {
        !matches!(self, Self::Off)
    }
}

const fn default_true() -> bool {
    true
}

impl Default for AiPrefs {
    fn default() -> Self {
        Self {
            auto_complete: true,
            code_suggestions: true,
            streaming_responses: true,
            cot_mode: CoTMode::default(),
            self_consistency: SelfConsistencyMode::default(),
            tot_mode: ToTMode::default(),
        }
    }
}

impl AiPrefs {
    pub fn sanitize(self) -> Self {
        self
    }
}

/// Chat ページの推論先（設定で切替）
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ChatInferenceSource {
    /// OpenRouter / OpenAI / 汎用 OpenAI 互換
    #[default]
    #[serde(rename = "api")]
    Api,
    /// HTTP Ollama（`ollama_base_url` 必須）。JSON では従来どおり `local`
    #[serde(rename = "local")]
    Local,
    /// 設定に追加した GGUF / ONNX（ネイティブ `oag_inference_*`）
    #[serde(rename = "local_weights")]
    LocalWeights,
}

impl ChatInferenceSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Api => "クラウド API",
            Self::Local => "Ollama (HTTP)",
            Self::LocalWeights => "ローカル GGUF/ONNX",
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            Self::Api => Self::Local,
            Self::Local => Self::LocalWeights,
            Self::LocalWeights => Self::Api,
        }
    }
}

fn default_ollama_chat_model() -> String {
    "llama3.2".to_string()
}

const MAX_CHAT_MODEL_ID_LEN: usize = 512;

/// Chat で使うモデル ID（`model_params.json` の `chat`）
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ChatPrefs {
    pub source: ChatInferenceSource,
    /// クラウド向けモデル ID（空なら既定）
    pub api_model: String,
    /// クラウド向けプロバイダ ID（例: "openai", "groq", "deepseek"）空なら自動検出
    #[serde(default)]
    pub api_provider: String,
    /// Ollama のモデル名（`ollama list` に出る名前）
    pub ollama_model: String,
    /// `model_paths` リスト内のインデックス（LocalWeights 時）
    #[serde(default)]
    pub local_model_index: usize,
}

impl Default for ChatPrefs {
    fn default() -> Self {
        Self {
            source: ChatInferenceSource::default(),
            api_model: String::new(),
            api_provider: String::new(),
            ollama_model: default_ollama_chat_model(),
            local_model_index: 0,
        }
    }
}

impl ChatPrefs {
    pub fn sanitize(mut self) -> Self {
        self.api_model = Self::clamp_model_field(self.api_model);
        self.ollama_model = Self::clamp_model_field(self.ollama_model);
        if self.ollama_model.is_empty() {
            self.ollama_model = default_ollama_chat_model();
        }
        self.local_model_index = self.local_model_index.min(10_000);
        self
    }

    fn clamp_model_field(s: String) -> String {
        let t = s.trim();
        if t.len() > MAX_CHAT_MODEL_ID_LEN {
            t[..MAX_CHAT_MODEL_ID_LEN].trim().to_string()
        } else {
            t.to_string()
        }
    }
}

fn sanitize_model_path(path: Option<PathBuf>) -> Option<PathBuf> {
    path.and_then(|path| {
        if path.as_os_str().is_empty() {
            None
        } else if fs::metadata(&path).map(|m| m.is_file()).unwrap_or(false) {
            Some(path.canonicalize().unwrap_or(path))
        } else {
            None
        }
    })
}

fn sanitize_model_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in paths {
        if let Some(q) = sanitize_model_path(Some(p)) {
            if !out.contains(&q) {
                out.push(q);
            }
        }
    }
    out
}

/// 旧 JSON の単一 `model_path` を `model_paths` に併合する（必要なら正規化後に保存で置き換わる）。
fn migrate_prefs_json(raw: &str) -> String {
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(raw) else {
        return raw.to_string();
    };
    let Some(obj) = v.as_object_mut() else {
        return raw.to_string();
    };
    if let Some(old) = obj.remove("model_path") {
        let paths_val = obj.entry("model_paths").or_insert(serde_json::json!([]));
        if let serde_json::Value::Array(ref mut paths) = paths_val {
            if let serde_json::Value::String(s) = old {
                if !s.is_empty() && !paths.iter().any(|p| p.as_str() == Some(s.as_str())) {
                    paths.push(serde_json::Value::String(s));
                }
            }
        }
    }
    v.to_string()
}

/// モデル（ルートに flatten）＋ `hardware` / `appearance` / `ai` サブオブジェクト
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LocalLlmPrefs {
    #[serde(flatten)]
    pub model: ModelParams,
    #[serde(default)]
    pub hardware: HardwareParams,
    #[serde(default)]
    pub appearance: AppearancePrefs,
    #[serde(default)]
    pub ai: AiPrefs,
    /// 読み込み済みローカルモデル（順序保持・下に追加）
    #[serde(default)]
    pub model_paths: Vec<PathBuf>,
    /// Chat ページの推論先・モデル ID
    #[serde(default)]
    pub chat: ChatPrefs,
}

impl Default for LocalLlmPrefs {
    fn default() -> Self {
        Self {
            model: ModelParams::default(),
            hardware: HardwareParams::default(),
            appearance: AppearancePrefs::default(),
            ai: AiPrefs::default(),
            model_paths: Vec::new(),
            chat: ChatPrefs::default(),
        }
    }
}

impl LocalLlmPrefs {
    pub fn sanitize(mut self) -> Self {
        self.model = self.model.sanitize();
        self.hardware = self.hardware.sanitize();
        self.appearance = self.appearance.sanitize();
        self.ai = self.ai.sanitize();
        self.model_paths = sanitize_model_paths(std::mem::take(&mut self.model_paths));
        self.chat = self.chat.clone().sanitize();
        if self.chat.source == ChatInferenceSource::LocalWeights {
            self.model.max_output_tokens =
                effective_local_max_output_tokens(self.model.max_output_tokens);
        }
        self
    }

    pub fn clamp(&mut self) {
        self.model.clamp();
        self.hardware.clamp();
        self.appearance.clamp();
        self.ai = self.ai.clone().sanitize();
        self.chat = self.chat.clone().sanitize();
        if self.chat.source == ChatInferenceSource::LocalWeights {
            self.model.max_output_tokens =
                effective_local_max_output_tokens(self.model.max_output_tokens);
        }
    }
}

fn prefs_file() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("open_agents_gui")
        .join("model_params.json")
}

pub fn load_local_llm_prefs() -> LocalLlmPrefs {
    let path = prefs_file();
    let Ok(raw) = fs::read_to_string(&path) else {
        return LocalLlmPrefs::default();
    };
    let migrated = migrate_prefs_json(&raw);
    match serde_json::from_str::<LocalLlmPrefs>(&migrated) {
        Ok(p) => {
            let sanitized = p.clone().sanitize();
            if sanitized != p || migrated != raw {
                save_local_llm_prefs(&sanitized);
            }
            sanitized
        }
        Err(_) => {
            let model = serde_json::from_str::<ModelParams>(&raw)
                .or_else(|_| serde_json::from_str::<ModelParams>(&migrated))
                .unwrap_or_default()
                .sanitize();
            let prefs = LocalLlmPrefs {
                model,
                ..LocalLlmPrefs::default()
            }
            .sanitize();
            save_local_llm_prefs(&prefs);
            prefs
        }
    }
}

pub fn save_local_llm_prefs(prefs: &LocalLlmPrefs) {
    let path = prefs_file();
    let Some(parent) = path.parent() else {
        return;
    };
    if let Err(e) = fs::create_dir_all(parent) {
        eprintln!("model_prefs: ディレクトリ作成に失敗: {e}");
        return;
    }
    let mut p = prefs.clone();
    p.clamp();
    match serde_json::to_string_pretty(&p) {
        Ok(json) => {
            if let Err(e) = fs::write(&path, json) {
                eprintln!("model_prefs: 書き込み失敗: {e}");
            }
        }
        Err(e) => eprintln!("model_prefs: JSON 生成失敗: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_model_path(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("open_agents_{nonce}_{name}.gguf"))
    }

    #[test]
    fn sanitize_clamps_temperature_and_tokens() {
        let m = ModelParams {
            temperature: 5.0,
            max_output_tokens: 999_999,
            context_length: 10,
        }
        .sanitize();
        assert!((m.temperature - 2.0).abs() < 1e-6);
        assert_eq!(m.max_output_tokens, 8192);
        assert_eq!(m.context_length, 512);
    }

    #[test]
    fn local_gguf_max_tokens_defaults_and_caps() {
        assert_eq!(
            effective_local_max_output_tokens(0),
            DEFAULT_MAX_OUTPUT_TOKENS
        );
        assert_eq!(
            effective_local_max_output_tokens(2048),
            LOCAL_GGUF_MAX_OUTPUT_TOKENS_CAP
        );
        assert_eq!(effective_local_max_output_tokens(384), 384);
    }

    #[test]
    fn hardware_sanitize_clamps() {
        let h = HardwareParams {
            llama_runtime_preset: LlamaRuntimePreset::ExperimentalHybrid4090Arc,
            gpu_acceleration: true,
            gpu_layers: 1000,
            n_threads: 0,
            batch_size: 50,
            kv_cache_type: KvCacheType::Q8,
        }
        .sanitize();
        assert_eq!(h.gpu_layers, 80);
        assert_eq!(h.n_threads, 1);
        assert_eq!(h.batch_size, 128);
        assert_eq!(
            h.llama_runtime_preset,
            LlamaRuntimePreset::ExperimentalHybrid4090Arc
        );
    }

    #[test]
    fn legacy_flat_json_loads_model_only() {
        let raw = r#"{"temperature":0.5,"max_output_tokens":1024,"context_length":2048}"#;
        let p: LocalLlmPrefs = serde_json::from_str(raw).unwrap();
        assert!((p.model.temperature - 0.5).abs() < 1e-6);
        assert_eq!(p.model.max_output_tokens, 1024);
        assert_eq!(p.hardware, HardwareParams::default());
        assert_eq!(p.appearance, AppearancePrefs::default());
        assert_eq!(p.ai, AiPrefs::default());
        assert!(p.model_paths.is_empty());
    }

    #[test]
    fn migrates_legacy_model_path_into_model_paths() {
        let raw = r#"{"temperature":0.7,"max_output_tokens":2048,"context_length":4096,"model_path":"D:/models/x.gguf"}"#;
        let migrated = migrate_prefs_json(raw);
        let p: LocalLlmPrefs = serde_json::from_str(&migrated).unwrap();
        assert_eq!(p.model_paths.len(), 1);
        assert!(p.model_paths[0]
            .as_os_str()
            .to_string_lossy()
            .contains("x.gguf"));
    }

    #[test]
    fn appearance_json_roundtrip() {
        let raw = r#"{"temperature":0.7,"max_output_tokens":2048,"context_length":4096,"hardware":{},"appearance":{"theme":"light","font_size_px":16,"show_line_numbers":false}}"#;
        let p: LocalLlmPrefs = serde_json::from_str(raw).unwrap();
        assert_eq!(p.appearance.theme, UiTheme::Light);
        assert_eq!(p.appearance.font_size_px, 16);
        assert!(!p.appearance.show_line_numbers);
    }

    #[test]
    fn hardware_runtime_preset_json_roundtrip() {
        let raw = r#"{"temperature":0.7,"max_output_tokens":2048,"context_length":4096,"hardware":{"llama_runtime_preset":"experimental_hybrid_4090_arc","gpu_layers":64,"n_threads":12,"batch_size":768,"kv_cache_type":"q8"},"appearance":{},"ai":{},"model_paths":[],"chat":{}}"#;
        let p: LocalLlmPrefs = serde_json::from_str(raw).unwrap();
        assert_eq!(
            p.hardware.llama_runtime_preset,
            LlamaRuntimePreset::ExperimentalHybrid4090Arc
        );
        assert_eq!(p.hardware.gpu_layers, 64);
    }

    #[test]
    fn appearance_sanitize_snaps_font() {
        let a = AppearancePrefs {
            theme: UiTheme::Dark,
            font_size_px: 13,
            show_line_numbers: true,
        }
        .sanitize();
        assert_eq!(a.font_size_px, 14); // 13 は 12/14 同距離のとき大きい方へスナップ
    }

    #[test]
    fn sanitize_drops_empty_model_paths_entries() {
        let p = LocalLlmPrefs {
            model_paths: vec![PathBuf::new()],
            ..LocalLlmPrefs::default()
        }
        .sanitize();
        assert!(p.model_paths.is_empty());
    }

    #[test]
    fn local_weights_source_caps_extreme_max_tokens() {
        let prefs = LocalLlmPrefs {
            model: ModelParams {
                max_output_tokens: 2048,
                ..ModelParams::default()
            },
            chat: ChatPrefs {
                source: ChatInferenceSource::LocalWeights,
                ..ChatPrefs::default()
            },
            ..LocalLlmPrefs::default()
        }
        .sanitize();
        assert_eq!(
            prefs.model.max_output_tokens,
            LOCAL_GGUF_MAX_OUTPUT_TOKENS_CAP
        );
    }

    #[test]
    fn api_source_preserves_large_max_tokens() {
        let prefs = LocalLlmPrefs {
            model: ModelParams {
                max_output_tokens: 2048,
                ..ModelParams::default()
            },
            chat: ChatPrefs {
                source: ChatInferenceSource::Api,
                ..ChatPrefs::default()
            },
            ..LocalLlmPrefs::default()
        }
        .sanitize();
        assert_eq!(prefs.model.max_output_tokens, 2048);
    }

    #[test]
    fn sanitize_drops_missing_model_paths_entries() {
        let missing = temp_model_path("missing");
        let p = LocalLlmPrefs {
            model_paths: vec![missing],
            ..LocalLlmPrefs::default()
        }
        .sanitize();
        assert!(p.model_paths.is_empty());
    }

    #[test]
    fn sanitize_dedupes_model_paths() {
        let path = temp_model_path("dedupe");
        fs::write(&path, b"gguf").unwrap();
        let p = LocalLlmPrefs {
            model_paths: vec![path.clone(), path.clone()],
            ..LocalLlmPrefs::default()
        }
        .sanitize();
        let _ = fs::remove_file(&path);
        assert_eq!(p.model_paths.len(), 1);
    }
}
