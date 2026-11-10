// AACS MMC access on macOS via IOKit/SCSITaskDeviceInterface.
//
// Three entry points:
//   bf_aacs_vid_libredrive - vendor READ BUFFER 0x3c from a LibreDrive drive,
//                            which returns the decrypted VID with no
//                            authentication at all. This is what MakeMKV does.
//   bf_aacs_volume_id      - unauthenticated READ DISC STRUCTURE 0x80 (works
//                            only on drives that serve the VID to an
//                            unauthenticated AGID)
//   bf_aacs_auth_vid       - full AACS drive authentication (AGID + host
//                            certificate), then read the VID.
//
// We drive the SCSI layer ourselves rather than libaacs's MMC because libaacs's
// darwin backend cannot resolve the device (statfs on /dev/rdiskN) and hides the
// SCSI sense. Crypto (ECDSA sign/verify, ECDH bus key, AES-CMAC) comes from
// libaacs. This mirrors src/libaacs/mmc.c.

#include <string.h>
#include <stdio.h>
#include <stdlib.h>
#include <IOKit/IOKitLib.h>
#include <IOKit/IOBSD.h>
#include <IOKit/scsi/SCSITaskLib.h>

#ifndef kIOMainPortDefault
#define kIOMainPortDefault kIOMasterPortDefault
#endif

// libaacs crypto (linked statically)
extern void crypto_create_nonce(uint8_t *buf, size_t len);
extern void crypto_create_host_key_pair(uint8_t *key, uint8_t *key_point);
extern void crypto_create_bus_key(const uint8_t *priv_key, const uint8_t *drive_key_point, unsigned char *bus_key);
extern int crypto_aacs_sign(const uint8_t *cert, const uint8_t *priv_key, uint8_t *signature,
                            const uint8_t *nonce, const uint8_t *point);
extern int crypto_aacs_verify(const uint8_t *cert, const uint8_t *signature,
                              const uint8_t *data, unsigned int len);
extern int crypto_aacs_verify_drive_cert(const uint8_t *cert);
extern int crypto_aes_cmac_16(const unsigned char *data, const unsigned char *aes_key, unsigned char *cmac);

#define SEND_KEY 0xa3
#define REPORT_KEY 0xa4
#define READ_DISC_STRUCTURE 0xad
#define READ_BUFFER 0x3c
#define KEY_CLASS_AACS 0x02
#define RK_AGID 0x00
#define RK_DRIVE_CERT_CHALLENGE 0x01
#define RK_DRIVE_KEY 0x02
#define RK_INVALIDATE_AGID 0x3f
#define SK_HOST_CERT_CHALLENGE 0x01
#define SK_HOST_KEY 0x02

#define CDB_LEN 16

typedef struct {
    io_service_t svc;
    IOCFPlugInInterface **plug;
    MMCDeviceInterface **mmc;
    SCSITaskDeviceInterface **dev;
    int exclusive;
} BfDev;

static io_service_t find_bd_service(const char *bsd_name)
{
    CFMutableDictionaryRef match = IOServiceMatching("IOBDServices");
    io_iterator_t iter = 0;
    io_service_t svc, found = 0;
    if (!match || IOServiceGetMatchingServices(kIOMainPortDefault, match, &iter) != KERN_SUCCESS) {
        return 0;
    }
    while ((svc = IOIteratorNext(iter))) {
        CFTypeRef t = IORegistryEntrySearchCFProperty(svc, kIOServicePlane, CFSTR(kIOBSDNameKey),
                                                      kCFAllocatorDefault, kIORegistryIterateRecursively);
        char name[64] = "";
        if (t) {
            CFStringGetCString(t, name, sizeof(name), kCFStringEncodingASCII);
            CFRelease(t);
        }
        if (strcmp(name, bsd_name) == 0) {
            found = svc;
            break;
        }
        IOObjectRelease(svc);
    }
    IOObjectRelease(iter);
    return found;
}

static int open_dev(const char *bsd_name, BfDev *d)
{
    d->svc = find_bd_service(bsd_name);
    if (!d->svc) {
        return -10;
    }
    SInt32 score = 0;
    kern_return_t kr = IOCreatePlugInInterfaceForService(d->svc, kIOMMCDeviceUserClientTypeID,
                                                         kIOCFPlugInInterfaceID, &d->plug, &score);
    IOObjectRelease(d->svc);
    d->svc = 0;
    if (kr != KERN_SUCCESS || !d->plug) {
        return -11;
    }
    HRESULT hr = (*d->plug)->QueryInterface(d->plug, CFUUIDGetUUIDBytes(kIOMMCDeviceInterfaceID),
                                            (LPVOID *)&d->mmc);
    if (hr != S_OK || !d->mmc) {
        return -12;
    }
    d->dev = (*d->mmc)->GetSCSITaskDeviceInterface(d->mmc);
    if (!d->dev) {
        return -13;
    }
    d->exclusive = ((*d->dev)->ObtainExclusiveAccess(d->dev) == kIOReturnSuccess);
    fprintf(stderr, "aacs-mmc: open %s ok (exclusive=%d dev=%p)\n", bsd_name, d->exclusive, (void*)d->dev);
    return 0;
}

static void close_dev(BfDev *d)
{
    if (d->exclusive && d->dev) {
        (*d->dev)->ReleaseExclusiveAccess(d->dev);
    }
    if (d->mmc) {
        (*d->mmc)->Release(d->mmc);
    }
    if (d->plug) {
        (*d->plug)->Release(d->plug);
    }
    memset(d, 0, sizeof(*d));
}

// Send one MMC command. `tx` bytes out (or `rx` in). Returns 0 on success; on a
// CHECK CONDITION prints the sense and returns -1.
static int send_mmc(BfDev *d, const unsigned char cdb[CDB_LEN], unsigned char *buf,
                    unsigned int tx, unsigned int rx)
{
    int rc = -1;
    SCSITaskInterface **task = (*d->dev)->CreateSCSITask(d->dev);
    if (!task) {
        fprintf(stderr, "aacs-mmc: CreateSCSITask failed\n");
        return -2;
    }
    SCSITaskSGElement sg;
    sg.address = (IOVirtualAddress)(uintptr_t)buf;
    sg.length = tx ? tx : rx;
    UInt8 dir = buf ? (tx ? kSCSIDataTransfer_FromInitiatorToTarget
                          : kSCSIDataTransfer_FromTargetToInitiator)
                    : kSCSIDataTransfer_NoDataTransfer;
    IOReturn e1 = (*task)->SetCommandDescriptorBlock(task, cdb, CDB_LEN);
    IOReturn e2 = (*task)->SetScatterGatherEntries(task, &sg, 1, sg.length, dir);
    IOReturn e3 = (*task)->SetTimeoutDuration(task, 10000);
    if (e1 || e2 || e3) {
        fprintf(stderr, "aacs-mmc: setup failed CDB %02x (e1=%d e2=%d e3=%d)\n", cdb[0], e1, e2, e3);
    } else {
        SCSITaskStatus st = 0;
        SCSI_Sense_Data sense;
        UInt64 transferred = 0;
        memset(&sense, 0, sizeof(sense));
        if ((*task)->ExecuteTaskSync(task, &sense, &st, &transferred) == kIOReturnSuccess &&
            st == kSCSITaskStatus_GOOD) {
            rc = 0;
        } else {
            fprintf(stderr, "aacs-mmc: CDB %02x%02x%02x%02x failed (status %d sense %02x/%02x/%02x)\n",
                    cdb[0], cdb[1], cdb[2], cdb[3], st, sense.SENSE_KEY & 0x0f,
                    sense.ADDITIONAL_SENSE_CODE, sense.ADDITIONAL_SENSE_CODE_QUALIFIER);
            rc = -1;
        }
    }
    (*task)->Release(task);
    return rc;
}

static int report_key(BfDev *d, unsigned char agid, unsigned int addr, unsigned char blocks,
                      unsigned char format, unsigned char *buf, unsigned int len)
{
    unsigned char cdb[CDB_LEN];
    memset(cdb, 0, CDB_LEN);
    cdb[0] = REPORT_KEY;
    cdb[2] = (addr >> 24) & 0xff;
    cdb[3] = (addr >> 16) & 0xff;
    cdb[4] = (addr >> 8) & 0xff;
    cdb[5] = addr & 0xff;
    cdb[6] = blocks;
    cdb[7] = KEY_CLASS_AACS;
    cdb[8] = (len >> 8) & 0xff;
    cdb[9] = len & 0xff;
    cdb[10] = (agid << 6) | (format & 0x3f);
    return send_mmc(d, cdb, buf, 0, len);
}

static int send_key(BfDev *d, unsigned char agid, unsigned char format, unsigned char *buf, unsigned int len)
{
    unsigned char cdb[CDB_LEN];
    memset(cdb, 0, CDB_LEN);
    cdb[0] = SEND_KEY;
    cdb[7] = KEY_CLASS_AACS;
    cdb[8] = (len >> 8) & 0xff;
    cdb[9] = len & 0xff;
    cdb[10] = (agid << 6) | (format & 0x3f);
    return send_mmc(d, cdb, buf, len, 0);
}

static int read_disc_structure(BfDev *d, unsigned char agid, unsigned char format, unsigned char *buf, unsigned int len)
{
    unsigned char cdb[CDB_LEN];
    memset(cdb, 0, CDB_LEN);
    cdb[0] = READ_DISC_STRUCTURE;
    cdb[1] = 0x01; // media type: Blu-ray
    cdb[6] = 0x00; // layer
    cdb[7] = format;
    cdb[8] = (len >> 8) & 0xff;
    cdb[9] = len & 0xff;
    cdb[10] = (agid << 6);
    return send_mmc(d, cdb, buf, 0, len);
}

static int read_buffer(BfDev *d, unsigned int buffer_id, unsigned int offset,
                        unsigned char *buf, unsigned int len)
{
    unsigned char cdb[CDB_LEN];
    memset(cdb, 0, CDB_LEN);
    cdb[0] = READ_BUFFER;
    cdb[1] = 0x01; // mode: read buffer data
    cdb[2] = (buffer_id >> 8) & 0xff;
    cdb[3] = buffer_id & 0xff;
    cdb[4] = (offset >> 16) & 0xff;
    cdb[5] = (offset >> 8) & 0xff;
    cdb[6] = offset & 0xff;
    cdb[7] = (len >> 16) & 0xff;
    cdb[8] = (len >> 8) & 0xff;
    cdb[9] = len & 0xff;
    return send_mmc(d, cdb, buf, 0, len);
}

static int alloc_agid(BfDev *d, unsigned char *agid)
{
    int i;
    unsigned char buf[8];
    for (i = 0; i < 4; i++) {
        memset(buf, 0, sizeof(buf));
        report_key(d, (unsigned char)i, 0, 0, RK_INVALIDATE_AGID, buf, sizeof(buf));
    }
    memset(buf, 0, sizeof(buf));
    if (report_key(d, 0, 0, 0, RK_AGID, buf, sizeof(buf))) {
        return -1;
    }
    *agid = (buf[7] >> 6) & 0x03;
    return 0;
}

// LibreDrive vendor read of the AACS volume id.
//
// A LibreDrive-patched MediaTek drive (this one is a BU40N 1.03) decrypts the
// AACS VID in its own firmware and serves it over vendor READ BUFFER buffer id
// 0x4410 as a table of 36-byte records:
//
//     00 22 | 00 50 | <16-byte VID> | 16 zero bytes
//     ^len   ^type
//
// MakeMKV reads exactly this (one 36-byte record per drive open) and never
// sends SEND KEY / REPORT KEY, which is why it works with a revoked host
// certificate and why LibreDrive mode shows up in its log. The drive ignores
// the buffer offset: any offset returns the first record.
int bf_aacs_vid_libredrive(const char *bsd_name, unsigned char out[16])
{
    BfDev d;
    unsigned char buf[36];
    unsigned char *big = NULL;
    int rc, i;

    if (open_dev(bsd_name, &d)) {
        return -10;
    }
    memset(buf, 0, sizeof(buf));
    rc = read_buffer(&d, 0x4410, 0, buf, sizeof(buf));
    // Length 0x22 and record type 0x50 identify the AACS VID entry.
    if (rc == 0 && buf[0] == 0x00 && buf[1] == 0x22 && buf[2] == 0x00 && buf[3] == 0x50) {
        memcpy(out, buf + 4, 16);
        close_dev(&d);
        return 0;
    }
    // The record table could start with a different entry; scan the whole
    // buffer for the VID record before giving up.
    big = malloc(9216);
    if (big) {
        memset(big, 0, 9216);
        if (read_buffer(&d, 0x4410, 0, big, 9216) == 0) {
            for (i = 0; i + 36 <= 9216; i += 36) {
                if (big[i] == 0x00 && big[i + 1] == 0x22 && big[i + 2] == 0x00 && big[i + 3] == 0x50) {
                    memcpy(out, big + i + 4, 16);
                    free(big);
                    close_dev(&d);
                    return 0;
                }
            }
        }
        free(big);
    }
    fprintf(stderr, "aacs-mmc: no LibreDrive VID record in buffer 0x4410\n");
    close_dev(&d);
    return -13;
}

// Unauthenticated read of the volume id (may fail with "authentication required").
int bf_aacs_volume_id(const char *bsd_name, unsigned char out[16])
{
    BfDev d;
    unsigned char agid = 0, buf[36];
    int rc;
    if (open_dev(bsd_name, &d)) {
        return -10;
    }
    if (alloc_agid(&d, &agid)) {
        close_dev(&d);
        return -11;
    }
    memset(buf, 0, sizeof(buf));
    rc = read_disc_structure(&d, agid, 0x80, buf, sizeof(buf));
    close_dev(&d);
    if (rc) {
        return -12;
    }
    memcpy(out, buf + 4, 16);
    return 0;
}

// Full AACS authentication, then read the VID. host_priv is 20 bytes, host_cert
// 92 bytes (from the KEYDB `| HC |` entry).
int bf_aacs_auth_vid(const char *bsd_name, const unsigned char *host_priv,
                     const unsigned char *host_cert, unsigned char out[16])
{
    BfDev d;
    unsigned char agid = 0;
    unsigned char host_nonce[20], drive_nonce[20];
    unsigned char drive_cert[92], drive_point[40], drive_sig[40];
    unsigned char host_pub[40], host_sig[40];
    unsigned char host_priv_key[16], bus_key[16], data[60], calc_mac[16];
    unsigned char buf[116];
    int rc = -1;

    if (open_dev(bsd_name, &d)) {
        return -10;
    }
    if (alloc_agid(&d, &agid)) {
        close_dev(&d);
        return -11;
    }
    fprintf(stderr, "aacs-mmc: host cert id %02x%02x%02x%02x%02x%02x priv %02x%02x..\n", host_cert[4],host_cert[5],host_cert[6],host_cert[7],host_cert[8],host_cert[9], host_priv[0],host_priv[1]);
    crypto_create_nonce(host_nonce, sizeof(host_nonce));
    crypto_create_host_key_pair(host_priv_key, host_pub);

    // 1. send host certificate + host nonce (challenge)
    memset(buf, 0, sizeof(buf));
    buf[1] = 0x32;
    memcpy(buf + 4, host_nonce, 20);
    memcpy(buf + 24, host_cert, 92);
    if (send_key(&d, agid, SK_HOST_CERT_CHALLENGE, buf, sizeof(buf))) {
        fprintf(stderr, "aacs-mmc: SEND KEY (host cert) rejected\n");
        goto out;
    }

    // 2. read drive certificate + nonce
    memset(buf, 0, sizeof(buf));
    if (report_key(&d, agid, 0, 0, RK_DRIVE_CERT_CHALLENGE, buf, sizeof(buf))) {
        fprintf(stderr, "aacs-mmc: no drive certificate\n");
        goto out;
    }
    memcpy(drive_nonce, buf + 4, 20);
    memcpy(drive_cert, buf + 24, 92);
    if (drive_cert[0] == 0x11) {
        fprintf(stderr, "aacs-mmc: drive uses AACS2 certificate\n");
    }
    if (crypto_aacs_verify_drive_cert(drive_cert)) {
        fprintf(stderr, "aacs-mmc: drive certificate signature invalid\n");
    }

    // 3. read drive key point + signature
    memset(buf, 0, sizeof(buf));
    if (report_key(&d, agid, 0, 0, RK_DRIVE_KEY, buf, sizeof(buf))) {
        fprintf(stderr, "aacs-mmc: no drive key\n");
        goto out;
    }
    memcpy(drive_point, buf + 4, 40);
    memcpy(drive_sig, buf + 44, 40);

    // 4. verify drive signature over host nonce + drive key point
    memcpy(data, host_nonce, 20);
    memcpy(data + 20, drive_point, 40);
    if (crypto_aacs_verify(drive_cert, drive_sig, data, sizeof(data))) {
        fprintf(stderr, "aacs-mmc: drive signature invalid\n");
    }

    // 5. sign drive nonce + host key point with the host key, send it
    if (crypto_aacs_sign(host_cert, host_priv, host_sig, drive_nonce, host_pub)) {
        fprintf(stderr, "aacs-mmc: host signing failed\n");
        goto out;
    }
    memset(buf, 0, sizeof(buf));
    buf[1] = 0x52;
    memcpy(buf + 4, host_pub, 40);
    memcpy(buf + 44, host_sig, 40);
    if (send_key(&d, agid, SK_HOST_KEY, buf, sizeof(buf))) {
        fprintf(stderr, "aacs-mmc: SEND KEY (host key) rejected\n");
        goto out;
    }

    // 6. derive the bus key and read the (encrypted) volume id
    crypto_create_bus_key(host_priv_key, drive_point, bus_key);
    memset(buf, 0, sizeof(buf));
    if (read_disc_structure(&d, agid, 0x80, buf, 36)) {
        fprintf(stderr, "aacs-mmc: could not read volume id\n");
        goto out;
    }
    // buf[4..20] = vid, buf[20..36] = CMAC over the vid keyed with the bus key
    crypto_aes_cmac_16(buf + 4, bus_key, calc_mac);
    if (memcmp(calc_mac, buf + 20, 16)) {
        fprintf(stderr, "aacs-mmc: volume id CMAC mismatch\n");
    }
    memcpy(out, buf + 4, 16);
    rc = 0;

out:
    {
        unsigned char rel[8];
        memset(rel, 0, sizeof(rel));
        report_key(&d, agid, 0, 0, RK_INVALIDATE_AGID, rel, sizeof(rel));
    }
    close_dev(&d);
    return rc;
}
