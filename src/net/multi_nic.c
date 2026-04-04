// Open_Agents — Multi-NIC (有線LAN + WiFi 同時送受信)
#include "net/multi_nic.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#include <winsock2.h>
#include <ws2tcpip.h>
#include <iphlpapi.h>
#pragma comment(lib, "iphlpapi.lib")
#pragma comment(lib, "ws2_32.lib")
#else
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <ifaddrs.h>
#include <net/if.h>
#include <unistd.h>
#endif

// ============================================================
// Discover NICs
// ============================================================

#ifdef _WIN32
static void discover_nics_win(oag_multi_nic_t* mn) {
    ULONG buf_size = 0;
    GetAdaptersAddresses(AF_INET, 0, NULL, NULL, &buf_size);

    IP_ADAPTER_ADDRESSES* addrs = (IP_ADAPTER_ADDRESSES*)malloc(buf_size);
    if (!addrs) return;
    if (GetAdaptersAddresses(AF_INET, 0, NULL, addrs, &buf_size) != NO_ERROR) {
        free(addrs);
        return;
    }

    for (IP_ADAPTER_ADDRESSES* a = addrs; a && mn->n_nics < OAG_MAX_NICS; a = a->Next) {
        // Skip loopback and tunnel adapters
        if (a->IfType == IF_TYPE_SOFTWARE_LOOPBACK) continue;
        if (a->OperStatus != IfOperStatusUp) continue;

        // Get first unicast address
        if (!a->FirstUnicastAddress) continue;

        oag_nic_info_t* nic = &mn->nics[mn->n_nics];

        // Name
        WideCharToMultiByte(CP_UTF8, 0, a->FriendlyName, -1,
                           nic->name, sizeof(nic->name), NULL, NULL);

        // IP
        struct sockaddr_in* sa = (struct sockaddr_in*)a->FirstUnicastAddress->Address.lpSockaddr;
        inet_ntop(AF_INET, &sa->sin_addr, nic->ip, sizeof(nic->ip));

        // Type
        if (a->IfType == IF_TYPE_IEEE80211) {
            nic->type = OAG_NIC_WIFI;
        } else if (a->IfType == IF_TYPE_ETHERNET_CSMACD) {
            nic->type = OAG_NIC_ETHERNET;
        } else {
            continue;  // skip unknown types
        }

        // Speed (bps → Mbps)
        nic->speed_mbps = a->TransmitLinkSpeed / 1000000ULL;
        nic->active = true;
        nic->socket_fd = -1;

        mn->n_nics++;
    }

    free(addrs);
}
#else
static void discover_nics_unix(oag_multi_nic_t* mn) {
    struct ifaddrs* ifas;
    if (getifaddrs(&ifas) != 0) return;

    for (struct ifaddrs* ifa = ifas; ifa && mn->n_nics < OAG_MAX_NICS; ifa = ifa->ifa_next) {
        if (!ifa->ifa_addr || ifa->ifa_addr->sa_family != AF_INET) continue;
        if (ifa->ifa_flags & IFF_LOOPBACK) continue;
        if (!(ifa->ifa_flags & IFF_UP)) continue;

        oag_nic_info_t* nic = &mn->nics[mn->n_nics];
        strncpy(nic->name, ifa->ifa_name, sizeof(nic->name) - 1);

        struct sockaddr_in* sa = (struct sockaddr_in*)ifa->ifa_addr;
        inet_ntop(AF_INET, &sa->sin_addr, nic->ip, sizeof(nic->ip));

        // Guess type from name
        if (strstr(ifa->ifa_name, "wlan") || strstr(ifa->ifa_name, "wlp")) {
            nic->type = OAG_NIC_WIFI;
        } else {
            nic->type = OAG_NIC_ETHERNET;
        }

        nic->speed_mbps = 1000;  // default guess
        nic->active = true;
        nic->socket_fd = -1;

        mn->n_nics++;
    }

    freeifaddrs(ifas);
}
#endif

oag_multi_nic_t* oag_multi_nic_create(void) {
    oag_multi_nic_t* mn = (oag_multi_nic_t*)calloc(1, sizeof(oag_multi_nic_t));
    mn->mode = OAG_BOND_ROUND_ROBIN;

    #ifdef _WIN32
    WSADATA wsa;
    WSAStartup(MAKEWORD(2, 2), &wsa);
    discover_nics_win(mn);
    #else
    discover_nics_unix(mn);
    #endif

    return mn;
}

void oag_multi_nic_free(oag_multi_nic_t* mn) {
    if (!mn) return;
    for (int i = 0; i < mn->n_nics; i++) {
        if (mn->nics[i].socket_fd >= 0) {
            #ifdef _WIN32
            closesocket(mn->nics[i].socket_fd);
            #else
            close(mn->nics[i].socket_fd);
            #endif
        }
    }
    free(mn);
}

void oag_multi_nic_print(const oag_multi_nic_t* mn) {
    printf("[Multi-NIC] Found %d interfaces:\n", mn->n_nics);
    for (int i = 0; i < mn->n_nics; i++) {
        const oag_nic_info_t* n = &mn->nics[i];
        printf("  [%d] %-20s %s %-15s %llu Mbps %s\n",
               i, n->name,
               n->type == OAG_NIC_ETHERNET ? "ETH " : "WiFi",
               n->ip,
               (unsigned long long)n->speed_mbps,
               n->active ? "UP" : "DOWN");
    }
    printf("[Multi-NIC] Mode: %s\n",
           mn->mode == OAG_BOND_ROUND_ROBIN ? "Round-Robin" :
           mn->mode == OAG_BOND_FASTEST_FIRST ? "Fastest-First" : "Parallel");
}

void oag_multi_nic_set_mode(oag_multi_nic_t* mn, oag_bond_mode_t mode) {
    mn->mode = mode;
}

// ============================================================
// Bonded download (parallel chunk transfer)
// ============================================================

// ============================================================
// URL parser (minimal)
// ============================================================

typedef struct {
    char host[256];
    char path[512];
    int  port;
    bool use_ssl;
} parsed_url_t;

static bool parse_url(const char* url, parsed_url_t* out) {
    memset(out, 0, sizeof(*out));
    out->port = 80;

    if (strncmp(url, "https://", 8) == 0) {
        out->use_ssl = true;
        out->port = 443;
        url += 8;
    } else if (strncmp(url, "http://", 7) == 0) {
        url += 7;
    }

    const char* slash = strchr(url, '/');
    const char* colon = strchr(url, ':');

    if (colon && (!slash || colon < slash)) {
        size_t hlen = colon - url;
        memcpy(out->host, url, hlen);
        out->port = atoi(colon + 1);
        if (slash) strncpy(out->path, slash, sizeof(out->path) - 1);
        else strcpy(out->path, "/");
    } else if (slash) {
        size_t hlen = slash - url;
        memcpy(out->host, url, hlen);
        strncpy(out->path, slash, sizeof(out->path) - 1);
    } else {
        strncpy(out->host, url, sizeof(out->host) - 1);
        strcpy(out->path, "/");
    }

    return out->host[0] != '\0';
}

// ============================================================
// Per-NIC chunk downloader (threaded)
// ============================================================

#ifdef _WIN32

typedef struct {
    oag_nic_info_t* nic;
    parsed_url_t    url;
    int64_t         range_start;
    int64_t         range_end;
    uint8_t*        buffer;        // output buffer for this chunk
    int64_t         bytes_read;
    bool            success;
    int             thread_id;
} chunk_task_t;

static DWORD WINAPI download_chunk_thread(LPVOID param) {
    chunk_task_t* task = (chunk_task_t*)param;
    task->success = false;
    task->bytes_read = 0;

    // Create socket bound to specific NIC
    SOCKET sock = socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
    if (sock == INVALID_SOCKET) return 0;

    // Bind to NIC IP
    struct sockaddr_in bind_addr;
    memset(&bind_addr, 0, sizeof(bind_addr));
    bind_addr.sin_family = AF_INET;
    bind_addr.sin_port = 0;  // any port
    inet_pton(AF_INET, task->nic->ip, &bind_addr.sin_addr);

    if (bind(sock, (struct sockaddr*)&bind_addr, sizeof(bind_addr)) != 0) {
        printf("[NIC-%d] Bind to %s failed\n", task->thread_id, task->nic->ip);
        closesocket(sock);
        return 0;
    }

    // Resolve host
    struct sockaddr_in server;
    memset(&server, 0, sizeof(server));
    server.sin_family = AF_INET;
    server.sin_port = htons((u_short)task->url.port);

    struct addrinfo hints = {0}, *res;
    hints.ai_family = AF_INET;
    hints.ai_socktype = SOCK_STREAM;
    if (getaddrinfo(task->url.host, NULL, &hints, &res) != 0) {
        closesocket(sock);
        return 0;
    }
    server.sin_addr = ((struct sockaddr_in*)res->ai_addr)->sin_addr;
    freeaddrinfo(res);

    // Connect
    if (connect(sock, (struct sockaddr*)&server, sizeof(server)) != 0) {
        printf("[NIC-%d] Connect failed via %s\n", task->thread_id, task->nic->ip);
        closesocket(sock);
        return 0;
    }

    // Send HTTP GET with Range header
    char request[1024];
    int req_len = snprintf(request, sizeof(request),
        "GET %s HTTP/1.1\r\n"
        "Host: %s\r\n"
        "Range: bytes=%lld-%lld\r\n"
        "Connection: close\r\n"
        "\r\n",
        task->url.path, task->url.host,
        (long long)task->range_start, (long long)task->range_end);

    send(sock, request, req_len, 0);

    // Read response — skip headers
    char header_buf[4096];
    int total_hdr = 0;
    bool headers_done = false;
    int body_start = 0;

    while (!headers_done && total_hdr < (int)sizeof(header_buf) - 1) {
        int n = recv(sock, header_buf + total_hdr, sizeof(header_buf) - 1 - total_hdr, 0);
        if (n <= 0) break;
        total_hdr += n;
        header_buf[total_hdr] = '\0';

        char* end = strstr(header_buf, "\r\n\r\n");
        if (end) {
            headers_done = true;
            body_start = (int)(end + 4 - header_buf);

            // Copy body part that was in header buffer
            int body_bytes = total_hdr - body_start;
            if (body_bytes > 0 && task->buffer) {
                memcpy(task->buffer, header_buf + body_start, body_bytes);
                task->bytes_read = body_bytes;
            }
        }
    }

    // Read remaining body
    int64_t expected = task->range_end - task->range_start + 1;
    while (task->bytes_read < expected) {
        int to_read = (int)(expected - task->bytes_read);
        if (to_read > 65536) to_read = 65536;
        int n = recv(sock, (char*)(task->buffer + task->bytes_read), to_read, 0);
        if (n <= 0) break;
        task->bytes_read += n;
    }

    closesocket(sock);

    task->success = (task->bytes_read > 0);
    printf("[NIC-%d] %s: %lld bytes via %s\n",
           task->thread_id,
           task->success ? "OK" : "FAIL",
           (long long)task->bytes_read,
           task->nic->name);

    return 0;
}
#endif

// ============================================================
// Bonded download (parallel chunk transfer)
// ============================================================

int64_t oag_multi_nic_download(oag_multi_nic_t* mn,
                                const char* url,
                                const char* output_path) {
    if (mn->n_nics == 0) {
        fprintf(stderr, "[Multi-NIC] No active interfaces\n");
        return -1;
    }

    parsed_url_t purl;
    if (!parse_url(url, &purl)) {
        fprintf(stderr, "[Multi-NIC] Invalid URL: %s\n", url);
        return -1;
    }

    if (purl.use_ssl) {
        printf("[Multi-NIC] HTTPS not yet supported for parallel download.\n");
        printf("[Multi-NIC] Use HTTP URL or download with single NIC.\n");
        return -1;
    }

    printf("[Multi-NIC] Parallel download: %s → %s\n", url, output_path);
    printf("[Multi-NIC] Using %d NICs: ", mn->n_nics);
    for (int i = 0; i < mn->n_nics; i++) {
        printf("%s(%s) ", mn->nics[i].name, mn->nics[i].ip);
    }
    printf("\n");

#ifdef _WIN32
    // Step 1: HEAD request to get content length (use first NIC)
    // Simplified: assume file size from URL or use GET with range
    // For HuggingFace GGUF files, we'd need proper HTTP handling
    // For now, use a fixed test or accept size as parameter
    int64_t file_size = 0;

    // Try HTTP HEAD to get Content-Length
    SOCKET head_sock = socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
    if (head_sock != INVALID_SOCKET) {
        struct addrinfo hints = {0}, *res;
        hints.ai_family = AF_INET;
        hints.ai_socktype = SOCK_STREAM;
        if (getaddrinfo(purl.host, NULL, &hints, &res) == 0) {
            struct sockaddr_in server;
            memset(&server, 0, sizeof(server));
            server.sin_family = AF_INET;
            server.sin_port = htons((u_short)purl.port);
            server.sin_addr = ((struct sockaddr_in*)res->ai_addr)->sin_addr;
            freeaddrinfo(res);

            if (connect(head_sock, (struct sockaddr*)&server, sizeof(server)) == 0) {
                char req[512];
                int len = snprintf(req, sizeof(req),
                    "HEAD %s HTTP/1.1\r\nHost: %s\r\nConnection: close\r\n\r\n",
                    purl.path, purl.host);
                send(head_sock, req, len, 0);

                char resp[4096];
                int n = recv(head_sock, resp, sizeof(resp) - 1, 0);
                if (n > 0) {
                    resp[n] = '\0';
                    char* cl = strstr(resp, "Content-Length:");
                    if (!cl) cl = strstr(resp, "content-length:");
                    if (cl) file_size = atoll(cl + 15);
                }
            }
        }
        closesocket(head_sock);
    }

    if (file_size <= 0) {
        printf("[Multi-NIC] Cannot determine file size. Single-NIC fallback needed.\n");
        return -1;
    }

    printf("[Multi-NIC] File size: %lld bytes (%.1f MB)\n",
           (long long)file_size, file_size / (1024.0 * 1024.0));

    // Step 2: Divide into chunks per NIC
    int n_chunks = mn->n_nics;
    int64_t chunk_size = file_size / n_chunks;

    chunk_task_t* tasks = (chunk_task_t*)calloc(n_chunks, sizeof(chunk_task_t));
    uint8_t** buffers = (uint8_t**)calloc(n_chunks, sizeof(uint8_t*));
    HANDLE* threads = (HANDLE*)calloc(n_chunks, sizeof(HANDLE));

    for (int i = 0; i < n_chunks; i++) {
        tasks[i].nic = &mn->nics[i % mn->n_nics];
        tasks[i].url = purl;
        tasks[i].range_start = i * chunk_size;
        tasks[i].range_end = (i == n_chunks - 1) ? file_size - 1 : (i + 1) * chunk_size - 1;
        tasks[i].thread_id = i;

        int64_t buf_size = tasks[i].range_end - tasks[i].range_start + 1;
        buffers[i] = (uint8_t*)malloc((size_t)buf_size);
        if (!buffers[i]) {
            fprintf(stderr, "[Multi-NIC] Failed to allocate %lld bytes for chunk %d\n",
                    (long long)buf_size, i);
            // Cleanup and abort
            for (int j = 0; j < i; j++) free(buffers[j]);
            free(tasks); free(buffers); free(threads);
            return -1;
        }
        tasks[i].buffer = buffers[i];
    }

    // Step 3: Launch parallel downloads
    printf("[Multi-NIC] Launching %d parallel chunk downloads...\n", n_chunks);
    LARGE_INTEGER freq, t0, t1;
    QueryPerformanceFrequency(&freq);
    QueryPerformanceCounter(&t0);

    for (int i = 0; i < n_chunks; i++) {
        threads[i] = CreateThread(NULL, 0, download_chunk_thread, &tasks[i], 0, NULL);
    }

    // Wait for all threads
    WaitForMultipleObjects(n_chunks, threads, TRUE, 60000);  // 60s timeout

    QueryPerformanceCounter(&t1);
    double elapsed_ms = (double)(t1.QuadPart - t0.QuadPart) / freq.QuadPart * 1000.0;

    // Step 4: Assemble file
    FILE* out = fopen(output_path, "wb");
    int64_t total_written = 0;

    if (out) {
        for (int i = 0; i < n_chunks; i++) {
            if (tasks[i].success && tasks[i].bytes_read > 0) {
                fwrite(buffers[i], 1, (size_t)tasks[i].bytes_read, out);
                total_written += tasks[i].bytes_read;
            }
        }
        fclose(out);
    }

    // Stats
    double speed_mbps = (total_written * 8.0) / (elapsed_ms / 1000.0) / 1e6;
    printf("[Multi-NIC] Downloaded %lld bytes in %.1f ms (%.1f Mbps)\n",
           (long long)total_written, elapsed_ms, speed_mbps);

    // Cleanup
    for (int i = 0; i < n_chunks; i++) {
        CloseHandle(threads[i]);
        free(buffers[i]);
    }
    free(tasks);
    free(buffers);
    free(threads);

    return total_written;
#else
    (void)output_path;
    (void)purl;
    printf("[Multi-NIC] Parallel download not available on this platform\n");
    return -1;
#endif
}

char* oag_multi_nic_http_get(oag_multi_nic_t* mn,
                              const char* url,
                              size_t* out_len) {
    (void)mn;
    (void)url;
    *out_len = 0;
    return NULL;
}
