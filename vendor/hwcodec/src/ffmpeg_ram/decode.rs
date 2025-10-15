#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
use super::Priority;
use crate::common::TEST_TIMEOUT_MS;
use crate::ffmpeg::{init_av_log, AVHWDeviceType::*};

use crate::{
    common::DataFormat::*,
    ffmpeg::{AVHWDeviceType, AVPixelFormat},
    ffmpeg_ram::{
        ffmpeg_ram_decode, ffmpeg_ram_free_decoder, ffmpeg_ram_new_decoder, CodecInfo,
        AV_NUM_DATA_POINTERS,
    },
};
use log::error;
use std::{
    ffi::{c_void, CString},
    os::raw::c_int,
    slice::from_raw_parts,
    time::Instant,
    vec,
};

#[derive(Debug, Clone)]
pub struct DecodeContext {
    pub name: String,
    pub device_type: AVHWDeviceType,
    pub thread_count: i32,
}

pub struct DecodeFrame {
    pub pixfmt: AVPixelFormat,
    pub width: i32,
    pub height: i32,
    pub data: Vec<Vec<u8>>,
    pub linesize: Vec<i32>,
    pub key: bool,
}

impl std::fmt::Display for DecodeFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = String::from("data:");
        for data in self.data.iter() {
            s.push_str(format!("{} ", data.len()).as_str());
        }
        s.push_str(", linesize:");
        for linesize in self.linesize.iter() {
            s.push_str(format!("{} ", linesize).as_str());
        }

        write!(
            f,
            "fixfmt:{}, width:{}, height:{},key:{}, {}",
            self.pixfmt as i32, self.width, self.height, self.key, s,
        )
    }
}

pub struct Decoder {
    codec: *mut c_void,
    frames: *mut Vec<DecodeFrame>,
    pub ctx: DecodeContext,
    instance_id: u64,  // Decoder instance ID, used to distinguish the state of different decoders
    packet_cache: Option<Vec<u8>>,
    decoder_initialized: bool,
    last_sps: Option<Vec<u8>>,
    last_pps: Option<Vec<u8>>,
    last_vps: Option<Vec<u8>>,  // H265 specific
}

static mut DECODER_INSTANCE_COUNTER: u64 = 0;

unsafe impl Send for Decoder {}
unsafe impl Sync for Decoder {}

impl Decoder {
    pub fn new(ctx: DecodeContext) -> Result<Self, ()> {
        init_av_log();
        unsafe {
            let codec = ffmpeg_ram_new_decoder(
                CString::new(ctx.name.as_str()).map_err(|_| ())?.as_ptr(),
                ctx.device_type as _,
                ctx.thread_count,
                Some(Decoder::callback),
            );

            if codec.is_null() {
                return Err(());
            }

            DECODER_INSTANCE_COUNTER += 1;
            let instance_id = DECODER_INSTANCE_COUNTER;

            Ok(Decoder {
                codec,
                frames: Box::into_raw(Box::new(Vec::<DecodeFrame>::new())),
                ctx,
                instance_id,
                // Initialize instance state
                packet_cache: None,
                decoder_initialized: false,
                last_sps: None,
                last_pps: None,
                last_vps: None,
            })
        }
    }

    pub fn decode(&mut self, packet: &[u8]) -> Result<&mut Vec<DecodeFrame>, i32> {
        use std::fs::OpenOptions;
        use std::io::Write;

        // Detect encoding format: determine whether it's H264 or H265 based on decoder name
        let is_h265 = self.ctx.name.contains("hevc") || self.ctx.name.contains("h265");

        // NALU analysis: support H264 and H265, determine if it's I-frame with SPS/PPS preceding it, and detect new SPS/PPS
        let mut has_sps = false;
        let mut has_pps = false;
        let mut has_vps = false;  // H265 specific
        let mut is_iframe = false;
        let mut found_new_sps = false;
        let mut found_new_pps = false;
        let mut pos = 0;
        
        while pos + 4 < packet.len() {
            let start_code = if &packet[pos..pos+3] == [0,0,1] { 3 } else if &packet[pos..pos+4] == [0,0,0,1] { 4 } else { 0 };
            if start_code > 0 {
                let nalu_header = packet[pos+start_code];
                let nalu_start = pos + start_code;
                // Find next NALU header
                let mut nalu_end = nalu_start + 1;
                while nalu_end + 4 < packet.len() {
                    if &packet[nalu_end..nalu_end+3] == [0,0,1] || &packet[nalu_end..nalu_end+4] == [0,0,0,1] {
                        break;
                    }
                    nalu_end += 1;
                }
                
                if is_h265 {
                    // H265 NALU type parsing (6 bits)
                    let nalu_type = (nalu_header & 0x7E) >> 1;
                    match nalu_type {
                        32 => { // VPS (Video Parameter Set)
                            has_vps = true;
                            let vps = packet[nalu_start..nalu_end].to_vec();
                            if self.last_vps.as_ref().map_or(true, |last| *last != vps) {
                                self.last_vps = Some(vps);
                            }
                        },
                        33 => { // SPS
                            has_sps = true;
                            let sps = packet[nalu_start..nalu_end].to_vec();
                            if self.last_sps.as_ref().map_or(true, |last| *last != sps) {
                                self.last_sps = Some(sps);
                                found_new_sps = true;
                            }
                        },
                        34 => { // PPS
                            has_pps = true;
                            let pps = packet[nalu_start..nalu_end].to_vec();
                            if self.last_pps.as_ref().map_or(true, |last| *last != pps) {
                                self.last_pps = Some(pps);
                                found_new_pps = true;
                            }
                        },
                        16..=23 => { // I-frame (IDR and CRA)
                            is_iframe = true;
                        },
                        _ => {}
                    }
                } else {
                    // H264 NALU type parsing (5 bits)
                    let nalu_type = nalu_header & 0x1F;
                    match nalu_type {
                        7 => { // SPS
                            has_sps = true;
                            let sps = packet[nalu_start..nalu_end].to_vec();
                            if self.last_sps.as_ref().map_or(true, |last| *last != sps) {
                                self.last_sps = Some(sps);
                                found_new_sps = true;
                            }
                        },
                        8 => { // PPS
                            has_pps = true;
                            let pps = packet[nalu_start..nalu_end].to_vec();
                            if self.last_pps.as_ref().map_or(true, |last| *last != pps) {
                                self.last_pps = Some(pps);
                                found_new_pps = true;
                            }
                        },
                        5 => { // I-frame (IDR)
                            is_iframe = true;
                        },
                        _ => {}
                    }
                }
                pos = nalu_end;
            } else {
                pos += 1;
            }
        }

        // Check if complete stream header exists based on encoding format
        let has_complete_header = if is_h265 {
            has_sps && has_pps  // H265 requires at least SPS+PPS, VPS is optional
        } else {
            has_sps && has_pps  // H264 requires SPS+PPS
        };

        unsafe {
            // If decoder is already initialized, send all frames directly (including P/B frames)
            if self.decoder_initialized {
                (&mut *self.frames).clear();
                let ret = ffmpeg_ram_decode(
                    self.codec,
                    packet.as_ptr(),
                    packet.len() as c_int,
                    self.frames as *const _ as *const c_void,
                );
                return if ret < 0 {
                    Err(ret)
                } else {
                    Ok(&mut *self.frames)
                };
            }

            // Logic when decoder is not initialized
            let codec_type = if is_h265 { "H265" } else { "H264" };
            
            // Case 1: Encountered SPS/PPS but missing I-frame, cache it (for subsequent merging)
            if (has_sps || has_pps || has_vps) && !is_iframe {
                log::warn!("[{}#{}] cache packet: header without I-frame (vps={}, sps={}, pps={}, iframe={})", 
                    codec_type, self.instance_id, has_vps, has_sps, has_pps, is_iframe);
                match &mut self.packet_cache {
                    Some(buf) => buf.extend_from_slice(packet),
                    None => self.packet_cache = Some(packet.to_vec()),
                }
                return Ok(&mut *self.frames);
            }

            // Case 2: Encountered I-frame but missing complete header, try to merge with cached header info
            if is_iframe && !has_complete_header {
                log::warn!("[{}#{}] I-frame without complete header, trying to merge with cache (vps={}, sps={}, pps={}, iframe={})", 
                    codec_type, self.instance_id, has_vps, has_sps, has_pps, is_iframe);
                if let Some(ref cache) = self.packet_cache {
                    let mut merged = cache.clone();
                    merged.extend_from_slice(packet);
                    (&mut *self.frames).clear();
                    let ret = ffmpeg_ram_decode(
                        self.codec,
                        merged.as_ptr(),
                        merged.len() as c_int,
                        self.frames as *const _ as *const c_void,
                    );
                    self.packet_cache = None;
                    if ret >= 0 {
                        self.decoder_initialized = true;
                        log::info!("[{}#{}] Decoder initialized successfully with merged header + I-frame", 
                            codec_type, self.instance_id);
                    }
                    return if ret < 0 {
                        Err(ret)
                    } else {
                        Ok(&mut *self.frames)
                    };
                } else {
                    // No cache, send directly
                    (&mut *self.frames).clear();
                    let ret = ffmpeg_ram_decode(
                        self.codec,
                        packet.as_ptr(),
                        packet.len() as c_int,
                        self.frames as *const _ as *const c_void,
                    );
                    if ret >= 0 {
                        self.decoder_initialized = true;
                        log::info!("[{}#{}] Decoder initialized successfully with I-frame only", 
                            codec_type, self.instance_id);
                    }
                    return if ret < 0 {
                        Err(ret)
                    } else {
                        Ok(&mut *self.frames)
                    };
                }
            }
            
            // Case 3: Complete header + I-frame are all present, send directly
            if has_complete_header && is_iframe {
                log::info!("[{}#{}] Complete header received, decoding (vps={}, sps={}, pps={}, iframe={})", 
                    codec_type, self.instance_id, has_vps, has_sps, has_pps, is_iframe);
                (&mut *self.frames).clear();
                let ret = ffmpeg_ram_decode(
                    self.codec,
                    packet.as_ptr(),
                    packet.len() as c_int,
                    self.frames as *const _ as *const c_void,
                );
                self.packet_cache = None;
                if ret >= 0 {
                    self.decoder_initialized = true;
                    log::info!("[{}#{}] Decoder initialized successfully with complete header", 
                        codec_type, self.instance_id);
                }
                return if ret < 0 {
                    Err(ret)
                } else {
                    Ok(&mut *self.frames)
                };
            }
            
            // Other cases (such as all missing), directly ignore
            log::debug!("[{}#{}] ignoring packet: missing header/I-frame (vps={}, sps={}, pps={}, iframe={})", 
                codec_type, self.instance_id, has_vps, has_sps, has_pps, is_iframe);
            return Ok(&mut *self.frames);
        }
    }

    unsafe extern "C" fn callback(
        obj: *const c_void,
        width: c_int,
        height: c_int,
        pixfmt: c_int,
        linesizes: *mut c_int,
        datas: *mut *mut u8,
        key: c_int,
    ) {
        let frames = &mut *(obj as *mut Vec<DecodeFrame>);
        let datas = from_raw_parts(datas, AV_NUM_DATA_POINTERS as _);
        let linesizes = from_raw_parts(linesizes, AV_NUM_DATA_POINTERS as _);

        let mut frame = DecodeFrame {
            pixfmt: std::mem::transmute(pixfmt),
            width,
            height,
            data: vec![],
            linesize: vec![],
            key: key != 0,
        };

        if pixfmt == AVPixelFormat::AV_PIX_FMT_YUV420P as c_int {
            let y = from_raw_parts(datas[0], (linesizes[0] * height) as usize).to_vec();
            let u = from_raw_parts(datas[1], (linesizes[1] * height / 2) as usize).to_vec();
            let v = from_raw_parts(datas[2], (linesizes[2] * height / 2) as usize).to_vec();

            frame.data.push(y);
            frame.data.push(u);
            frame.data.push(v);

            frame.linesize.push(linesizes[0]);
            frame.linesize.push(linesizes[1]);
            frame.linesize.push(linesizes[2]);

            frames.push(frame);
        } else if pixfmt == AVPixelFormat::AV_PIX_FMT_NV12 as c_int {
            let y = from_raw_parts(datas[0], (linesizes[0] * height) as usize).to_vec();
            let uv = from_raw_parts(datas[1], (linesizes[1] * height / 2) as usize).to_vec();

            frame.data.push(y);
            frame.data.push(uv);

            frame.linesize.push(linesizes[0]);
            frame.linesize.push(linesizes[1]);

            frames.push(frame);
        } else {
            error!("unsupported pixfmt {}", pixfmt as i32);
        }
    }

    pub fn available_decoders() -> Vec<CodecInfo> {
        use log::debug;

        #[allow(unused_mut)]
        let mut codecs: Vec<CodecInfo> = vec![];
        // windows disable nvdec to avoid gpu stuck
        #[cfg(target_os = "linux")]
        {
            let (nv, _, _) = crate::common::supported_gpu(false);
            debug!("Linux GPU support detected - NV: {}", nv);
            if nv {
                codecs.push(CodecInfo {
                    name: "h264".to_owned(),
                    format: H264,
                    hwdevice: AV_HWDEVICE_TYPE_CUDA,
                    priority: Priority::Good as _,
                    ..Default::default()
                });
                codecs.push(CodecInfo {
                    name: "hevc".to_owned(),
                    format: H265,
                    hwdevice: AV_HWDEVICE_TYPE_CUDA,
                    priority: Priority::Good as _,
                    ..Default::default()
                });
            }
        }

        #[cfg(target_os = "windows")]
        {
            codecs.append(&mut vec![
                CodecInfo {
                    name: "h264".to_owned(),
                    format: H264,
                    hwdevice: AV_HWDEVICE_TYPE_D3D11VA,
                    priority: Priority::Best as _,
                    ..Default::default()
                },
                CodecInfo {
                    name: "hevc".to_owned(),
                    format: H265,
                    hwdevice: AV_HWDEVICE_TYPE_D3D11VA,
                    priority: Priority::Best as _,
                    ..Default::default()
                },
            ]);
        }

        #[cfg(target_os = "linux")]
        {
            codecs.append(&mut vec![
                CodecInfo {
                    name: "h264".to_owned(),
                    format: H264,
                    hwdevice: AV_HWDEVICE_TYPE_VAAPI,
                    priority: Priority::Good as _,
                    ..Default::default()
                },
                CodecInfo {
                    name: "hevc".to_owned(),
                    format: H265,
                    hwdevice: AV_HWDEVICE_TYPE_VAAPI,
                    priority: Priority::Good as _,
                    ..Default::default()
                },
            ]);
        }

        #[cfg(target_os = "macos")]
        {
            let (_, _, h264, h265) = crate::common::get_video_toolbox_codec_support();
            debug!(
                "VideoToolbox decode support - H264: {}, H265: {}",
                h264, h265
            );
            if h264 {
                codecs.push(CodecInfo {
                    name: "h264".to_owned(),
                    format: H264,
                    hwdevice: AV_HWDEVICE_TYPE_VIDEOTOOLBOX,
                    priority: Priority::Best as _,
                    ..Default::default()
                });
            }
            if h265 {
                codecs.push(CodecInfo {
                    name: "hevc".to_owned(),
                    format: H265,
                    hwdevice: AV_HWDEVICE_TYPE_VIDEOTOOLBOX,
                    priority: Priority::Best as _,
                    ..Default::default()
                });
            }
        }

        let mut res = Vec::<CodecInfo>::new();
        let buf264 = &crate::common::DATA_H264_720P[..];
        let buf265 = &crate::common::DATA_H265_720P[..];

        for codec in codecs {
            // Skip if this format already exists in results
            if res
                .iter()
                .any(|existing: &CodecInfo| existing.format == codec.format)
            {
                continue;
            }

            debug!(
                "Testing decoder: {} (hwdevice: {:?})",
                codec.name, codec.hwdevice
            );

            let c = DecodeContext {
                name: codec.name.clone(),
                device_type: codec.hwdevice,
                thread_count: 4,
            };

            match Decoder::new(c) {
                Ok(mut decoder) => {
                    debug!("Decoder {} created successfully", codec.name);
                    let data = match codec.format {
                        H264 => buf264,
                        H265 => buf265,
                        _ => {
                            log::error!("Unsupported format: {:?}, skipping", codec.format);
                            continue;
                        }
                    };

                    let start = Instant::now();

                    match decoder.decode(data) {
                        Ok(_) => {
                            let elapsed = start.elapsed().as_millis();

                            if elapsed < TEST_TIMEOUT_MS as _ {
                                debug!("Decoder {} test passed", codec.name);
                                res.push(codec);
                            } else {
                                debug!(
                                    "Decoder {} test failed - timeout: {}ms",
                                    codec.name, elapsed
                                );
                            }
                        }
                        Err(err) => {
                            debug!("Decoder {} test failed with error: {}", codec.name, err);
                        }
                    }
                }
                Err(_) => {
                    debug!("Failed to create decoder {}", codec.name);
                }
            }
        }

        let soft = CodecInfo::soft();
        if let Some(c) = soft.h264 {
            res.push(c);
        }
        if let Some(c) = soft.h265 {
            res.push(c);
        }

        res
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe {
            ffmpeg_ram_free_decoder(self.codec);
            self.codec = std::ptr::null_mut();
            let _ = Box::from_raw(self.frames);
        }
    }
}
