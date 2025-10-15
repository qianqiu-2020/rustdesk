#include <AVFoundation/AVFoundation.h>
#include <CoreFoundation/CoreFoundation.h>
#include <CoreMedia/CoreMedia.h>
#include <MacTypes.h>
#include <VideoToolbox/VideoToolbox.h>
#include <cstdlib>
#include <pthread.h>
#include <ratio>
#include <sys/_types/_int32_t.h>
#include <sys/event.h>
#include <unistd.h>
#include "../../log.h"


static int32_t hasHardwareEncoder(bool h265) {
    CFMutableDictionaryRef spec = CFDictionaryCreateMutable(kCFAllocatorDefault, 0,
                                                            &kCFTypeDictionaryKeyCallBacks,
                                                            &kCFTypeDictionaryValueCallBacks);
    #if TARGET_OS_MAC
        // Specify that we require a hardware-accelerated video encoder
        CFDictionarySetValue(spec, kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder, kCFBooleanTrue);
    #endif

    CMVideoCodecType codecType = h265 ? kCMVideoCodecType_HEVC : kCMVideoCodecType_H264;
    CFDictionaryRef properties = NULL;
    CFStringRef outID = NULL;
    OSStatus result = VTCopySupportedPropertyDictionaryForEncoder(1920, 1080, codecType, spec, &outID, &properties);

    CFRelease(spec); // Clean up the specification dictionary

    if (result == kVTCouldNotFindVideoEncoderErr) {
        return 0; // No hardware encoder found
    }

    if (properties != NULL) {
        CFRelease(properties);
    }
    if (outID != NULL) {
        CFRelease(outID);
    }

    return result == noErr ? 1 : 0;
}

extern "C" void checkVideoToolboxSupport(int32_t *h264Encoder, int32_t *h265Encoder, int32_t *h264Decoder, int32_t *h265Decoder) {
    // https://stackoverflow.com/questions/50956097/determine-if-ios-device-can-support-hevc-encoding
    *h264Encoder = hasHardwareEncoder(false);
    *h265Encoder = hasHardwareEncoder(true);
    // *h265Encoder = [[AVAssetExportSession allExportPresets] containsObject:@"AVAssetExportPresetHEVCHighestQuality"];

    *h264Decoder = VTIsHardwareDecodeSupported(kCMVideoCodecType_H264);
    *h265Decoder = VTIsHardwareDecodeSupported(kCMVideoCodecType_HEVC);

    return;
}

extern "C" uint64_t GetHwcodecGpuSignature() {
    int32_t h264Encoder = 0;
    int32_t h265Encoder = 0;
    int32_t h264Decoder = 0;
    int32_t h265Decoder = 0;
    checkVideoToolboxSupport(&h264Encoder, &h265Encoder, &h264Decoder, &h265Decoder);
    return (uint64_t)h264Encoder << 24 | (uint64_t)h265Encoder << 16 | (uint64_t)h264Decoder << 8 | (uint64_t)h265Decoder;
}   

static void *parent_death_monitor_thread(void *arg) {
  int kq = (intptr_t)arg;
  struct kevent events[1];

  int ret = kevent(kq, NULL, 0, events, 1, NULL);

  if (ret > 0) {
    // Parent process died, terminate this process
    LOG_INFO("Parent process died, terminating hwcodec check process");
    exit(1);
  }

  return NULL;
}

extern "C" int setup_parent_death_signal() {
  // On macOS, use kqueue to monitor parent process death
  pid_t parent_pid = getppid();
  int kq = kqueue();

  if (kq == -1) {
    LOG_DEBUG("Failed to create kqueue for parent monitoring");
    return -1;
  }

  struct kevent event;
  EV_SET(&event, parent_pid, EVFILT_PROC, EV_ADD | EV_ONESHOT, NOTE_EXIT, 0,
         NULL);

  int ret = kevent(kq, &event, 1, NULL, 0, NULL);

  if (ret == -1) {
    LOG_ERROR("Failed to register parent death monitoring on macOS\n");
    close(kq);
    return -1;
  } else {

    // Spawn a thread to monitor parent death
    pthread_t monitor_thread;
    ret = pthread_create(&monitor_thread, NULL, parent_death_monitor_thread,
                         (void *)(intptr_t)kq);

    if (ret != 0) {
      LOG_ERROR("Failed to create parent death monitor thread");
      close(kq);
      return -1;
    }

    // Detach the thread so it can run independently
    pthread_detach(monitor_thread);
    return 0;
  }
}

