//! gpu_grid.zig — DX12 instanced-draw grid renderer with DirectWrite glyph atlas.
//!
//! Architecture:
//!   Startup / font change:
//!     DirectWrite rasterize → R8 atlas bitmap (CPU) → GPU texture (upload once)
//!   Per frame:
//!     dirty cells → UpdateSubresource (structured buffer, few KB)
//!     → DrawInstanced(6, cell_count) → shader: atlas sample + fg/bg blend
//!     → render target → readback buffer → CPU RGBA pixels
//!
//! DX12 COM vtable bindings are minimal — only methods actually called are typed.
//! Pattern taken from ZWG Terminal's dx12.zig (proven correct).

const std = @import("std");
const Allocator = std.mem.Allocator;

// ================================================================
// Windows primitive types
// ================================================================

const HRESULT = i32;
const HANDLE = *anyopaque;
const BOOL = i32;
const LPCWSTR = [*:0]const u16;
const TRUE: BOOL = 1;
const FALSE: BOOL = 0;
const INFINITE: u32 = 0xFFFFFFFF;
const S_OK: HRESULT = 0;

inline fn SUCCEEDED(hr: HRESULT) bool {
    return hr >= 0;
}

const GUID = extern struct {
    Data1: u32,
    Data2: u16,
    Data3: u16,
    Data4: [8]u8,
};

const RECT = extern struct {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
};

// ================================================================
// DirectWrite types
// ================================================================

const DWRITE_FACTORY_TYPE = enum(i32) { SHARED = 0, ISOLATED = 1, _ };
const DWRITE_FONT_WEIGHT = enum(i32) { NORMAL = 400, _ };
const DWRITE_FONT_STYLE = enum(i32) { NORMAL = 0, _ };
const DWRITE_FONT_STRETCH = enum(i32) { NORMAL = 5, _ };
const DWRITE_MEASURING_MODE = enum(i32) { NATURAL = 0, _ };
const DWRITE_RENDERING_MODE = enum(i32) { DEFAULT = 0, ALIASED = 1, _ };
const DWRITE_TEXTURE_TYPE = enum(i32) { ALIASED_1x1 = 0, CLEARTYPE_3x1 = 1, _ };

const DWRITE_MATRIX = extern struct {
    m11: f32 = 1, m12: f32 = 0,
    m21: f32 = 0, m22: f32 = 1,
    dx: f32 = 0, dy: f32 = 0,
};

const DWRITE_FONT_METRICS = extern struct {
    designUnitsPerEm: u16 = 0,
    ascent: u16 = 0,
    descent: u16 = 0,
    lineGap: i16 = 0,
    capHeight: u16 = 0,
    xHeight: u16 = 0,
    underlinePosition: i16 = 0,
    underlineThickness: u16 = 0,
    strikethroughPosition: i16 = 0,
    strikethroughThickness: u16 = 0,
};

const DWRITE_GLYPH_OFFSET = extern struct {
    advanceOffset: f32 = 0,
    ascenderOffset: f32 = 0,
};

const DWRITE_GLYPH_RUN = extern struct {
    fontFace: ?*IDWriteFontFace = null,
    fontEmSize: f32 = 0,
    glyphCount: u32 = 0,
    glyphIndices: ?[*]const u16 = null,
    glyphAdvances: ?[*]const f32 = null,
    glyphOffsets: ?[*]const DWRITE_GLYPH_OFFSET = null,
    isSideways: BOOL = FALSE,
    bidiLevel: u32 = 0,
};

// ================================================================
// DXGI / D3D12 enums
// ================================================================

const DXGI_FORMAT = enum(u32) {
    UNKNOWN = 0,
    R8G8B8A8_UNORM = 28,
    B8G8R8A8_UNORM = 87,
    R8_UNORM = 61,
    _,
};

const DXGI_SAMPLE_DESC = extern struct { Count: u32 = 1, Quality: u32 = 0 };

const D3D_FEATURE_LEVEL = enum(u32) { @"11_0" = 0xb000, @"12_0" = 0xc000, _ };
const D3D12_COMMAND_LIST_TYPE = enum(u32) { DIRECT = 0, _ };
const D3D12_COMMAND_QUEUE_FLAGS = enum(u32) { NONE = 0, _ };
const D3D12_DESCRIPTOR_HEAP_TYPE = enum(u32) { CBV_SRV_UAV = 0, SAMPLER = 1, RTV = 2, _ };
const D3D12_DESCRIPTOR_HEAP_FLAGS = enum(u32) { NONE = 0, SHADER_VISIBLE = 1, _ };
const D3D12_PRIMITIVE_TOPOLOGY_TYPE = enum(u32) { TRIANGLE = 3, _ };
const D3D12_HEAP_TYPE = enum(u32) { DEFAULT = 1, UPLOAD = 2, READBACK = 3, _ };
const D3D12_CPU_PAGE_PROPERTY = enum(u32) { UNKNOWN = 0, _ };
const D3D12_MEMORY_POOL = enum(u32) { UNKNOWN = 0, _ };
const D3D12_RESOURCE_DIMENSION = enum(u32) { BUFFER = 1, TEXTURE2D = 3, _ };
const D3D12_TEXTURE_LAYOUT = enum(u32) { UNKNOWN = 0, ROW_MAJOR = 1, _ };
const D3D12_RESOURCE_FLAGS = enum(u32) { NONE = 0, ALLOW_RENDER_TARGET = 0x1, _ };
const D3D12_HEAP_FLAGS = enum(u32) { NONE = 0, _ };
const D3D12_FENCE_FLAGS = enum(u32) { NONE = 0, _ };
const D3D12_RESOURCE_BARRIER_TYPE = enum(u32) { TRANSITION = 0, _ };
const D3D12_RESOURCE_BARRIER_FLAGS = enum(u32) { NONE = 0, _ };
const D3D12_PRIMITIVE_TOPOLOGY = enum(u32) { TRIANGLELIST = 4, _ };
const D3D12_FILL_MODE = enum(u32) { SOLID = 3, _ };
const D3D12_CULL_MODE = enum(u32) { NONE = 1, _ };
const D3D12_BLEND = enum(u32) { ONE = 2, SRC_ALPHA = 5, INV_SRC_ALPHA = 6, _ };
const D3D12_BLEND_OP = enum(u32) { ADD = 1, _ };
const D3D12_LOGIC_OP = enum(u32) { NOOP = 5, _ };
const D3D12_COLOR_WRITE_ENABLE = enum(u8) { ALL = 0xF, _ };
const D3D12_ROOT_SIGNATURE_FLAGS = enum(u32) { ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT = 0x1, _ };
const D3D_ROOT_SIGNATURE_VERSION = enum(u32) { @"1_0" = 1, _ };
const D3D12_ROOT_PARAMETER_TYPE = enum(u32) { DESCRIPTOR_TABLE = 0, @"32BIT_CONSTANTS" = 1, SRV = 3, _ };
const D3D12_SHADER_VISIBILITY = enum(u32) { ALL = 0, VERTEX = 1, PIXEL = 5, _ };
const D3D12_DESCRIPTOR_RANGE_TYPE = enum(u32) { SRV = 0, _ };
const D3D12_FILTER = enum(u32) { MIN_MAG_MIP_POINT = 0, _ };
const D3D12_TEXTURE_ADDRESS_MODE = enum(u32) { CLAMP = 3, _ };
const D3D12_COMPARISON_FUNC = enum(u32) { NEVER = 1, _ };
const D3D12_STATIC_BORDER_COLOR = enum(u32) { TRANSPARENT_BLACK = 0, _ };
const D3D12_SRV_DIMENSION = enum(u32) { BUFFER = 1, TEXTURE2D = 4, _ };

const D3D12_RESOURCE_STATES = enum(u32) {
    COMMON = 0,
    RENDER_TARGET = 0x4,
    COPY_DEST = 0x400,
    COPY_SOURCE = 0x800,
    GENERIC_READ = 0x1 | 0x2 | 0x40 | 0x80 | 0x200 | 0x800,
    PIXEL_SHADER_RESOURCE = 0x80,
    _,
};

const RESOURCE_BARRIER_ALL_SUBRESOURCES: u32 = 0xFFFFFFFF;

// ================================================================
// D3D12 structs
// ================================================================

const D3D12_COMMAND_QUEUE_DESC = extern struct {
    Type: D3D12_COMMAND_LIST_TYPE = .DIRECT,
    Priority: i32 = 0,
    Flags: D3D12_COMMAND_QUEUE_FLAGS = .NONE,
    NodeMask: u32 = 0,
};

const D3D12_DESCRIPTOR_HEAP_DESC = extern struct {
    Type: D3D12_DESCRIPTOR_HEAP_TYPE,
    NumDescriptors: u32,
    Flags: D3D12_DESCRIPTOR_HEAP_FLAGS = .NONE,
    NodeMask: u32 = 0,
};

const D3D12_CPU_DESCRIPTOR_HANDLE = extern struct { ptr: usize = 0 };
const D3D12_GPU_DESCRIPTOR_HANDLE = extern struct { ptr: u64 = 0 };

const D3D12_HEAP_PROPERTIES = extern struct {
    Type: D3D12_HEAP_TYPE,
    CPUPageProperty: D3D12_CPU_PAGE_PROPERTY = .UNKNOWN,
    MemoryPoolPreference: D3D12_MEMORY_POOL = .UNKNOWN,
    CreationNodeMask: u32 = 0,
    VisibleNodeMask: u32 = 0,
};

const D3D12_RESOURCE_DESC = extern struct {
    Dimension: D3D12_RESOURCE_DIMENSION = .BUFFER,
    Alignment: u64 = 0,
    Width: u64 = 0,
    Height: u32 = 1,
    DepthOrArraySize: u16 = 1,
    MipLevels: u16 = 1,
    Format: DXGI_FORMAT = .UNKNOWN,
    SampleDesc: DXGI_SAMPLE_DESC = .{},
    Layout: D3D12_TEXTURE_LAYOUT = .UNKNOWN,
    Flags: D3D12_RESOURCE_FLAGS = .NONE,
};

const D3D12_CLEAR_VALUE = extern struct { Format: DXGI_FORMAT, Color: [4]f32 };

const D3D12_RESOURCE_TRANSITION_BARRIER = extern struct {
    pResource: ?*anyopaque = null,
    Subresource: u32 = RESOURCE_BARRIER_ALL_SUBRESOURCES,
    StateBefore: D3D12_RESOURCE_STATES = .COMMON,
    StateAfter: D3D12_RESOURCE_STATES = .COMMON,
};

const D3D12_RESOURCE_BARRIER = extern struct {
    Type: D3D12_RESOURCE_BARRIER_TYPE = .TRANSITION,
    Flags: D3D12_RESOURCE_BARRIER_FLAGS = .NONE,
    Transition: D3D12_RESOURCE_TRANSITION_BARRIER = .{},

    fn transition(resource: ?*anyopaque, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) D3D12_RESOURCE_BARRIER {
        return .{ .Transition = .{
            .pResource = resource,
            .Subresource = RESOURCE_BARRIER_ALL_SUBRESOURCES,
            .StateBefore = before,
            .StateAfter = after,
        } };
    }
};

const D3D12_VIEWPORT = extern struct {
    TopLeftX: f32 = 0, TopLeftY: f32 = 0,
    Width: f32, Height: f32,
    MinDepth: f32 = 0, MaxDepth: f32 = 1,
};

const D3D12_SHADER_BYTECODE = extern struct {
    pShaderBytecode: ?*const anyopaque = null,
    BytecodeLength: usize = 0,
};

const D3D12_RASTERIZER_DESC = extern struct {
    FillMode: D3D12_FILL_MODE = .SOLID,
    CullMode: D3D12_CULL_MODE = .NONE,
    FrontCounterClockwise: BOOL = FALSE,
    DepthBias: i32 = 0,
    DepthBiasClamp: f32 = 0,
    SlopeScaledDepthBias: f32 = 0,
    DepthClipEnable: BOOL = TRUE,
    MultisampleEnable: BOOL = FALSE,
    AntialiasedLineEnable: BOOL = FALSE,
    ForcedSampleCount: u32 = 0,
    ConservativeRaster: u32 = 0,
};

const D3D12_RENDER_TARGET_BLEND_DESC = extern struct {
    BlendEnable: BOOL = TRUE,
    LogicOpEnable: BOOL = FALSE,
    SrcBlend: D3D12_BLEND = .SRC_ALPHA,
    DestBlend: D3D12_BLEND = .INV_SRC_ALPHA,
    BlendOp: D3D12_BLEND_OP = .ADD,
    SrcBlendAlpha: D3D12_BLEND = .ONE,
    DestBlendAlpha: D3D12_BLEND = .INV_SRC_ALPHA,
    BlendOpAlpha: D3D12_BLEND_OP = .ADD,
    LogicOp: D3D12_LOGIC_OP = .NOOP,
    RenderTargetWriteMask: u8 = @intFromEnum(D3D12_COLOR_WRITE_ENABLE.ALL),
};

const D3D12_BLEND_DESC = extern struct {
    AlphaToCoverageEnable: BOOL = FALSE,
    IndependentBlendEnable: BOOL = FALSE,
    RenderTarget: [8]D3D12_RENDER_TARGET_BLEND_DESC = .{D3D12_RENDER_TARGET_BLEND_DESC{}} ** 8,
};

const D3D12_DEPTH_STENCIL_DESC = extern struct {
    DepthEnable: BOOL = FALSE,
    DepthWriteMask: u32 = 0,
    DepthFunc: u32 = 0,
    StencilEnable: BOOL = FALSE,
    StencilReadMask: u8 = 0xFF,
    StencilWriteMask: u8 = 0xFF,
    FrontFace: [4]u32 = .{ 0, 0, 0, 0 },
    BackFace: [4]u32 = .{ 0, 0, 0, 0 },
};

const D3D12_INPUT_LAYOUT_DESC = extern struct {
    pInputElementDescs: ?*const anyopaque = null,
    NumElements: u32 = 0,
};

const D3D12_STREAM_OUTPUT_DESC = extern struct {
    pSODeclaration: ?*const anyopaque = null,
    NumEntries: u32 = 0,
    pBufferStrides: ?[*]const u32 = null,
    NumStrides: u32 = 0,
    RasterizedStream: u32 = 0,
};

const D3D12_CACHED_PIPELINE_STATE = extern struct {
    pCachedBlob: ?*const anyopaque = null,
    CachedBlobSizeInBytes: usize = 0,
};

const D3D12_GRAPHICS_PIPELINE_STATE_DESC = extern struct {
    pRootSignature: ?*anyopaque = null,
    VS: D3D12_SHADER_BYTECODE = .{},
    PS: D3D12_SHADER_BYTECODE = .{},
    DS: D3D12_SHADER_BYTECODE = .{},
    HS: D3D12_SHADER_BYTECODE = .{},
    GS: D3D12_SHADER_BYTECODE = .{},
    StreamOutput: D3D12_STREAM_OUTPUT_DESC = .{},
    BlendState: D3D12_BLEND_DESC = .{},
    SampleMask: u32 = 0xFFFFFFFF,
    RasterizerState: D3D12_RASTERIZER_DESC = .{},
    DepthStencilState: D3D12_DEPTH_STENCIL_DESC = .{},
    InputLayout: D3D12_INPUT_LAYOUT_DESC = .{},
    IBStripCutValue: u32 = 0,
    PrimitiveTopologyType: D3D12_PRIMITIVE_TOPOLOGY_TYPE = .TRIANGLE,
    NumRenderTargets: u32 = 1,
    RTVFormats: [8]DXGI_FORMAT = .{.UNKNOWN} ** 8,
    DSVFormat: DXGI_FORMAT = .UNKNOWN,
    SampleDesc: DXGI_SAMPLE_DESC = .{},
    NodeMask: u32 = 0,
    CachedPSO: D3D12_CACHED_PIPELINE_STATE = .{},
    Flags: u32 = 0,
};

const D3D12_DESCRIPTOR_RANGE = extern struct {
    RangeType: D3D12_DESCRIPTOR_RANGE_TYPE,
    NumDescriptors: u32,
    BaseShaderRegister: u32,
    RegisterSpace: u32 = 0,
    OffsetInDescriptorsFromTableStart: u32 = 0xFFFFFFFF,
};

const D3D12_ROOT_DESCRIPTOR_TABLE = extern struct {
    NumDescriptorRanges: u32,
    pDescriptorRanges: ?[*]const D3D12_DESCRIPTOR_RANGE,
};

const D3D12_ROOT_CONSTANTS = extern struct {
    ShaderRegister: u32,
    RegisterSpace: u32 = 0,
    Num32BitValues: u32,
};

const D3D12_ROOT_PARAMETER = extern struct {
    ParameterType: D3D12_ROOT_PARAMETER_TYPE,
    u: extern union {
        DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE,
        Constants: D3D12_ROOT_CONSTANTS,
        Descriptor: extern struct { ShaderRegister: u32, RegisterSpace: u32 },
    },
    ShaderVisibility: D3D12_SHADER_VISIBILITY = .ALL,
};

const D3D12_STATIC_SAMPLER_DESC = extern struct {
    Filter: D3D12_FILTER = .MIN_MAG_MIP_POINT,
    AddressU: D3D12_TEXTURE_ADDRESS_MODE = .CLAMP,
    AddressV: D3D12_TEXTURE_ADDRESS_MODE = .CLAMP,
    AddressW: D3D12_TEXTURE_ADDRESS_MODE = .CLAMP,
    MipLODBias: f32 = 0,
    MaxAnisotropy: u32 = 0,
    ComparisonFunc: D3D12_COMPARISON_FUNC = .NEVER,
    BorderColor: D3D12_STATIC_BORDER_COLOR = .TRANSPARENT_BLACK,
    MinLOD: f32 = 0,
    MaxLOD: f32 = 3.402823466e+38,
    ShaderRegister: u32 = 0,
    RegisterSpace: u32 = 0,
    ShaderVisibility: D3D12_SHADER_VISIBILITY = .PIXEL,
};

const D3D12_ROOT_SIGNATURE_DESC = extern struct {
    NumParameters: u32 = 0,
    pParameters: ?[*]const D3D12_ROOT_PARAMETER = null,
    NumStaticSamplers: u32 = 0,
    pStaticSamplers: ?[*]const D3D12_STATIC_SAMPLER_DESC = null,
    Flags: D3D12_ROOT_SIGNATURE_FLAGS = .ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
};

const D3D12_SHADER_RESOURCE_VIEW_DESC = extern struct {
    Format: DXGI_FORMAT,
    ViewDimension: D3D12_SRV_DIMENSION,
    Shader4ComponentMapping: u32 = 0x00001688,
    u: extern union {
        Buffer: extern struct {
            FirstElement: u64 = 0,
            NumElements: u32 = 0,
            StructureByteStride: u32 = 0,
            Flags: u32 = 0,
        },
        Texture2D: extern struct {
            MostDetailedMip: u32 = 0,
            MipLevels: u32 = 1,
            PlaneSlice: u32 = 0,
            ResourceMinLODClamp: f32 = 0,
        },
    } = .{ .Texture2D = .{} },
};

const D3D12_TEXTURE_COPY_LOCATION = extern struct {
    pResource: ?*anyopaque = null,
    Type: u32 = 0,
    u: extern union {
        PlacedFootprint: extern struct {
            Offset: u64 = 0,
            Format: DXGI_FORMAT = .UNKNOWN,
            Width: u32 = 0,
            Height: u32 = 0,
            Depth: u32 = 1,
            RowPitch: u32 = 0,
        },
        SubresourceIndex: u32,
    } = .{ .SubresourceIndex = 0 },
};

const D3D12_BOX = extern struct {
    left: u32 = 0, top: u32 = 0, front: u32 = 0,
    right: u32, bottom: u32, back: u32 = 1,
};

// ================================================================
// COM vtable helper (ZWG Terminal pattern)
// ================================================================

fn vtCall(ptr: ?*anyopaque, comptime idx: usize, comptime F: type) F {
    const base: [*]const usize = @ptrCast(@alignCast(ptr));
    return @ptrFromInt(base[idx]);
}

// ================================================================
// COM interfaces — only methods we actually call are typed
// ================================================================

const ID3DBlob = extern struct {
    lpVtbl: ?*anyopaque,
    fn Release(self: *ID3DBlob) u32 { return vtCall(self.lpVtbl, 2, *const fn (*ID3DBlob) callconv(.c) u32)(self); }
    fn GetBufferPointer(self: *ID3DBlob) ?*anyopaque { return vtCall(self.lpVtbl, 3, *const fn (*ID3DBlob) callconv(.c) ?*anyopaque)(self); }
    fn GetBufferSize(self: *ID3DBlob) usize { return vtCall(self.lpVtbl, 4, *const fn (*ID3DBlob) callconv(.c) usize)(self); }
};

const IDWriteFactory = extern struct {
    lpVtbl: ?*anyopaque,
    fn Release(self: *IDWriteFactory) u32 { return vtCall(self.lpVtbl, 2, *const fn (*IDWriteFactory) callconv(.c) u32)(self); }
    fn GetGdiInterop(self: *IDWriteFactory, out: *?*anyopaque) HRESULT {
        return vtCall(self.lpVtbl, 17, *const fn (*IDWriteFactory, *?*anyopaque) callconv(.c) HRESULT)(self, out);
    }
    fn CreateGlyphRunAnalysis(
        self: *IDWriteFactory, glyph_run: *const DWRITE_GLYPH_RUN, ppd: f32,
        transform: ?*const DWRITE_MATRIX, rm: DWRITE_RENDERING_MODE, mm: DWRITE_MEASURING_MODE,
        bx: f32, by: f32, out: *?*anyopaque,
    ) HRESULT {
        return vtCall(self.lpVtbl, 23, *const fn (
            *IDWriteFactory, *const DWRITE_GLYPH_RUN, f32, ?*const DWRITE_MATRIX,
            DWRITE_RENDERING_MODE, DWRITE_MEASURING_MODE, f32, f32, *?*anyopaque,
        ) callconv(.c) HRESULT)(self, glyph_run, ppd, transform, rm, mm, bx, by, out);
    }
};

const IDWriteGdiInterop = extern struct {
    lpVtbl: ?*anyopaque,
    fn Release(self: *IDWriteGdiInterop) u32 { return vtCall(self.lpVtbl, 2, *const fn (*IDWriteGdiInterop) callconv(.c) u32)(self); }
    fn CreateFontFaceFromHdc(self: *IDWriteGdiInterop, hdc: HDC, out: *?*anyopaque) HRESULT {
        return vtCall(self.lpVtbl, 6, *const fn (*IDWriteGdiInterop, HDC, *?*anyopaque) callconv(.c) HRESULT)(self, hdc, out);
    }
};

const IDWriteFontFace = extern struct {
    lpVtbl: ?*anyopaque,
    fn Release(self: *IDWriteFontFace) u32 { return vtCall(self.lpVtbl, 2, *const fn (*IDWriteFontFace) callconv(.c) u32)(self); }
    fn GetGlyphIndices(self: *IDWriteFontFace, cps: [*]const u32, count: u32, out: [*]u16) HRESULT {
        return vtCall(self.lpVtbl, 11, *const fn (*IDWriteFontFace, [*]const u32, u32, [*]u16) callconv(.c) HRESULT)(self, cps, count, out);
    }
    fn GetGdiCompatibleMetrics(self: *IDWriteFontFace, em: f32, ppd: f32, t: ?*const DWRITE_MATRIX, out: *DWRITE_FONT_METRICS) HRESULT {
        return vtCall(self.lpVtbl, 16, *const fn (*IDWriteFontFace, f32, f32, ?*const DWRITE_MATRIX, *DWRITE_FONT_METRICS) callconv(.c) HRESULT)(self, em, ppd, t, out);
    }
};

const IDWriteGlyphRunAnalysis = extern struct {
    lpVtbl: ?*anyopaque,
    fn Release(self: *IDWriteGlyphRunAnalysis) u32 { return vtCall(self.lpVtbl, 2, *const fn (*IDWriteGlyphRunAnalysis) callconv(.c) u32)(self); }
    fn GetAlphaTextureBounds(self: *IDWriteGlyphRunAnalysis, tt: DWRITE_TEXTURE_TYPE, out: *RECT) HRESULT {
        return vtCall(self.lpVtbl, 3, *const fn (*IDWriteGlyphRunAnalysis, DWRITE_TEXTURE_TYPE, *RECT) callconv(.c) HRESULT)(self, tt, out);
    }
    fn CreateAlphaTexture(self: *IDWriteGlyphRunAnalysis, tt: DWRITE_TEXTURE_TYPE, bounds: *const RECT, buf: [*]u8, size: u32) HRESULT {
        return vtCall(self.lpVtbl, 4, *const fn (*IDWriteGlyphRunAnalysis, DWRITE_TEXTURE_TYPE, *const RECT, [*]u8, u32) callconv(.c) HRESULT)(self, tt, bounds, buf, size);
    }
};

const ID3D12Device = extern struct {
    lpVtbl: ?*anyopaque,
    fn Release(self: *ID3D12Device) u32 { return vtCall(self.lpVtbl, 2, *const fn (*ID3D12Device) callconv(.c) u32)(self); }
    fn CreateCommandQueue(self: *ID3D12Device, desc: *const D3D12_COMMAND_QUEUE_DESC, riid: *const GUID, out: *?*anyopaque) HRESULT {
        return vtCall(self.lpVtbl, 8, *const fn (*ID3D12Device, *const D3D12_COMMAND_QUEUE_DESC, *const GUID, *?*anyopaque) callconv(.c) HRESULT)(self, desc, riid, out);
    }
    fn CreateCommandAllocator(self: *ID3D12Device, typ: D3D12_COMMAND_LIST_TYPE, riid: *const GUID, out: *?*anyopaque) HRESULT {
        return vtCall(self.lpVtbl, 9, *const fn (*ID3D12Device, D3D12_COMMAND_LIST_TYPE, *const GUID, *?*anyopaque) callconv(.c) HRESULT)(self, typ, riid, out);
    }
    fn CreateGraphicsPipelineState(self: *ID3D12Device, desc: *const D3D12_GRAPHICS_PIPELINE_STATE_DESC, riid: *const GUID, out: *?*anyopaque) HRESULT {
        return vtCall(self.lpVtbl, 10, *const fn (*ID3D12Device, *const D3D12_GRAPHICS_PIPELINE_STATE_DESC, *const GUID, *?*anyopaque) callconv(.c) HRESULT)(self, desc, riid, out);
    }
    fn CreateCommandList(self: *ID3D12Device, nm: u32, typ: D3D12_COMMAND_LIST_TYPE, alloc: *anyopaque, pso: ?*anyopaque, riid: *const GUID, out: *?*anyopaque) HRESULT {
        return vtCall(self.lpVtbl, 12, *const fn (*ID3D12Device, u32, D3D12_COMMAND_LIST_TYPE, *anyopaque, ?*anyopaque, *const GUID, *?*anyopaque) callconv(.c) HRESULT)(self, nm, typ, alloc, pso, riid, out);
    }
    fn CreateDescriptorHeap(self: *ID3D12Device, desc: *const D3D12_DESCRIPTOR_HEAP_DESC, riid: *const GUID, out: *?*anyopaque) HRESULT {
        return vtCall(self.lpVtbl, 14, *const fn (*ID3D12Device, *const D3D12_DESCRIPTOR_HEAP_DESC, *const GUID, *?*anyopaque) callconv(.c) HRESULT)(self, desc, riid, out);
    }
    fn GetDescriptorHandleIncrementSize(self: *ID3D12Device, typ: D3D12_DESCRIPTOR_HEAP_TYPE) u32 {
        return vtCall(self.lpVtbl, 15, *const fn (*ID3D12Device, D3D12_DESCRIPTOR_HEAP_TYPE) callconv(.c) u32)(self, typ);
    }
    fn CreateRootSignature(self: *ID3D12Device, nm: u32, blob: ?*const anyopaque, len: usize, riid: *const GUID, out: *?*anyopaque) HRESULT {
        return vtCall(self.lpVtbl, 16, *const fn (*ID3D12Device, u32, ?*const anyopaque, usize, *const GUID, *?*anyopaque) callconv(.c) HRESULT)(self, nm, blob, len, riid, out);
    }
    fn CreateShaderResourceView(self: *ID3D12Device, res: ?*anyopaque, desc: ?*const D3D12_SHADER_RESOURCE_VIEW_DESC, handle: D3D12_CPU_DESCRIPTOR_HANDLE) void {
        vtCall(self.lpVtbl, 18, *const fn (*ID3D12Device, ?*anyopaque, ?*const D3D12_SHADER_RESOURCE_VIEW_DESC, D3D12_CPU_DESCRIPTOR_HANDLE) callconv(.c) void)(self, res, desc, handle);
    }
    fn CreateRenderTargetView(self: *ID3D12Device, res: ?*anyopaque, desc: ?*anyopaque, handle: D3D12_CPU_DESCRIPTOR_HANDLE) void {
        vtCall(self.lpVtbl, 20, *const fn (*ID3D12Device, ?*anyopaque, ?*anyopaque, D3D12_CPU_DESCRIPTOR_HANDLE) callconv(.c) void)(self, res, desc, handle);
    }
    fn CreateCommittedResource(self: *ID3D12Device, hp: *const D3D12_HEAP_PROPERTIES, hf: D3D12_HEAP_FLAGS, desc: *const D3D12_RESOURCE_DESC, state: D3D12_RESOURCE_STATES, cv: ?*const D3D12_CLEAR_VALUE, riid: *const GUID, out: *?*anyopaque) HRESULT {
        return vtCall(self.lpVtbl, 27, *const fn (*ID3D12Device, *const D3D12_HEAP_PROPERTIES, D3D12_HEAP_FLAGS, *const D3D12_RESOURCE_DESC, D3D12_RESOURCE_STATES, ?*const D3D12_CLEAR_VALUE, *const GUID, *?*anyopaque) callconv(.c) HRESULT)(self, hp, hf, desc, state, cv, riid, out);
    }
    fn CreateFence(self: *ID3D12Device, val: u64, flags: D3D12_FENCE_FLAGS, riid: *const GUID, out: *?*anyopaque) HRESULT {
        return vtCall(self.lpVtbl, 36, *const fn (*ID3D12Device, u64, D3D12_FENCE_FLAGS, *const GUID, *?*anyopaque) callconv(.c) HRESULT)(self, val, flags, riid, out);
    }
};

const ID3D12CommandQueue = extern struct {
    lpVtbl: ?*anyopaque,
    fn Release(self: *@This()) u32 { return vtCall(self.lpVtbl, 2, *const fn (*@This()) callconv(.c) u32)(self); }
    fn ExecuteCommandLists(self: *@This(), num: u32, lists: [*]const *anyopaque) void {
        vtCall(self.lpVtbl, 10, *const fn (*@This(), u32, [*]const *anyopaque) callconv(.c) void)(self, num, lists);
    }
    fn Signal(self: *@This(), fence: *anyopaque, value: u64) HRESULT {
        return vtCall(self.lpVtbl, 14, *const fn (*@This(), *anyopaque, u64) callconv(.c) HRESULT)(self, fence, value);
    }
};

const ID3D12CommandAllocator = extern struct {
    lpVtbl: ?*anyopaque,
    fn Release(self: *@This()) u32 { return vtCall(self.lpVtbl, 2, *const fn (*@This()) callconv(.c) u32)(self); }
    fn Reset(self: *@This()) HRESULT { return vtCall(self.lpVtbl, 8, *const fn (*@This()) callconv(.c) HRESULT)(self); }
};

const ID3D12GraphicsCommandList = extern struct {
    lpVtbl: ?*anyopaque,
    fn Release(self: *@This()) u32 { return vtCall(self.lpVtbl, 2, *const fn (*@This()) callconv(.c) u32)(self); }
    fn Close(self: *@This()) HRESULT { return vtCall(self.lpVtbl, 9, *const fn (*@This()) callconv(.c) HRESULT)(self); }
    fn Reset(self: *@This(), alloc: *anyopaque, pso: ?*anyopaque) HRESULT {
        return vtCall(self.lpVtbl, 10, *const fn (*@This(), *anyopaque, ?*anyopaque) callconv(.c) HRESULT)(self, alloc, pso);
    }
    fn DrawInstanced(self: *@This(), vc: u32, ic: u32, sv: u32, si: u32) void {
        vtCall(self.lpVtbl, 12, *const fn (*@This(), u32, u32, u32, u32) callconv(.c) void)(self, vc, ic, sv, si);
    }
    fn CopyResource(self: *@This(), dst: *anyopaque, src: *anyopaque) void {
        vtCall(self.lpVtbl, 17, *const fn (*@This(), *anyopaque, *anyopaque) callconv(.c) void)(self, dst, src);
    }
    fn CopyTextureRegion(self: *@This(), dst: *const D3D12_TEXTURE_COPY_LOCATION, dx_: u32, dy: u32, dz: u32, src: *const D3D12_TEXTURE_COPY_LOCATION, box: ?*const D3D12_BOX) void {
        vtCall(self.lpVtbl, 16, *const fn (*@This(), *const D3D12_TEXTURE_COPY_LOCATION, u32, u32, u32, *const D3D12_TEXTURE_COPY_LOCATION, ?*const D3D12_BOX) callconv(.c) void)(self, dst, dx_, dy, dz, src, box);
    }
    fn IASetPrimitiveTopology(self: *@This(), t: D3D12_PRIMITIVE_TOPOLOGY) void {
        vtCall(self.lpVtbl, 20, *const fn (*@This(), D3D12_PRIMITIVE_TOPOLOGY) callconv(.c) void)(self, t);
    }
    fn RSSetViewports(self: *@This(), n: u32, v: [*]const D3D12_VIEWPORT) void {
        vtCall(self.lpVtbl, 21, *const fn (*@This(), u32, [*]const D3D12_VIEWPORT) callconv(.c) void)(self, n, v);
    }
    fn RSSetScissorRects(self: *@This(), n: u32, r: [*]const RECT) void {
        vtCall(self.lpVtbl, 22, *const fn (*@This(), u32, [*]const RECT) callconv(.c) void)(self, n, r);
    }
    fn SetPipelineState(self: *@This(), pso: *anyopaque) void {
        vtCall(self.lpVtbl, 25, *const fn (*@This(), *anyopaque) callconv(.c) void)(self, pso);
    }
    fn ResourceBarrier(self: *@This(), n: u32, b: [*]const D3D12_RESOURCE_BARRIER) void {
        vtCall(self.lpVtbl, 26, *const fn (*@This(), u32, [*]const D3D12_RESOURCE_BARRIER) callconv(.c) void)(self, n, b);
    }
    fn SetDescriptorHeaps(self: *@This(), n: u32, h: [*]const *anyopaque) void {
        vtCall(self.lpVtbl, 28, *const fn (*@This(), u32, [*]const *anyopaque) callconv(.c) void)(self, n, h);
    }
    fn SetGraphicsRootSignature(self: *@This(), sig: *anyopaque) void {
        vtCall(self.lpVtbl, 30, *const fn (*@This(), *anyopaque) callconv(.c) void)(self, sig);
    }
    fn SetGraphicsRootDescriptorTable(self: *@This(), idx: u32, base: D3D12_GPU_DESCRIPTOR_HANDLE) void {
        vtCall(self.lpVtbl, 32, *const fn (*@This(), u32, D3D12_GPU_DESCRIPTOR_HANDLE) callconv(.c) void)(self, idx, base);
    }
    fn SetGraphicsRoot32BitConstants(self: *@This(), idx: u32, n: u32, data: ?*const anyopaque, off: u32) void {
        vtCall(self.lpVtbl, 36, *const fn (*@This(), u32, u32, ?*const anyopaque, u32) callconv(.c) void)(self, idx, n, data, off);
    }
    fn OMSetRenderTargets(self: *@This(), n: u32, rtvs: ?[*]const D3D12_CPU_DESCRIPTOR_HANDLE, single: BOOL, dsv: ?*const D3D12_CPU_DESCRIPTOR_HANDLE) void {
        vtCall(self.lpVtbl, 46, *const fn (*@This(), u32, ?[*]const D3D12_CPU_DESCRIPTOR_HANDLE, BOOL, ?*const D3D12_CPU_DESCRIPTOR_HANDLE) callconv(.c) void)(self, n, rtvs, single, dsv);
    }
    fn ClearRenderTargetView(self: *@This(), h: D3D12_CPU_DESCRIPTOR_HANDLE, col: *const [4]f32, n: u32, r: ?[*]const RECT) void {
        vtCall(self.lpVtbl, 48, *const fn (*@This(), D3D12_CPU_DESCRIPTOR_HANDLE, *const [4]f32, u32, ?[*]const RECT) callconv(.c) void)(self, h, col, n, r);
    }
};

const ID3D12Fence = extern struct {
    lpVtbl: ?*anyopaque,
    fn Release(self: *@This()) u32 { return vtCall(self.lpVtbl, 2, *const fn (*@This()) callconv(.c) u32)(self); }
    fn GetCompletedValue(self: *@This()) u64 { return vtCall(self.lpVtbl, 8, *const fn (*@This()) callconv(.c) u64)(self); }
    fn SetEventOnCompletion(self: *@This(), val: u64, ev: HANDLE) HRESULT {
        return vtCall(self.lpVtbl, 9, *const fn (*@This(), u64, HANDLE) callconv(.c) HRESULT)(self, val, ev);
    }
};

const ID3D12Resource = extern struct {
    lpVtbl: ?*anyopaque,
    fn Release(self: *@This()) u32 { return vtCall(self.lpVtbl, 2, *const fn (*@This()) callconv(.c) u32)(self); }
    fn Map(self: *@This(), sub: u32, range: ?*const anyopaque, out: *?*anyopaque) HRESULT {
        return vtCall(self.lpVtbl, 8, *const fn (*@This(), u32, ?*const anyopaque, *?*anyopaque) callconv(.c) HRESULT)(self, sub, range, out);
    }
    fn Unmap(self: *@This(), sub: u32, range: ?*const anyopaque) void {
        vtCall(self.lpVtbl, 9, *const fn (*@This(), u32, ?*const anyopaque) callconv(.c) void)(self, sub, range);
    }
    fn GetGPUVirtualAddress(self: *@This()) u64 {
        return vtCall(self.lpVtbl, 11, *const fn (*@This()) callconv(.c) u64)(self);
    }
};

const ID3D12DescriptorHeap = extern struct {
    lpVtbl: ?*anyopaque,
    fn Release(self: *@This()) u32 { return vtCall(self.lpVtbl, 2, *const fn (*@This()) callconv(.c) u32)(self); }
    fn GetCPUDescriptorHandleForHeapStart(self: *@This()) D3D12_CPU_DESCRIPTOR_HANDLE {
        var h: D3D12_CPU_DESCRIPTOR_HANDLE = .{};
        _ = vtCall(self.lpVtbl, 9, *const fn (*@This(), *D3D12_CPU_DESCRIPTOR_HANDLE) callconv(.c) *D3D12_CPU_DESCRIPTOR_HANDLE)(self, &h);
        return h;
    }
    fn GetGPUDescriptorHandleForHeapStart(self: *@This()) D3D12_GPU_DESCRIPTOR_HANDLE {
        var h: D3D12_GPU_DESCRIPTOR_HANDLE = .{};
        _ = vtCall(self.lpVtbl, 10, *const fn (*@This(), *D3D12_GPU_DESCRIPTOR_HANDLE) callconv(.c) *D3D12_GPU_DESCRIPTOR_HANDLE)(self, &h);
        return h;
    }
};

const IDXGIFactory4 = extern struct {
    lpVtbl: ?*anyopaque,
    fn Release(self: *@This()) u32 { return vtCall(self.lpVtbl, 2, *const fn (*@This()) callconv(.c) u32)(self); }
    fn EnumWarpAdapter(self: *@This(), riid: *const GUID, out: *?*anyopaque) HRESULT {
        return vtCall(self.lpVtbl, 27, *const fn (*@This(), *const GUID, *?*anyopaque) callconv(.c) HRESULT)(self, riid, out);
    }
};

const IUnknown = extern struct {
    lpVtbl: ?*anyopaque,
    fn Release(self: *@This()) u32 { return vtCall(self.lpVtbl, 2, *const fn (*@This()) callconv(.c) u32)(self); }
};

// ================================================================
// Free functions
// ================================================================

extern fn D3D12CreateDevice(?*anyopaque, D3D_FEATURE_LEVEL, *const GUID, *?*anyopaque) callconv(.c) HRESULT;
extern fn D3D12SerializeRootSignature(*const D3D12_ROOT_SIGNATURE_DESC, D3D_ROOT_SIGNATURE_VERSION, *?*ID3DBlob, *?*ID3DBlob) callconv(.c) HRESULT;
// D3DCompile removed — using pre-compiled CSO via @embedFile
extern "dwrite" fn DWriteCreateFactory(DWRITE_FACTORY_TYPE, *const GUID, *?*anyopaque) callconv(.c) HRESULT;
extern "dxgi" fn CreateDXGIFactory1(*const GUID, *?*anyopaque) callconv(.c) HRESULT;

extern "kernel32" fn CreateEventW(?*anyopaque, BOOL, BOOL, ?LPCWSTR) callconv(.c) ?HANDLE;
extern "kernel32" fn WaitForSingleObject(HANDLE, u32) callconv(.c) u32;
extern "kernel32" fn CloseHandle(HANDLE) callconv(.c) BOOL;

// GDI for glyph rasterization
const HDC = ?*anyopaque;
const HBITMAP = ?*anyopaque;
const HFONT = ?*anyopaque;
const HGDIOBJ = ?*anyopaque;

const BITMAPINFOHEADER = extern struct {
    biSize: u32 = 40, biWidth: i32 = 0, biHeight: i32 = 0,
    biPlanes: u16 = 1, biBitCount: u16 = 32, biCompression: u32 = 0,
    biSizeImage: u32 = 0, biXPelsPerMeter: i32 = 0, biYPelsPerMeter: i32 = 0,
    biClrUsed: u32 = 0, biClrImportant: u32 = 0,
};
const BITMAPINFO = extern struct { bmiHeader: BITMAPINFOHEADER = .{}, bmiColors: [1]u32 = .{0} };
const SIZE = extern struct { cx: i32 = 0, cy: i32 = 0 };

extern "gdi32" fn CreateCompatibleDC(HDC) callconv(.c) HDC;
extern "gdi32" fn DeleteDC(HDC) callconv(.c) BOOL;
extern "gdi32" fn CreateDIBSection(HDC, *const BITMAPINFO, u32, *?*anyopaque, ?*anyopaque, u32) callconv(.c) HBITMAP;
extern "gdi32" fn DeleteObject(HGDIOBJ) callconv(.c) BOOL;
extern "gdi32" fn SelectObject(HDC, HGDIOBJ) callconv(.c) HGDIOBJ;
extern "gdi32" fn CreateFontW(i32, i32, i32, i32, i32, u32, u32, u32, u32, u32, u32, u32, u32, LPCWSTR) callconv(.c) HFONT;
extern "gdi32" fn SetBkMode(HDC, i32) callconv(.c) i32;
extern "gdi32" fn SetTextColor(HDC, u32) callconv(.c) u32;
extern "gdi32" fn SetBkColor(HDC, u32) callconv(.c) u32;
extern "gdi32" fn GetTextExtentPoint32W(HDC, [*]const u16, i32, *SIZE) callconv(.c) BOOL;
extern "gdi32" fn TextOutW(HDC, i32, i32, [*]const u16, i32) callconv(.c) BOOL;

// ================================================================
// IID constants
// ================================================================

const IID_ID3D12Device = GUID{ .Data1 = 0x189819f1, .Data2 = 0x1db6, .Data3 = 0x4b57, .Data4 = .{ 0xbe, 0x54, 0x18, 0x21, 0x33, 0x9b, 0x85, 0xf7 } };
const IID_ID3D12CommandQueue = GUID{ .Data1 = 0x0ec870a6, .Data2 = 0x5d7e, .Data3 = 0x4c22, .Data4 = .{ 0x8c, 0xfc, 0x5b, 0xaa, 0xe0, 0x76, 0x16, 0xed } };
const IID_ID3D12CommandAllocator = GUID{ .Data1 = 0x6102dee4, .Data2 = 0xaf59, .Data3 = 0x4b09, .Data4 = .{ 0xb9, 0x99, 0xb4, 0x4d, 0x73, 0xf0, 0x9b, 0x24 } };
const IID_ID3D12GraphicsCommandList = GUID{ .Data1 = 0x5b160d0f, .Data2 = 0xac1b, .Data3 = 0x4185, .Data4 = .{ 0x8b, 0xa8, 0xb3, 0xae, 0x42, 0xa5, 0xa4, 0x55 } };
const IID_ID3D12Fence = GUID{ .Data1 = 0x0a753dcf, .Data2 = 0xc4d8, .Data3 = 0x4b91, .Data4 = .{ 0xad, 0xf6, 0xbe, 0x5a, 0x60, 0xd9, 0x5a, 0x76 } };
const IID_ID3D12DescriptorHeap = GUID{ .Data1 = 0x8efb471d, .Data2 = 0x616c, .Data3 = 0x4f49, .Data4 = .{ 0x90, 0xf7, 0x12, 0x7b, 0xb7, 0x63, 0xfa, 0x51 } };
const IID_ID3D12RootSignature = GUID{ .Data1 = 0xc54a6b66, .Data2 = 0x72df, .Data3 = 0x4ee8, .Data4 = .{ 0x8b, 0xe5, 0xa9, 0x46, 0xa1, 0x42, 0x92, 0x14 } };
const IID_ID3D12PipelineState = GUID{ .Data1 = 0x765a30f3, .Data2 = 0xf624, .Data3 = 0x4c6f, .Data4 = .{ 0xa8, 0x28, 0xac, 0xe9, 0x48, 0x62, 0x24, 0x45 } };
const IID_ID3D12Resource = GUID{ .Data1 = 0x696442be, .Data2 = 0xa72e, .Data3 = 0x4059, .Data4 = .{ 0xbc, 0x79, 0x5b, 0x5c, 0x98, 0x04, 0x0f, 0xad } };
const IID_IDXGIFactory4 = GUID{ .Data1 = 0x1bc6ea02, .Data2 = 0xef36, .Data3 = 0x464f, .Data4 = .{ 0xbf, 0x0c, 0x21, 0xca, 0x39, 0xe5, 0x16, 0x8a } };
const IID_IDXGIAdapter = GUID{ .Data1 = 0x2411e7e1, .Data2 = 0x12ac, .Data3 = 0x4ccf, .Data4 = .{ 0xbd, 0x14, 0x97, 0x98, 0xe8, 0x53, 0x4d, 0xc0 } };
const IID_IDWriteFactory = GUID{ .Data1 = 0xb859ee5a, .Data2 = 0xd838, .Data3 = 0x4b5b, .Data4 = .{ 0xa2, 0xe8, 0x1a, 0xdc, 0x7d, 0x93, 0xdb, 0x48 } };

// ================================================================
// Atlas constants
// ================================================================

const ATLAS_SIZE: u32 = 2048; // 2048x2048 R8 atlas
const MAX_GLYPHS: u32 = 4096;
const MAX_CELLS: u32 = 16384; // 200 cols × 80 rows fits easily

// ================================================================
// GpuCell — matches oag_gpu_cell_t in gpu_grid.h
// ================================================================

pub const GpuCell = extern struct {
    glyph_idx: u32 = 0,
    fg_rgba: u32 = 0xFFFFFFFF,
    bg_rgba: u32 = 0xFF000000,
    flags: u32 = 0,
};

// ================================================================
// GpuGridRenderer
// ================================================================

pub const GpuGridRenderer = struct {
    alloc: Allocator,

    // DX12 core
    device: *ID3D12Device = undefined,
    cmd_queue: *ID3D12CommandQueue = undefined,
    cmd_alloc: *ID3D12CommandAllocator = undefined,
    cmd_list: *ID3D12GraphicsCommandList = undefined,
    root_sig: *anyopaque = undefined,
    pipeline: *anyopaque = undefined,

    // Descriptor heaps
    srv_heap: *ID3D12DescriptorHeap = undefined,
    rtv_heap: *ID3D12DescriptorHeap = undefined,
    srv_increment: u32 = 0,

    // Fence
    fence: *ID3D12Fence = undefined,
    fence_value: u64 = 0,
    fence_event: ?HANDLE = null,

    // Glyph atlas (R8, ATLAS_SIZE x ATLAS_SIZE)
    atlas_texture: *ID3D12Resource = undefined,
    atlas_upload: *ID3D12Resource = undefined,
    atlas_bitmap: []u8 = &.{},
    atlas_dirty: bool = false,
    glyph_cell_w: u32 = 0,
    glyph_cell_h: u32 = 0,
    atlas_cols: u32 = 0,
    atlas_rows: u32 = 0,
    glyph_count: u32 = 0,

    // Glyph hash map: codepoint → atlas index
    glyph_map_keys: [MAX_GLYPHS]u32 = [_]u32{0} ** MAX_GLYPHS,
    glyph_map_vals: [MAX_GLYPHS]u32 = [_]u32{0} ** MAX_GLYPHS,
    glyph_map_used: [MAX_GLYPHS]bool = [_]bool{false} ** MAX_GLYPHS,

    // Cell structured buffer (UPLOAD heap, persistently mapped)
    cell_buffer: *ID3D12Resource = undefined,
    cell_mapped: ?[*]GpuCell = null,
    cell_capacity: u32 = MAX_CELLS,

    // Render target + readback
    render_target: *ID3D12Resource = undefined,
    readback_buffer: *ID3D12Resource = undefined,
    readback_mapped: ?[*]u8 = null,
    width: u32 = 0,
    height: u32 = 0,

    // DirectWrite / GDI
    dwrite_factory: *IDWriteFactory = undefined,
    dwrite_font_face: *IDWriteFontFace = undefined,
    gdi_dc: HDC = null,
    gdi_dib: HBITMAP = null,
    gdi_dib_bits: ?*anyopaque = null,
    gdi_font: HFONT = null,
    font_size: f32 = 14.0,
    font_baseline: f32 = 0,

    initialized: bool = false,

    // ----------------------------------------------------------------
    // init — create all DX12 resources
    // ----------------------------------------------------------------
    pub fn init(alloc: Allocator, w: u32, h: u32, font_size: f32) ?*GpuGridRenderer {
        const self = alloc.create(GpuGridRenderer) catch return null;
        self.* = GpuGridRenderer{ .alloc = alloc, .width = w, .height = h, .font_size = font_size };

        if (!self.initDevice()) return null;
        if (!self.initCommandObjects()) return null;
        if (!self.initFence()) return null;
        if (!self.initDirectWrite()) return null;
        if (!self.initAtlas()) return null;
        if (!self.initCellBuffer()) return null;
        if (!self.initRenderTargetAndReadback()) return null;
        if (!self.initDescriptorHeaps()) return null;
        if (!self.initRootSignatureAndPipeline()) return null;

        self.initialized = true;
        return self;
    }

    // ----------------------------------------------------------------
    // destroy — release all COM objects
    // ----------------------------------------------------------------
    pub fn destroy(self: *GpuGridRenderer) void {
        if (self.initialized) {
            self.waitGpu();
            // Release DX12 objects (order: derived → base)
            _ = @as(*IUnknown, @ptrCast(@alignCast(self.pipeline))).Release();
            _ = @as(*IUnknown, @ptrCast(@alignCast(self.root_sig))).Release();
            self.readback_buffer.Unmap(0, null);
            _ = self.readback_buffer.Release();
            _ = self.render_target.Release();
            self.cell_buffer.Unmap(0, null);
            _ = self.cell_buffer.Release();
            _ = self.atlas_upload.Release();
            _ = self.atlas_texture.Release();
            _ = self.rtv_heap.Release();
            _ = self.srv_heap.Release();
            if (self.fence_event) |ev| _ = CloseHandle(ev);
            _ = self.fence.Release();
            _ = self.cmd_list.Release();
            _ = self.cmd_alloc.Release();
            _ = self.cmd_queue.Release();
            // DirectWrite / GDI
            _ = self.dwrite_font_face.Release();
            _ = self.dwrite_factory.Release();
            if (self.gdi_font) |f| _ = DeleteObject(f);
            if (self.gdi_dib) |d| _ = DeleteObject(d);
            if (self.gdi_dc) |dc| _ = DeleteDC(dc);
            _ = self.device.Release();
        }
        if (self.atlas_bitmap.len > 0) self.alloc.free(self.atlas_bitmap);
        self.alloc.destroy(self);
    }

    // ----------------------------------------------------------------
    // rasterizeGlyph — DirectWrite → R8 alpha into atlas
    // ----------------------------------------------------------------
    pub fn rasterizeGlyph(self: *GpuGridRenderer, codepoint: u32) u32 {
        // Check hash map
        const slot = self.glyphMapLookup(codepoint);
        if (slot) |idx| return idx;

        if (self.glyph_count >= MAX_GLYPHS) return 0;

        // Get glyph index from DirectWrite
        var glyph_idx: u16 = 0;
        const cp = codepoint;
        _ = self.dwrite_font_face.GetGlyphIndices(@ptrCast(&cp), 1, @ptrCast(&glyph_idx));

        // Create glyph run
        const advance: f32 = @floatFromInt(self.glyph_cell_w);
        const glyph_run = DWRITE_GLYPH_RUN{
            .fontFace = self.dwrite_font_face,
            .fontEmSize = self.font_size,
            .glyphCount = 1,
            .glyphIndices = @ptrCast(&glyph_idx),
            .glyphAdvances = @ptrCast(&advance),
        };

        // Create glyph run analysis
        var analysis_ptr: ?*anyopaque = null;
        const hr = self.dwrite_factory.CreateGlyphRunAnalysis(
            &glyph_run, 1.0, null, .ALIASED, .NATURAL, 0, self.font_baseline, &analysis_ptr,
        );
        if (!SUCCEEDED(hr) or analysis_ptr == null) return 0;
        const analysis: *IDWriteGlyphRunAnalysis = @ptrCast(@alignCast(analysis_ptr.?));
        defer _ = analysis.Release();

        // Get bounds
        var bounds: RECT = .{ .left = 0, .top = 0, .right = 0, .bottom = 0 };
        _ = analysis.GetAlphaTextureBounds(.ALIASED_1x1, &bounds);

        const gw: u32 = @intCast(@max(0, bounds.right - bounds.left));
        const gh: u32 = @intCast(@max(0, bounds.bottom - bounds.top));
        if (gw == 0 or gh == 0) {
            // Empty glyph (space) — assign slot but no pixels
            const new_idx = self.glyph_count + 1;
            self.glyphMapInsert(codepoint, new_idx);
            self.glyph_count += 1;
            return new_idx;
        }

        // Get alpha texture
        const buf_size = gw * gh;
        var tmp_buf: [256 * 256]u8 = undefined;
        if (buf_size > tmp_buf.len) return 0;
        _ = analysis.CreateAlphaTexture(.ALIASED_1x1, &bounds, &tmp_buf, buf_size);

        // Place in atlas grid
        const new_idx = self.glyph_count + 1;
        const atlas_col = (new_idx - 1) % self.atlas_cols;
        const atlas_row = (new_idx - 1) / self.atlas_cols;
        if (atlas_row >= self.atlas_rows) return 0;

        const dst_x = atlas_col * self.glyph_cell_w;
        const dst_y = atlas_row * self.glyph_cell_h;

        // Copy to atlas bitmap (clamp to cell size)
        const cw = @min(gw, self.glyph_cell_w);
        const ch = @min(gh, self.glyph_cell_h);
        for (0..ch) |row| {
            for (0..cw) |col| {
                const src_off = row * gw + col;
                const dst_off = (dst_y + @as(u32, @intCast(row))) * ATLAS_SIZE + (dst_x + @as(u32, @intCast(col)));
                if (dst_off < self.atlas_bitmap.len and src_off < buf_size) {
                    self.atlas_bitmap[dst_off] = tmp_buf[src_off];
                }
            }
        }

        self.glyphMapInsert(codepoint, new_idx);
        self.glyph_count += 1;
        self.atlas_dirty = true;
        return new_idx;
    }

    // ----------------------------------------------------------------
    // updateCells — write dirty cells into mapped cell buffer
    // ----------------------------------------------------------------
    pub fn updateCells(self: *GpuGridRenderer, cells: [*]const GpuCell, count: u32, dirty_indices: [*]const u32, dirty_count: u32) void {
        const mapped = self.cell_mapped orelse return;
        if (dirty_count == 0) {
            // Full update
            const n = @min(count, self.cell_capacity);
            @memcpy(mapped[0..n], cells[0..n]);
        } else {
            // Sparse dirty update
            for (0..dirty_count) |i| {
                const idx = dirty_indices[i];
                if (idx < count and idx < self.cell_capacity) {
                    mapped[idx] = cells[idx];
                }
            }
        }
    }

    // ----------------------------------------------------------------
    // renderFrame — execute GPU pipeline, return readback pixels
    // ----------------------------------------------------------------
    pub fn renderFrame(self: *GpuGridRenderer, cell_count: u32, term_cols: u32, cell_w: f32, cell_h: f32) ?[*]const u8 {
        if (!self.initialized) return null;

        // Upload atlas if dirty
        if (self.atlas_dirty) self.uploadAtlas();

        // Reset command list
        _ = self.cmd_alloc.Reset();
        _ = self.cmd_list.Reset(@ptrCast(self.cmd_alloc), self.pipeline);

        // Transition RT: COPY_SOURCE → RENDER_TARGET
        var barrier = D3D12_RESOURCE_BARRIER.transition(@ptrCast(self.render_target), .COPY_SOURCE, .RENDER_TARGET);
        self.cmd_list.ResourceBarrier(1, @ptrCast(&barrier));

        // Set root signature + pipeline + descriptor heaps
        self.cmd_list.SetGraphicsRootSignature(self.root_sig);
        const heap_ptr: *anyopaque = @ptrCast(self.srv_heap);
        self.cmd_list.SetDescriptorHeaps(1, @ptrCast(&heap_ptr));

        // Descriptor table: slot 0 = atlas SRV + cell buffer SRV
        const gpu_start = self.srv_heap.GetGPUDescriptorHandleForHeapStart();
        self.cmd_list.SetGraphicsRootDescriptorTable(0, gpu_start);

        // Push constants: slot 1 = { viewport_w, viewport_h, cell_w, cell_h, atlas_cols, atlas_size, term_cols, glyph_cell_w }
        const PushConstants = extern struct {
            viewport_w: f32,
            viewport_h: f32,
            cell_w_: f32,
            cell_h_: f32,
            atlas_cols_: u32,
            atlas_size: u32,
            term_cols_: u32,
            glyph_cell_w_: u32,
        };
        const pc = PushConstants{
            .viewport_w = @floatFromInt(self.width),
            .viewport_h = @floatFromInt(self.height),
            .cell_w_ = cell_w,
            .cell_h_ = cell_h,
            .atlas_cols_ = self.atlas_cols,
            .atlas_size = ATLAS_SIZE,
            .term_cols_ = term_cols,
            .glyph_cell_w_ = self.glyph_cell_w,
        };
        self.cmd_list.SetGraphicsRoot32BitConstants(1, @sizeOf(PushConstants) / 4, @ptrCast(&pc), 0);

        // Viewport + scissor
        const vp = D3D12_VIEWPORT{ .Width = @floatFromInt(self.width), .Height = @floatFromInt(self.height) };
        self.cmd_list.RSSetViewports(1, @ptrCast(&vp));
        const scissor = RECT{ .left = 0, .top = 0, .right = @intCast(self.width), .bottom = @intCast(self.height) };
        self.cmd_list.RSSetScissorRects(1, @ptrCast(&scissor));

        // Set render target
        const rtv_handle = self.rtv_heap.GetCPUDescriptorHandleForHeapStart();
        self.cmd_list.OMSetRenderTargets(1, @ptrCast(&rtv_handle), FALSE, null);

        // Clear
        const clear_color = [4]f32{ 0, 0, 0, 1 };
        self.cmd_list.ClearRenderTargetView(rtv_handle, &clear_color, 0, null);

        // Draw: 6 vertices per cell (fullscreen quad), instanced per cell
        self.cmd_list.IASetPrimitiveTopology(.TRIANGLELIST);
        self.cmd_list.DrawInstanced(6, cell_count, 0, 0);

        // Transition RT: RENDER_TARGET → COPY_SOURCE
        barrier = D3D12_RESOURCE_BARRIER.transition(@ptrCast(self.render_target), .RENDER_TARGET, .COPY_SOURCE);
        self.cmd_list.ResourceBarrier(1, @ptrCast(&barrier));

        // Copy RT → readback
        self.cmd_list.CopyResource(@ptrCast(self.readback_buffer), @ptrCast(self.render_target));

        // Close + execute
        _ = self.cmd_list.Close();
        const cmd_list_ptr: *anyopaque = @ptrCast(self.cmd_list);
        self.cmd_queue.ExecuteCommandLists(1, @ptrCast(&cmd_list_ptr));

        // Wait GPU
        self.waitGpu();

        return self.readback_mapped;
    }

    // ----------------------------------------------------------------
    // resize — recreate render target + readback for new dimensions
    // ----------------------------------------------------------------
    pub fn resize(self: *GpuGridRenderer, w: u32, h: u32) bool {
        if (w == self.width and h == self.height) return true;
        self.waitGpu();

        // Release old
        self.readback_buffer.Unmap(0, null);
        _ = self.readback_buffer.Release();
        _ = self.render_target.Release();

        self.width = w;
        self.height = h;

        // Recreate
        if (!self.initRenderTargetAndReadback()) return false;

        // Recreate RTV
        const rtv_handle = self.rtv_heap.GetCPUDescriptorHandleForHeapStart();
        self.device.CreateRenderTargetView(@ptrCast(self.render_target), null, rtv_handle);

        return true;
    }

    // ================================================================
    // Private helpers
    // ================================================================

    fn initDevice(self: *GpuGridRenderer) bool {
        // Create DXGI factory → WARP adapter → D3D12 device
        var factory_ptr: ?*anyopaque = null;
        if (!SUCCEEDED(CreateDXGIFactory1(&IID_IDXGIFactory4, &factory_ptr))) return false;
        const factory: *IDXGIFactory4 = @ptrCast(@alignCast(factory_ptr.?));
        defer _ = factory.Release();

        // Try WARP adapter for maximum compatibility
        var adapter_ptr: ?*anyopaque = null;
        _ = factory.EnumWarpAdapter(&IID_IDXGIAdapter, &adapter_ptr);

        var device_ptr: ?*anyopaque = null;
        if (!SUCCEEDED(D3D12CreateDevice(adapter_ptr, .@"11_0", &IID_ID3D12Device, &device_ptr))) {
            // Fallback: null adapter
            if (!SUCCEEDED(D3D12CreateDevice(null, .@"11_0", &IID_ID3D12Device, &device_ptr))) return false;
        }
        if (adapter_ptr) |a| _ = @as(*IUnknown, @ptrCast(@alignCast(a))).Release();
        self.device = @ptrCast(@alignCast(device_ptr.?));
        return true;
    }

    fn initCommandObjects(self: *GpuGridRenderer) bool {
        const desc = D3D12_COMMAND_QUEUE_DESC{};
        var ptr: ?*anyopaque = null;
        if (!SUCCEEDED(self.device.CreateCommandQueue(&desc, &IID_ID3D12CommandQueue, &ptr))) return false;
        self.cmd_queue = @ptrCast(@alignCast(ptr.?));

        ptr = null;
        if (!SUCCEEDED(self.device.CreateCommandAllocator(.DIRECT, &IID_ID3D12CommandAllocator, &ptr))) return false;
        self.cmd_alloc = @ptrCast(@alignCast(ptr.?));

        ptr = null;
        if (!SUCCEEDED(self.device.CreateCommandList(0, .DIRECT, @ptrCast(self.cmd_alloc), null, &IID_ID3D12GraphicsCommandList, &ptr))) return false;
        self.cmd_list = @ptrCast(@alignCast(ptr.?));
        _ = self.cmd_list.Close();
        return true;
    }

    fn initFence(self: *GpuGridRenderer) bool {
        var ptr: ?*anyopaque = null;
        if (!SUCCEEDED(self.device.CreateFence(0, .NONE, &IID_ID3D12Fence, &ptr))) return false;
        self.fence = @ptrCast(@alignCast(ptr.?));
        self.fence_event = CreateEventW(null, FALSE, FALSE, null);
        return self.fence_event != null;
    }

    fn initDirectWrite(self: *GpuGridRenderer) bool {
        // DWrite factory
        var factory_ptr: ?*anyopaque = null;
        if (!SUCCEEDED(DWriteCreateFactory(.SHARED, &IID_IDWriteFactory, &factory_ptr))) return false;
        self.dwrite_factory = @ptrCast(@alignCast(factory_ptr.?));

        // GDI interop → font face
        var interop_ptr: ?*anyopaque = null;
        if (!SUCCEEDED(self.dwrite_factory.GetGdiInterop(&interop_ptr))) return false;
        const interop: *IDWriteGdiInterop = @ptrCast(@alignCast(interop_ptr.?));
        defer _ = interop.Release();

        // Create GDI DC + font
        self.gdi_dc = CreateCompatibleDC(null);
        if (self.gdi_dc == null) return false;

        const font_height: i32 = -@as(i32, @intFromFloat(self.font_size));
        const face_name = [_:0]u16{ 'C', 'o', 'n', 's', 'o', 'l', 'a', 's', 0 };
        self.gdi_font = CreateFontW(
            font_height, 0, 0, 0, 400, 0, 0, 0,
            0, 0, 0, 5, // CLEARTYPE_QUALITY
            0x31, // FIXED_PITCH | FF_MODERN
            &face_name,
        );
        if (self.gdi_font == null) return false;
        _ = SelectObject(self.gdi_dc, self.gdi_font);

        // Create DWrite font face from GDI
        var face_ptr: ?*anyopaque = null;
        if (!SUCCEEDED(interop.CreateFontFaceFromHdc(self.gdi_dc, &face_ptr))) return false;
        self.dwrite_font_face = @ptrCast(@alignCast(face_ptr.?));

        // Measure cell size via GDI
        var size: SIZE = .{};
        const ch: u16 = 'M';
        _ = GetTextExtentPoint32W(self.gdi_dc, @ptrCast(&ch), 1, &size);
        self.glyph_cell_w = @intCast(@max(1, size.cx));
        self.glyph_cell_h = @intCast(@max(1, size.cy));

        // Get font metrics for baseline
        var metrics: DWRITE_FONT_METRICS = .{};
        _ = self.dwrite_font_face.GetGdiCompatibleMetrics(self.font_size, 1.0, null, &metrics);
        const em: f32 = @floatFromInt(metrics.designUnitsPerEm);
        if (em > 0) {
            self.font_baseline = self.font_size * @as(f32, @floatFromInt(metrics.ascent)) / em;
        }

        // Create GDI DIB for rasterization scratch buffer
        var bmi = BITMAPINFO{
            .bmiHeader = .{
                .biWidth = @intCast(self.glyph_cell_w),
                .biHeight = -@as(i32, @intCast(self.glyph_cell_h)), // top-down
                .biBitCount = 32,
            },
        };
        self.gdi_dib = CreateDIBSection(self.gdi_dc, &bmi, 0, &self.gdi_dib_bits, null, 0);

        return true;
    }

    fn initAtlas(self: *GpuGridRenderer) bool {
        self.atlas_cols = ATLAS_SIZE / @max(1, self.glyph_cell_w);
        self.atlas_rows = ATLAS_SIZE / @max(1, self.glyph_cell_h);

        // CPU-side R8 bitmap
        self.atlas_bitmap = self.alloc.alloc(u8, ATLAS_SIZE * ATLAS_SIZE) catch return false;
        @memset(self.atlas_bitmap, 0);

        // GPU atlas texture (DEFAULT heap, R8_UNORM)
        const tex_desc = D3D12_RESOURCE_DESC{
            .Dimension = .TEXTURE2D,
            .Width = ATLAS_SIZE,
            .Height = ATLAS_SIZE,
            .Format = .R8_UNORM,
        };
        const default_heap = D3D12_HEAP_PROPERTIES{ .Type = .DEFAULT };
        var ptr: ?*anyopaque = null;
        if (!SUCCEEDED(self.device.CreateCommittedResource(&default_heap, .NONE, &tex_desc, .COPY_DEST, null, &IID_ID3D12Resource, &ptr))) return false;
        self.atlas_texture = @ptrCast(@alignCast(ptr.?));

        // Upload buffer for atlas
        const row_pitch = (ATLAS_SIZE + 255) & ~@as(u32, 255); // 256-byte aligned
        const upload_size: u64 = @as(u64, row_pitch) * ATLAS_SIZE;
        const upload_desc = D3D12_RESOURCE_DESC{ .Width = upload_size, .Layout = .ROW_MAJOR };
        const upload_heap = D3D12_HEAP_PROPERTIES{ .Type = .UPLOAD };
        ptr = null;
        if (!SUCCEEDED(self.device.CreateCommittedResource(&upload_heap, .NONE, &upload_desc, .GENERIC_READ, null, &IID_ID3D12Resource, &ptr))) return false;
        self.atlas_upload = @ptrCast(@alignCast(ptr.?));

        return true;
    }

    fn initCellBuffer(self: *GpuGridRenderer) bool {
        const buf_size: u64 = @as(u64, self.cell_capacity) * @sizeOf(GpuCell);
        const buf_desc = D3D12_RESOURCE_DESC{ .Width = buf_size, .Layout = .ROW_MAJOR };
        const upload_heap = D3D12_HEAP_PROPERTIES{ .Type = .UPLOAD };
        var ptr: ?*anyopaque = null;
        if (!SUCCEEDED(self.device.CreateCommittedResource(&upload_heap, .NONE, &buf_desc, .GENERIC_READ, null, &IID_ID3D12Resource, &ptr))) return false;
        self.cell_buffer = @ptrCast(@alignCast(ptr.?));

        // Persistent map
        var mapped: ?*anyopaque = null;
        if (!SUCCEEDED(self.cell_buffer.Map(0, null, &mapped))) return false;
        self.cell_mapped = @ptrCast(@alignCast(mapped.?));

        return true;
    }

    fn initRenderTargetAndReadback(self: *GpuGridRenderer) bool {
        // Render target (RGBA8, DEFAULT heap)
        var clear_val = D3D12_CLEAR_VALUE{ .Format = .R8G8B8A8_UNORM, .Color = .{ 0, 0, 0, 1 } };
        const rt_desc = D3D12_RESOURCE_DESC{
            .Dimension = .TEXTURE2D,
            .Width = self.width,
            .Height = self.height,
            .Format = .R8G8B8A8_UNORM,
            .Flags = .ALLOW_RENDER_TARGET,
        };
        const default_heap = D3D12_HEAP_PROPERTIES{ .Type = .DEFAULT };
        var ptr: ?*anyopaque = null;
        if (!SUCCEEDED(self.device.CreateCommittedResource(&default_heap, .NONE, &rt_desc, .COPY_SOURCE, &clear_val, &IID_ID3D12Resource, &ptr))) return false;
        self.render_target = @ptrCast(@alignCast(ptr.?));

        // Readback buffer
        const rb_size: u64 = @as(u64, self.width) * self.height * 4;
        const rb_desc = D3D12_RESOURCE_DESC{ .Width = rb_size, .Layout = .ROW_MAJOR };
        const rb_heap = D3D12_HEAP_PROPERTIES{ .Type = .READBACK };
        ptr = null;
        if (!SUCCEEDED(self.device.CreateCommittedResource(&rb_heap, .NONE, &rb_desc, .COPY_DEST, null, &IID_ID3D12Resource, &ptr))) return false;
        self.readback_buffer = @ptrCast(@alignCast(ptr.?));

        var mapped: ?*anyopaque = null;
        if (!SUCCEEDED(self.readback_buffer.Map(0, null, &mapped))) return false;
        self.readback_mapped = @ptrCast(@alignCast(mapped.?));

        return true;
    }

    fn initDescriptorHeaps(self: *GpuGridRenderer) bool {
        // SRV heap: 2 descriptors (atlas texture + cell buffer)
        const srv_desc = D3D12_DESCRIPTOR_HEAP_DESC{
            .Type = .CBV_SRV_UAV,
            .NumDescriptors = 2,
            .Flags = .SHADER_VISIBLE,
        };
        var ptr: ?*anyopaque = null;
        if (!SUCCEEDED(self.device.CreateDescriptorHeap(&srv_desc, &IID_ID3D12DescriptorHeap, &ptr))) return false;
        self.srv_heap = @ptrCast(@alignCast(ptr.?));
        self.srv_increment = self.device.GetDescriptorHandleIncrementSize(.CBV_SRV_UAV);

        // Create SRVs
        const cpu_start = self.srv_heap.GetCPUDescriptorHandleForHeapStart();

        // [0] Atlas texture SRV (R8_UNORM, Texture2D)
        const atlas_srv = D3D12_SHADER_RESOURCE_VIEW_DESC{
            .Format = .R8_UNORM,
            .ViewDimension = .TEXTURE2D,
            .u = .{ .Texture2D = .{ .MipLevels = 1 } },
        };
        self.device.CreateShaderResourceView(@ptrCast(self.atlas_texture), &atlas_srv, cpu_start);

        // [1] Cell buffer SRV (structured buffer)
        const cell_srv = D3D12_SHADER_RESOURCE_VIEW_DESC{
            .Format = .UNKNOWN,
            .ViewDimension = .BUFFER,
            .u = .{ .Buffer = .{
                .NumElements = self.cell_capacity,
                .StructureByteStride = @sizeOf(GpuCell),
            } },
        };
        const cell_handle = D3D12_CPU_DESCRIPTOR_HANDLE{ .ptr = cpu_start.ptr + self.srv_increment };
        self.device.CreateShaderResourceView(@ptrCast(self.cell_buffer), &cell_srv, cell_handle);

        // RTV heap: 1 descriptor
        const rtv_desc = D3D12_DESCRIPTOR_HEAP_DESC{ .Type = .RTV, .NumDescriptors = 1 };
        ptr = null;
        if (!SUCCEEDED(self.device.CreateDescriptorHeap(&rtv_desc, &IID_ID3D12DescriptorHeap, &ptr))) return false;
        self.rtv_heap = @ptrCast(@alignCast(ptr.?));

        const rtv_handle = self.rtv_heap.GetCPUDescriptorHandleForHeapStart();
        self.device.CreateRenderTargetView(@ptrCast(self.render_target), null, rtv_handle);

        return true;
    }

    fn initRootSignatureAndPipeline(self: *GpuGridRenderer) bool {
        // Root signature: 1 descriptor table (2 SRVs) + 1 root constants (8 u32)
        var ranges = [_]D3D12_DESCRIPTOR_RANGE{
            .{ .RangeType = .SRV, .NumDescriptors = 2, .BaseShaderRegister = 0 },
        };
        var params = [_]D3D12_ROOT_PARAMETER{
            .{ // [0] descriptor table
                .ParameterType = .DESCRIPTOR_TABLE,
                .u = .{ .DescriptorTable = .{ .NumDescriptorRanges = 1, .pDescriptorRanges = &ranges } },
                .ShaderVisibility = .ALL,
            },
            .{ // [1] push constants
                .ParameterType = .@"32BIT_CONSTANTS",
                .u = .{ .Constants = .{ .ShaderRegister = 0, .Num32BitValues = 8 } },
                .ShaderVisibility = .ALL,
            },
        };
        var sampler = D3D12_STATIC_SAMPLER_DESC{};
        const rs_desc = D3D12_ROOT_SIGNATURE_DESC{
            .NumParameters = 2,
            .pParameters = &params,
            .NumStaticSamplers = 1,
            .pStaticSamplers = @ptrCast(&sampler),
        };

        var blob: ?*ID3DBlob = null;
        var err_blob: ?*ID3DBlob = null;
        if (!SUCCEEDED(D3D12SerializeRootSignature(&rs_desc, .@"1_0", &blob, &err_blob))) {
            if (err_blob) |e| _ = e.Release();
            return false;
        }
        defer _ = blob.?.Release();
        if (err_blob) |e| _ = e.Release();

        var rs_ptr: ?*anyopaque = null;
        if (!SUCCEEDED(self.device.CreateRootSignature(0, blob.?.GetBufferPointer(), blob.?.GetBufferSize(), &IID_ID3D12RootSignature, &rs_ptr))) return false;
        self.root_sig = rs_ptr.?;

        // Pre-compiled shader bytecode (fxc SM 5.1)
        const vs_cso = @embedFile("shaders/grid_vs.cso");
        const ps_cso = @embedFile("shaders/grid_ps.cso");

        // Pipeline state
        var pso_desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC{
            .pRootSignature = self.root_sig,
            .VS = .{ .pShaderBytecode = vs_cso.ptr, .BytecodeLength = vs_cso.len },
            .PS = .{ .pShaderBytecode = ps_cso.ptr, .BytecodeLength = ps_cso.len },
        };
        pso_desc.RTVFormats[0] = .R8G8B8A8_UNORM;

        var pso_ptr: ?*anyopaque = null;
        if (!SUCCEEDED(self.device.CreateGraphicsPipelineState(&pso_desc, &IID_ID3D12PipelineState, &pso_ptr))) return false;
        self.pipeline = pso_ptr.?;

        return true;
    }

    fn uploadAtlas(self: *GpuGridRenderer) void {
        // Map upload buffer, copy atlas_bitmap row by row (respecting row pitch alignment)
        var mapped: ?*anyopaque = null;
        if (!SUCCEEDED(self.atlas_upload.Map(0, null, &mapped))) return;
        const dst: [*]u8 = @ptrCast(@alignCast(mapped.?));
        const row_pitch = (ATLAS_SIZE + 255) & ~@as(u32, 255);

        for (0..ATLAS_SIZE) |row| {
            const src_off = row * ATLAS_SIZE;
            const dst_off = row * row_pitch;
            @memcpy(dst[dst_off .. dst_off + ATLAS_SIZE], self.atlas_bitmap[src_off .. src_off + ATLAS_SIZE]);
        }
        self.atlas_upload.Unmap(0, null);

        // Record copy command
        _ = self.cmd_alloc.Reset();
        _ = self.cmd_list.Reset(@ptrCast(self.cmd_alloc), null);

        const src_loc = D3D12_TEXTURE_COPY_LOCATION{
            .pResource = @ptrCast(self.atlas_upload),
            .Type = 1, // PLACED_FOOTPRINT
            .u = .{ .PlacedFootprint = .{
                .Format = .R8_UNORM,
                .Width = ATLAS_SIZE,
                .Height = ATLAS_SIZE,
                .RowPitch = row_pitch,
            } },
        };
        const dst_loc = D3D12_TEXTURE_COPY_LOCATION{
            .pResource = @ptrCast(self.atlas_texture),
            .Type = 0, // SUBRESOURCE_INDEX
            .u = .{ .SubresourceIndex = 0 },
        };
        self.cmd_list.CopyTextureRegion(&dst_loc, 0, 0, 0, &src_loc, null);

        // Transition atlas: COPY_DEST → PIXEL_SHADER_RESOURCE
        var barrier = D3D12_RESOURCE_BARRIER.transition(@ptrCast(self.atlas_texture), .COPY_DEST, .PIXEL_SHADER_RESOURCE);
        self.cmd_list.ResourceBarrier(1, @ptrCast(&barrier));

        _ = self.cmd_list.Close();
        const cmd_ptr: *anyopaque = @ptrCast(self.cmd_list);
        self.cmd_queue.ExecuteCommandLists(1, @ptrCast(&cmd_ptr));
        self.waitGpu();

        // Transition back for next upload
        _ = self.cmd_alloc.Reset();
        _ = self.cmd_list.Reset(@ptrCast(self.cmd_alloc), null);
        barrier = D3D12_RESOURCE_BARRIER.transition(@ptrCast(self.atlas_texture), .PIXEL_SHADER_RESOURCE, .COPY_DEST);
        self.cmd_list.ResourceBarrier(1, @ptrCast(&barrier));
        _ = self.cmd_list.Close();
        const cmd_ptr2: *anyopaque = @ptrCast(self.cmd_list);
        self.cmd_queue.ExecuteCommandLists(1, @ptrCast(&cmd_ptr2));
        self.waitGpu();

        self.atlas_dirty = false;
    }

    fn waitGpu(self: *GpuGridRenderer) void {
        self.fence_value += 1;
        _ = self.cmd_queue.Signal(@ptrCast(self.fence), self.fence_value);
        if (self.fence.GetCompletedValue() < self.fence_value) {
            _ = self.fence.SetEventOnCompletion(self.fence_value, self.fence_event.?);
            _ = WaitForSingleObject(self.fence_event.?, 2); // 2ms max (GPU frame latency rule)
        }
    }

    // Simple hash map for codepoint → atlas index
    fn glyphMapHash(codepoint: u32) u32 {
        var h = codepoint;
        h = ((h >> 16) ^ h) *% 0x45d9f3b;
        h = ((h >> 16) ^ h) *% 0x45d9f3b;
        h = (h >> 16) ^ h;
        return h % MAX_GLYPHS;
    }

    fn glyphMapLookup(self: *GpuGridRenderer, codepoint: u32) ?u32 {
        var idx = glyphMapHash(codepoint);
        var probes: u32 = 0;
        while (probes < MAX_GLYPHS) : (probes += 1) {
            if (!self.glyph_map_used[idx]) return null;
            if (self.glyph_map_keys[idx] == codepoint) return self.glyph_map_vals[idx];
            idx = (idx + 1) % MAX_GLYPHS;
        }
        return null;
    }

    fn glyphMapInsert(self: *GpuGridRenderer, codepoint: u32, value: u32) void {
        var idx = glyphMapHash(codepoint);
        var probes: u32 = 0;
        while (probes < MAX_GLYPHS) : (probes += 1) {
            if (!self.glyph_map_used[idx]) {
                self.glyph_map_keys[idx] = codepoint;
                self.glyph_map_vals[idx] = value;
                self.glyph_map_used[idx] = true;
                return;
            }
            if (self.glyph_map_keys[idx] == codepoint) {
                self.glyph_map_vals[idx] = value;
                return;
            }
            idx = (idx + 1) % MAX_GLYPHS;
        }
    }
};

// ================================================================
// Shader bytecode — pre-compiled CSO loaded via @embedFile above.
// HLSL source is in src/render/shaders/grid.hlsl.
// ================================================================

// (removed embedded HLSL — using grid_vs.cso / grid_ps.cso)
const _vs_hlsl_removed: []const u8 =
    \\cbuffer PushConstants : register(b0) {
    \\    float viewport_w;
    \\    float viewport_h;
    \\    float cell_w;
    \\    float cell_h;
    \\    uint atlas_cols;
    \\    uint atlas_size;
    \\    uint term_cols;
    \\    uint glyph_cell_w;
    \\};
    \\
    \\struct CellData {
    \\    uint glyph_idx;
    \\    uint fg_rgba;
    \\    uint bg_rgba;
    \\    uint flags;
    \\};
    \\StructuredBuffer<CellData> cells : register(t1);
    \\
    \\struct VS_OUT {
    \\    float4 pos : SV_Position;
    \\    float2 uv : TEXCOORD0;
    \\    nointerpolation uint cell_id : TEXCOORD1;
    \\};
    \\
    \\VS_OUT VSMain(uint vid : SV_VertexID, uint iid : SV_InstanceID) {
    \\    VS_OUT o;
    \\    o.cell_id = iid;
    \\    uint col = iid % term_cols;
    \\    uint row = iid / term_cols;
    \\    float x0 = col * cell_w;
    \\    float y0 = row * cell_h;
    \\    // 6 vertices: 2 triangles for a quad
    \\    float2 corners[6] = {
    \\        float2(0, 0), float2(1, 0), float2(0, 1),
    \\        float2(1, 0), float2(1, 1), float2(0, 1)
    \\    };
    \\    float2 c = corners[vid];
    \\    float px = x0 + c.x * cell_w;
    \\    float py = y0 + c.y * cell_h;
    \\    o.pos = float4(px / viewport_w * 2.0 - 1.0, 1.0 - py / viewport_h * 2.0, 0, 1);
    \\    o.uv = c;
    \\    return o;
    \\}
;

const ps_hlsl: []const u8 =
    \\cbuffer PushConstants : register(b0) {
    \\    float viewport_w;
    \\    float viewport_h;
    \\    float cell_w;
    \\    float cell_h;
    \\    uint atlas_cols;
    \\    uint atlas_size;
    \\    uint term_cols;
    \\    uint glyph_cell_w;
    \\};
    \\
    \\struct CellData {
    \\    uint glyph_idx;
    \\    uint fg_rgba;
    \\    uint bg_rgba;
    \\    uint flags;
    \\};
    \\
    \\Texture2D<float> atlas : register(t0);
    \\StructuredBuffer<CellData> cells : register(t1);
    \\SamplerState samp : register(s0);
    \\
    \\struct PS_IN {
    \\    float4 pos : SV_Position;
    \\    float2 uv : TEXCOORD0;
    \\    nointerpolation uint cell_id : TEXCOORD1;
    \\};
    \\
    \\float4 unpack_rgba(uint c) {
    \\    return float4(
    \\        ((c >> 0) & 0xFF) / 255.0,
    \\        ((c >> 8) & 0xFF) / 255.0,
    \\        ((c >> 16) & 0xFF) / 255.0,
    \\        ((c >> 24) & 0xFF) / 255.0
    \\    );
    \\}
    \\
    \\float4 PSMain(PS_IN i) : SV_Target {
    \\    CellData cell = cells[i.cell_id];
    \\    float4 bg = unpack_rgba(cell.bg_rgba);
    \\    if (cell.glyph_idx == 0) return bg;
    \\
    \\    // Atlas UV lookup
    \\    uint gi = cell.glyph_idx - 1;
    \\    uint ax = (gi % atlas_cols) * glyph_cell_w;
    \\    uint ay = (gi / atlas_cols) * (uint)cell_h;
    \\    float u = (ax + i.uv.x * glyph_cell_w) / (float)atlas_size;
    \\    float v = (ay + i.uv.y * cell_h) / (float)atlas_size;
    \\    float alpha = atlas.Sample(samp, float2(u, v));
    \\
    \\    float4 fg = unpack_rgba(cell.fg_rgba);
    \\    return lerp(bg, fg, alpha);
    \\}
;

// ================================================================
// C FFI exports
// ================================================================

var gpa = std.heap.GeneralPurposeAllocator(.{}){};

export fn oag_gpu_grid_create(w: u32, h: u32, font_size: f32) ?*GpuGridRenderer {
    return GpuGridRenderer.init(gpa.allocator(), w, h, font_size);
}

export fn oag_gpu_grid_destroy(r: *GpuGridRenderer) void {
    r.destroy();
}

export fn oag_gpu_grid_update_cells(r: *GpuGridRenderer, cells: [*]const GpuCell, count: u32, dirty: [*]const u32, dirty_count: u32) void {
    r.updateCells(cells, count, dirty, dirty_count);
}

export fn oag_gpu_grid_render(r: *GpuGridRenderer, cell_count: u32, cols: u32, cw: f32, ch: f32) ?[*]const u8 {
    return r.renderFrame(cell_count, cols, cw, ch);
}

export fn oag_gpu_grid_resize(r: *GpuGridRenderer, w: u32, h: u32) u8 {
    return if (r.resize(w, h)) 1 else 0;
}

export fn oag_gpu_grid_width(r: *const GpuGridRenderer) u32 {
    return r.width;
}

export fn oag_gpu_grid_height(r: *const GpuGridRenderer) u32 {
    return r.height;
}

export fn oag_gpu_grid_pixel_stride(r: *const GpuGridRenderer) u32 {
    return r.width * 4; // RGBA8
}

export fn oag_gpu_grid_rasterize_glyph(r: *GpuGridRenderer, codepoint: u32) u32 {
    return r.rasterizeGlyph(codepoint);
}
