// Open_Agents — Multi-NIC (有線LAN + WiFi 同時送受信)
// Bond multiple network interfaces for parallel data transfer
#ifndef OAG_MULTI_NIC_H
#define OAG_MULTI_NIC_H

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

#define OAG_MAX_NICS 8

typedef enum {
    OAG_NIC_ETHERNET = 0,
    OAG_NIC_WIFI     = 1,
    OAG_NIC_LOOPBACK = 2,
} oag_nic_type_t;

typedef struct {
    char          name[64];
    char          ip[46];       // IPv4 or IPv6
    oag_nic_type_t type;
    uint64_t      speed_mbps;   // link speed
    bool          active;
    int           socket_fd;    // bound socket (-1 if not bound)
} oag_nic_info_t;

typedef enum {
    OAG_BOND_ROUND_ROBIN  = 0,  // alternate requests between NICs
    OAG_BOND_FASTEST_FIRST = 1, // send on fastest available
    OAG_BOND_PARALLEL     = 2,  // split data across NICs (bonding)
} oag_bond_mode_t;

typedef struct {
    oag_nic_info_t nics[OAG_MAX_NICS];
    int            n_nics;
    oag_bond_mode_t mode;
    int            current_nic;  // round-robin index
} oag_multi_nic_t;

// Discover available network interfaces
oag_multi_nic_t* oag_multi_nic_create(void);
void             oag_multi_nic_free(oag_multi_nic_t* mn);

// Print discovered NICs
void oag_multi_nic_print(const oag_multi_nic_t* mn);

// Set bonding mode
void oag_multi_nic_set_mode(oag_multi_nic_t* mn, oag_bond_mode_t mode);

// Download file using bonded NICs (parallel chunks)
// Returns bytes downloaded, -1 on error
int64_t oag_multi_nic_download(oag_multi_nic_t* mn,
                                const char* url,
                                const char* output_path);

// HTTP GET with NIC selection
// Returns response body (caller frees), sets *out_len
char* oag_multi_nic_http_get(oag_multi_nic_t* mn,
                              const char* url,
                              size_t* out_len);

#endif // OAG_MULTI_NIC_H
