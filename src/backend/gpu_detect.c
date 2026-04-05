// Open_Agents — GPU Auto-Detection via DXGI
// Enumerates all GPUs, classifies vendor (NVIDIA/AMD/Intel), picks best backend
#include "backend/backend.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#define COBJMACROS
#define INITGUID
#include <windows.h>
#include <dxgi1_4.h>

// PCI Vendor IDs
#define PCI_VENDOR_NVIDIA  0x10DE
#define PCI_VENDOR_AMD     0x1002
#define PCI_VENDOR_INTEL   0x8086

static oag_gpu_vendor_t classify_vendor(uint32_t vendor_id) {
    switch (vendor_id) {
        case PCI_VENDOR_NVIDIA: return OAG_GPU_VENDOR_NVIDIA;
        case PCI_VENDOR_AMD:    return OAG_GPU_VENDOR_AMD;
        case PCI_VENDOR_INTEL:  return OAG_GPU_VENDOR_INTEL;
        default:                return OAG_GPU_VENDOR_UNKNOWN;
    }
}

static bool is_discrete_gpu(uint32_t vendor_id, uint64_t vram) {
    // Integrated GPUs typically share system RAM (< 512MB dedicated)
    // Discrete GPUs have their own VRAM (>= 1GB)
    if (vendor_id == PCI_VENDOR_INTEL && vram < (2ULL * 1024 * 1024 * 1024)) {
        return false;  // Intel integrated (Arc exception handled by VRAM check)
    }
    return vram >= (1ULL * 1024 * 1024 * 1024);
}

oag_gpu_detect_t oag_gpu_detect(void) {
    oag_gpu_detect_t det;
    memset(&det, 0, sizeof(det));
    det.best_gpu_idx = -1;
    det.recommended_backend = OAG_BACKEND_CPU;

    // Create DXGI Factory
    IDXGIFactory4* factory = NULL;
    HRESULT hr = CreateDXGIFactory1(&IID_IDXGIFactory4, (void**)&factory);
    if (FAILED(hr)) {
        // Fallback: try DXGI 1.1
        hr = CreateDXGIFactory1(&IID_IDXGIFactory1, (void**)&factory);
        if (FAILED(hr)) {
            printf("[GPU Detect] DXGI not available, falling back to CPU\n");
            return det;
        }
    }

    // Enumerate adapters
    IDXGIAdapter1* adapter = NULL;
    uint64_t best_vram = 0;

    for (UINT i = 0;
         IDXGIFactory1_EnumAdapters1((IDXGIFactory1*)factory, i, &adapter) != DXGI_ERROR_NOT_FOUND
         && det.n_gpus < OAG_MAX_GPUS;
         i++) {

        DXGI_ADAPTER_DESC1 desc;
        IDXGIAdapter1_GetDesc1(adapter, &desc);

        // Skip software adapters (Microsoft Basic Render Driver, etc.)
        if (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) {
            IDXGIAdapter1_Release(adapter);
            continue;
        }

        // Skip virtual display adapters
        if (desc.VendorId == 0 || desc.DedicatedVideoMemory == 0) {
            // Check if it's a known virtual adapter
            char name_utf8[128];
            WideCharToMultiByte(CP_UTF8, 0, desc.Description, -1,
                               name_utf8, sizeof(name_utf8), NULL, NULL);
            if (strstr(name_utf8, "Parsec") || strstr(name_utf8, "Virtual") ||
                strstr(name_utf8, "Microsoft Basic")) {
                IDXGIAdapter1_Release(adapter);
                continue;
            }
        }

        oag_gpu_device_t* gpu = &det.gpus[det.n_gpus];

        // Convert wide string name to UTF-8
        WideCharToMultiByte(CP_UTF8, 0, desc.Description, -1,
                           gpu->name, sizeof(gpu->name), NULL, NULL);

        gpu->vendor_id   = desc.VendorId;
        gpu->device_id   = desc.DeviceId;
        gpu->vendor      = classify_vendor(desc.VendorId);
        gpu->vram_bytes  = desc.DedicatedVideoMemory;
        gpu->adapter_idx = i;
        gpu->is_discrete = is_discrete_gpu(desc.VendorId, desc.DedicatedVideoMemory);

        // Track best GPU (highest VRAM discrete GPU)
        if (gpu->is_discrete && gpu->vram_bytes > best_vram) {
            best_vram = gpu->vram_bytes;
            det.best_gpu_idx = det.n_gpus;
            det.best_vendor = gpu->vendor;
        }

        det.n_gpus++;
        IDXGIAdapter1_Release(adapter);
    }

    IDXGIFactory1_Release((IDXGIFactory1*)factory);

    // If no discrete GPU found, pick best integrated
    if (det.best_gpu_idx < 0 && det.n_gpus > 0) {
        best_vram = 0;
        for (int i = 0; i < det.n_gpus; i++) {
            if (det.gpus[i].vram_bytes > best_vram) {
                best_vram = det.gpus[i].vram_bytes;
                det.best_gpu_idx = i;
                det.best_vendor = det.gpus[i].vendor;
            }
        }
    }

    // Determine recommended backend
    if (det.best_gpu_idx >= 0) {
        switch (det.best_vendor) {
            case OAG_GPU_VENDOR_NVIDIA:
                det.recommended_backend = OAG_BACKEND_CUDA;
                break;
            case OAG_GPU_VENDOR_AMD:
                det.recommended_backend = OAG_BACKEND_DIRECTML;
                break;
            case OAG_GPU_VENDOR_INTEL:
                // Intel Arc → DirectML, Intel integrated → NPU or CPU
                if (det.gpus[det.best_gpu_idx].is_discrete) {
                    det.recommended_backend = OAG_BACKEND_DIRECTML;
                } else {
                    det.recommended_backend = OAG_BACKEND_CPU;
                }
                break;
            default:
                det.recommended_backend = OAG_BACKEND_CPU;
                break;
        }
    }

    return det;
}

void oag_gpu_detect_print(const oag_gpu_detect_t* det) {
    /* CP932 等の既定コードページでも壊れないよう ASCII のみ */
    printf("+------------------------------------------------------+\n");
    printf("|            GPU Auto-Detection (DXGI)                 |\n");
    printf("+------------------------------------------------------+\n");

    if (det->n_gpus == 0) {
        printf("|  No GPUs detected                                    |\n");
    }

    for (int i = 0; i < det->n_gpus; i++) {
        const oag_gpu_device_t* g = &det->gpus[i];

        const char* vendor_str;
        switch (g->vendor) {
            case OAG_GPU_VENDOR_NVIDIA: vendor_str = "NVIDIA"; break;
            case OAG_GPU_VENDOR_AMD:    vendor_str = "AMD   "; break;
            case OAG_GPU_VENDOR_INTEL:  vendor_str = "Intel "; break;
            default:                    vendor_str = "???   "; break;
        }

        const char* type_str = g->is_discrete ? "Discrete" : "Integrated";
        const char* backend_str;
        switch (g->vendor) {
            case OAG_GPU_VENDOR_NVIDIA: backend_str = "CUDA"; break;
            case OAG_GPU_VENDOR_AMD:    backend_str = "DirectML"; break;
            case OAG_GPU_VENDOR_INTEL:
                backend_str = g->is_discrete ? "DirectML" : "CPU/NPU";
                break;
            default:                    backend_str = "CPU"; break;
        }

        printf("|  [%d] %s %-10s %5.1f GB  %s -> %s",
               i, vendor_str, type_str,
               g->vram_bytes / (1024.0 * 1024.0 * 1024.0),
               g->name, backend_str);

        if (i == det->best_gpu_idx) {
            printf(" *");
        }
        printf("\n");
    }

    printf("+------------------------------------------------------+\n");

    if (det->best_gpu_idx >= 0) {
        printf("|  Selected: %s -> %s backend",
               det->gpus[det->best_gpu_idx].name,
               oag_backend_type_name(det->recommended_backend));
    } else {
        printf("|  Selected: CPU (no suitable GPU found)");
    }
    printf("\n");
    printf("+------------------------------------------------------+\n");
}

#else
// Non-Windows: stub
oag_gpu_detect_t oag_gpu_detect(void) {
    oag_gpu_detect_t det;
    memset(&det, 0, sizeof(det));
    det.best_gpu_idx = -1;
    det.recommended_backend = OAG_BACKEND_CPU;
    printf("[GPU Detect] DXGI not available on this platform\n");
    return det;
}

void oag_gpu_detect_print(const oag_gpu_detect_t* det) {
    printf("[GPU Detect] %d GPUs, recommended: %s\n",
           det->n_gpus, oag_backend_type_name(det->recommended_backend));
}
#endif
