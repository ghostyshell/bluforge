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
            unsigned char cdb[16];
            memset(cdb, 0, sizeof(cdb));
            cdb[0] = 0xAD;   // READ DISC STRUCTURE
            cdb[1] = 0x01;   // media type: Blu-ray
            cdb[7] = 0x80;   // format: AACS volume identifier
            cdb[9] = 36;     // allocation length
            cdb[10] = 0x00;  // AGID 0

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
