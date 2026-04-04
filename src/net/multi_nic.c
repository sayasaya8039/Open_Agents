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

int64_t oag_multi_nic_download(oag_multi_nic_t* mn,
                                const char* url,
                                const char* output_path) {
    if (mn->n_nics == 0) {
        fprintf(stderr, "[Multi-NIC] No active interfaces\n");
        return -1;
    }

    printf("[Multi-NIC] Downloading %s via %d NICs...\n", url, mn->n_nics);

    // TODO: Implement parallel HTTP range requests per NIC
    // Each NIC downloads a chunk of the file simultaneously
    // Chunks are reassembled into output_path
    //
    // Algorithm:
    // 1. HEAD request to get Content-Length
    // 2. Divide into n_nics chunks
    // 3. Per NIC: bind socket to NIC IP, send GET with Range header
    // 4. Write chunks to file at correct offsets
    // 5. Verify integrity

    printf("[Multi-NIC] Parallel download not yet implemented (stub)\n");
    return -1;
}

char* oag_multi_nic_http_get(oag_multi_nic_t* mn,
                              const char* url,
                              size_t* out_len) {
    (void)mn;
    (void)url;
    *out_len = 0;
    // TODO: implement with NIC selection
    return NULL;
}
