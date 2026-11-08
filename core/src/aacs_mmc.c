// Read the AACS Volume ID from a Blu-ray drive on macOS.
//
// libaacs needs the 16-byte volume ID to turn a media key into the volume-unique
// key. Its own macOS MMC layer cannot find the drive on this stack
// ("Could not find matching IOBDServices mounted @ /dev/disk4"), so we issue the
// READ DISC STRUCTURE (format 0x80) command ourselves over the same
// IOBDServices / SCSITaskDevice interfaces and cache the result where libaacs
// looks for it.

#include <string.h>
#include <IOKit/IOKitLib.h>
#include <IOKit/IOBSD.h>
#include <IOKit/scsi/SCSITaskLib.h>

#ifndef kIOMainPortDefault
#define kIOMainPortDefault kIOMasterPortDefault
#endif

static io_service_t find_bd_service(const char *bsd_name) {
    CFMutableDictionaryRef match = IOServiceMatching("IOBDServices");
    io_iterator_t iter = 0;
    if (!match || IOServiceGetMatchingServices(kIOMainPortDefault, match, &iter) != KERN_SUCCESS) {
        return 0;
    }
    io_service_t svc = 0;
    io_service_t found = 0;
    while ((svc = IOIteratorNext(iter))) {
        CFTypeRef t = IORegistryEntrySearchCFProperty(svc, kIOServicePlane, CFSTR(kIOBSDNameKey),
                                                      kCFAllocatorDefault,
                                                      kIORegistryIterateRecursively);
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

int bf_aacs_volume_id(const char *bsd_name, unsigned char out[16]) {
    if (!bsd_name || !out) {
        return -1;
    }
    io_service_t svc = find_bd_service(bsd_name);
    if (!svc) {
        return -10;
    }

    IOCFPlugInInterface **plug = NULL;
    SInt32 score = 0;
    kern_return_t kr = IOCreatePlugInInterfaceForService(svc, kIOMMCDeviceUserClientTypeID,
                                                          kIOCFPlugInInterfaceID, &plug, &score);
    IOObjectRelease(svc);
    if (kr != KERN_SUCCESS || !plug) {
        return -11;
    }

    MMCDeviceInterface **mmc = NULL;
    HRESULT hr = (*plug)->QueryInterface(plug, CFUUIDGetUUIDBytes(kIOMMCDeviceInterfaceID),
                                          (LPVOID *)&mmc);
    if (hr != S_OK || !mmc) {
        (*plug)->Release(plug);
        return -12;
    }

    SCSITaskDeviceInterface **dev = (*mmc)->GetSCSITaskDeviceInterface(mmc);
    if (!dev) {
        (*mmc)->Release(mmc);
        (*plug)->Release(plug);
        return -13;
    }

    int result = -14;
    // Exclusive access is refused for this USB drive as a normal user; a
    // read-only READ DISC STRUCTURE does not actually need it, so try anyway.
    int exclusive = ((*dev)->ObtainExclusiveAccess(dev) == kIOReturnSuccess);
    {
        (void)exclusive;
        SCSITaskInterface **task = (*dev)->CreateSCSITask(dev);
        if (task) {
            unsigned char rbuf[8];

            /* Allocate an AGID (REPORT KEY format 0x00, AGID nibble 0x0f).
             * The AACS volume ID is only readable while an AGID is held. */
            unsigned char alloc[16];
            memset(alloc, 0, sizeof(alloc));
            alloc[0] = 0xA4;
            alloc[7] = 0x00;   /* AACS key class */
            alloc[8] = 0x00;
            alloc[9] = 0x08;   /* allocation length */
            alloc[10] = 0xC0;  /* AGID 0x0f = allocate */
            memset(rbuf, 0, sizeof(rbuf));
            SCSITaskSGElement asg;
            asg.address = (IOVirtualAddress)(uintptr_t)rbuf;
            asg.length = sizeof(rbuf);
            IOReturn a1 = (*task)->SetCommandDescriptorBlock(task, alloc, 16);
            IOReturn a2 = (*task)->SetScatterGatherEntries(task, &asg, 1, sizeof(rbuf),
                                                           kSCSIDataTransfer_FromTargetToInitiator);
            IOReturn a3 = (*task)->SetTimeoutDuration(task, 10000);
            int have_agid = 0;
            unsigned char agid = 0;
            if (!a1 && !a2 && !a3) {
                SCSITaskStatus ast = 0;
                UInt64 atr = 0;
                SCSI_Sense_Data asense;
                memset(&asense, 0, sizeof(asense));
                if ((*task)->ExecuteTaskSync(task, &asense, &ast, &atr) == kIOReturnSuccess &&
                    ast == kSCSITaskStatus_GOOD && atr >= 8) {
                    agid = (unsigned char)((rbuf[7] >> 6) & 0x03);
                    have_agid = 1;
                }
            }
            (void)have_agid;

            unsigned char cdb[16];
            memset(cdb, 0, sizeof(cdb));
            cdb[0] = 0xAD;   // READ DISC STRUCTURE
            cdb[1] = 0x01;   // media type: Blu-ray
            cdb[7] = 0x80;   // format: AACS volume identifier
            cdb[9] = 36;     // allocation length
            cdb[10] = (unsigned char)((agid & 0x03) << 6);  // the allocated AGID

            unsigned char buf[36];
            memset(buf, 0, sizeof(buf));
            SCSITaskSGElement sg;
            sg.address = (IOVirtualAddress)(uintptr_t)buf;
            sg.length = sizeof(buf);

            IOReturn e1 = (*task)->SetCommandDescriptorBlock(task, cdb, 16);
            IOReturn e2 = (*task)->SetScatterGatherEntries(task, &sg, 1, sizeof(buf),
                                                           kSCSIDataTransfer_FromTargetToInitiator);
            IOReturn e3 = (*task)->SetTimeoutDuration(task, 10000);
            if (!e1 && !e2 && !e3) {
                SCSITaskStatus status = 0;
                UInt64 transferred = 0;
                SCSI_Sense_Data sense;
                memset(&sense, 0, sizeof(sense));
                kr = (*task)->ExecuteTaskSync(task, &sense, &status, &transferred);
                if (kr == kIOReturnSuccess && status == kSCSITaskStatus_GOOD && transferred >= 20) {
                    memcpy(out, buf + 4, 16);
                    result = 0;
                } else {
                    // An authenticated AGID is normally required: sense 05/6F/02.
                    result = -15;
                }
            } else {
                result = -16;
            }
            // Release the AGID so the drive can serve other hosts.
            if (have_agid) {
                unsigned char rel[16];
                memset(rel, 0, sizeof(rel));
                rel[0] = 0xA4;
                rel[9] = 0x00;
                rel[10] = (unsigned char)(((agid & 0x03) << 6) | 0x3f);
                (*task)->SetCommandDescriptorBlock(task, rel, 16);
                SCSITaskStatus rst = 0;
                UInt64 rtr = 0;
                SCSI_Sense_Data rsense;
                memset(&rsense, 0, sizeof(rsense));
                (*task)->ExecuteTaskSync(task, &rsense, &rst, &rtr);
            }
            // No ReleaseSCSITask on this interface; the task dies with the device.
        } else {
            result = -17;
        }
        if (exclusive) {
            (*dev)->ReleaseExclusiveAccess(dev);
        }
    }
    (*mmc)->Release(mmc);
    (*plug)->Release(plug);
    return result;
}
