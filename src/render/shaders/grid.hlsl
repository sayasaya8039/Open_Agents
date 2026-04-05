// grid.hlsl — DX12 vertex + pixel shader for terminal grid rendering
//
// Compile with DXC (Shader Model 6.0):
//   dxc -T vs_6_0 -E VSMain grid.hlsl -Fo grid_vs.cso
//   dxc -T ps_6_0 -E PSMain grid.hlsl -Fo grid_ps.cso
//
// Or FXC (Shader Model 5.1):
//   fxc /T vs_5_1 /E VSMain grid.hlsl /Fo grid_vs.cso
//   fxc /T ps_5_1 /E PSMain grid.hlsl /Fo grid_ps.cso

// ── Push Constants (Root Constants, b0) ──────────────────────────
cbuffer PushConstants : register(b0) {
    float2 viewport_size;   // screen dimensions in pixels
    float2 cell_size;       // cell width/height in pixels
    float atlas_pitch_u;    // slot_pitch_w / ATLAS_SIZE
    float atlas_pitch_v;    // slot_pitch_h / ATLAS_SIZE
    float atlas_glyph_u;   // glyph_w / ATLAS_SIZE
    float atlas_glyph_v;   // glyph_h / ATLAS_SIZE
    float term_cols;        // terminal column count
    float atlas_grid_cols;  // atlas columns
    float2 _pad;            // alignment padding
};

// ── Cell Structured Buffer (t0) ──────────────────────────────────
struct CellData {
    uint glyph_idx;   // 0 = no glyph, 1+ = atlas index
    uint fg_rgba;      // packed RGBA (A<<24 | R<<16 | G<<8 | B)
    uint bg_rgba;      // packed RGBA
    uint flags;        // bit 0=bold, 1=italic, 2=underline
};

StructuredBuffer<CellData> cells : register(t0);

// ── Atlas Texture (t1) + Sampler (s0) ────────────────────────────
Texture2D<float> atlas : register(t1);          // R8_UNORM
SamplerState atlas_sampler : register(s0);       // POINT, CLAMP

// ── VS → PS interface ────────────────────────────────────────────
struct VSOutput {
    float4 position  : SV_Position;
    float4 fg_color  : COLOR0;
    float4 bg_color  : COLOR1;
    float2 uv        : TEXCOORD0;
    float  has_glyph : TEXCOORD1;
};

// ── Vertex Shader ────────────────────────────────────────────────
// 6 vertices per instance (2 triangles = 1 quad per cell)
VSOutput VSMain(uint vertex_id : SV_VertexID, uint instance_id : SV_InstanceID) {
    VSOutput output;

    // Fetch cell data for this instance
    CellData cell = cells[instance_id];
    uint col = instance_id % (uint)term_cols;
    uint row = instance_id / (uint)term_cols;

    // Quad corners: 2 triangles forming a quad
    //   0--1
    //   |\ |
    //   2--3
    // Triangle 1: 0,1,2  Triangle 2: 2,1,3
    static const float2 corners[6] = {
        float2(0, 0), float2(1, 0), float2(0, 1),  // tri 1
        float2(0, 1), float2(1, 0), float2(1, 1),  // tri 2
    };
    float2 corner = corners[vertex_id];

    // Screen position: pixel coords → NDC
    float2 pixel_pos = float2(col * cell_size.x, row * cell_size.y) + corner * cell_size;
    output.position = float4(
        pixel_pos.x / viewport_size.x *  2.0 - 1.0,
        1.0 - pixel_pos.y / viewport_size.y * 2.0,   // Y flipped for DX NDC
        0.0, 1.0
    );

    // Unpack fg color (ARGB packed as A<<24 | R<<16 | G<<8 | B)
    output.fg_color = float4(
        ((cell.fg_rgba >> 16) & 0xFF) / 255.0,
        ((cell.fg_rgba >>  8) & 0xFF) / 255.0,
        ( cell.fg_rgba        & 0xFF) / 255.0,
        ((cell.fg_rgba >> 24) & 0xFF) / 255.0
    );

    // Unpack bg color
    output.bg_color = float4(
        ((cell.bg_rgba >> 16) & 0xFF) / 255.0,
        ((cell.bg_rgba >>  8) & 0xFF) / 255.0,
        ( cell.bg_rgba        & 0xFF) / 255.0,
        ((cell.bg_rgba >> 24) & 0xFF) / 255.0
    );

    // Atlas UV calculation
    if (cell.glyph_idx > 0) {
        uint atlas_idx = cell.glyph_idx - 1;
        uint atlas_col = atlas_idx % (uint)atlas_grid_cols;
        uint atlas_row = atlas_idx / (uint)atlas_grid_cols;
        float2 atlas_origin = float2(atlas_col * atlas_pitch_u, atlas_row * atlas_pitch_v);
        output.uv = atlas_origin + corner * float2(atlas_glyph_u, atlas_glyph_v);
        output.has_glyph = 1.0;
    } else {
        output.uv = float2(0, 0);
        output.has_glyph = 0.0;
    }

    return output;
}

// ── Pixel Shader ─────────────────────────────────────────────────
// Sample glyph alpha from atlas, blend fg over bg
float4 PSMain(VSOutput input) : SV_Target {
    if (input.has_glyph > 0.5) {
        float alpha = atlas.Sample(atlas_sampler, input.uv);
        return lerp(input.bg_color, float4(input.fg_color.rgb, 1.0), alpha);
    }
    return input.bg_color;
}
