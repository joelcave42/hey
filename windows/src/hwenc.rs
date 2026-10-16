//! H.264 encoding on the graphics card, through Media Foundation's hardware
//! encoder (NVENC on NVIDIA, Quick Sync on Intel, AMF on AMD).
//!
//! Hardware encoders in Media Foundation are asynchronous: they say when
//! they want a frame (METransformNeedInput) and when one is encoded
//! (METransformHaveOutput). Hey sets them up for low latency (no reordered
//! frames, constant bitrate), so each frame in gives one frame out straight
//! away.

use std::mem::ManuallyDrop;
use std::sync::Once;
use std::time::{Duration, Instant};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree};
use windows::Win32::System::Variant::{VARIANT, VT_BOOL, VT_UI4};
use windows::core::{GUID, Interface, Result};

pub struct HwEncoder {
    transform: IMFTransform,
    events: IMFMediaEventGenerator,
    codec: Option<ICodecAPI>,
    provides_samples: bool,
    output_size: u32,
    /// Frames the encoder has asked for and not yet been given.
    wanted: u32,
    frame: i64,
    fps: u32,
    pub name: String,
}

// The encoder is only ever used from the thread that streams to one phone.
unsafe impl Send for HwEncoder {}

impl HwEncoder {
    /// Finds a hardware H.264 encoder and sets it up for live streaming.
    pub fn new(width: u32, height: u32, fps: u32, bitrate: u32) -> Result<HwEncoder> {
        static STARTUP: Once = Once::new();
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            STARTUP.call_once(|| {
                let _ = MFStartup(MF_VERSION, MFSTARTUP_FULL);
            });
        }
        let mut last_error = windows::core::Error::from_hresult(MF_E_NOT_FOUND);
        for activate in hardware_encoders()? {
            match unsafe { Self::configure(&activate, width, height, fps, bitrate) } {
                Ok(encoder) => return Ok(encoder),
                Err(e) => last_error = e,
            }
        }
        Err(last_error)
    }

    unsafe fn configure(
        activate: &IMFActivate,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
    ) -> Result<HwEncoder> {
        unsafe {
            let transform: IMFTransform = activate.ActivateObject()?;
            let name = friendly_name(activate);
            let attributes = transform.GetAttributes()?;
            attributes.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1)?;
            let _ = attributes.SetUINT32(&MF_LOW_LATENCY, 1);
            let events: IMFMediaEventGenerator = transform.cast()?;

            let size = ((width as u64) << 32) | height as u64;
            let rate = ((fps as u64) << 32) | 1;

            let output = MFCreateMediaType()?;
            output.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            output.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
            output.SetUINT32(&MF_MT_AVG_BITRATE, bitrate)?;
            output.SetUINT64(&MF_MT_FRAME_SIZE, size)?;
            output.SetUINT64(&MF_MT_FRAME_RATE, rate)?;
            output.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            output.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_Main.0 as u32)?;
            transform.SetOutputType(0, &output, 0)?;

            let input = MFCreateMediaType()?;
            input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
            input.SetUINT64(&MF_MT_FRAME_SIZE, size)?;
            input.SetUINT64(&MF_MT_FRAME_RATE, rate)?;
            input.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            transform.SetInputType(0, &input, 0)?;

            let codec: Option<ICodecAPI> = transform.cast().ok();
            if let Some(codec) = &codec {
                let _ = codec.SetValue(&CODECAPI_AVLowLatencyMode, &variant_bool(true));
                let _ = codec.SetValue(
                    &CODECAPI_AVEncCommonRateControlMode,
                    &variant_u32(eAVEncCommonRateControlMode_CBR.0 as u32),
                );
                let _ = codec.SetValue(&CODECAPI_AVEncCommonMeanBitRate, &variant_u32(bitrate));
                // A keyframe every few seconds, so a dropped connection recovers quickly.
                let _ = codec.SetValue(&CODECAPI_AVEncMPVGOPSize, &variant_u32(fps * 4));
                let _ = codec.SetValue(&CODECAPI_AVEncMPVDefaultBPictureCount, &variant_u32(0));
            }

            let info = transform.GetOutputStreamInfo(0)?;
            let provides_samples = info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 != 0;

            transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0)?;
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;

            Ok(HwEncoder {
                transform,
                events,
                codec,
                provides_samples,
                output_size: info.cbSize.max(width * height),
                wanted: 0,
                frame: 0,
                fps,
                name,
            })
        }
    }

    /// Encodes one NV12 frame and returns the H.264 it produced (Annex B).
    /// Can be empty if the encoder holds the frame back.
    pub fn encode(&mut self, nv12: &[u8], keyframe: bool) -> Result<Vec<u8>> {
        unsafe {
            if keyframe && let Some(codec) = &self.codec {
                let _ = codec.SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &variant_u32(1));
            }
            let deadline = Instant::now() + Duration::from_millis(250);
            let mut sent = false;
            let mut out = Vec::new();
            while Instant::now() < deadline {
                if !sent && self.wanted > 0 {
                    self.give(nv12)?;
                    self.wanted -= 1;
                    sent = true;
                }
                match self.events.GetEvent(MF_EVENT_FLAG_NO_WAIT) {
                    Ok(event) => match MF_EVENT_TYPE(event.GetType()? as i32) {
                        t if t == METransformNeedInput => self.wanted += 1,
                        t if t == METransformHaveOutput => {
                            self.take(&mut out)?;
                            if sent {
                                return Ok(out);
                            }
                        }
                        _ => {}
                    },
                    Err(e) if e.code() == MF_E_NO_EVENTS_AVAILABLE => {
                        std::thread::sleep(Duration::from_micros(500))
                    }
                    Err(e) => return Err(e),
                }
            }
            Ok(out)
        }
    }

    unsafe fn give(&mut self, nv12: &[u8]) -> Result<()> {
        unsafe {
            let buffer = MFCreateMemoryBuffer(nv12.len() as u32)?;
            let mut data = std::ptr::null_mut();
            buffer.Lock(&mut data, None, None)?;
            std::ptr::copy_nonoverlapping(nv12.as_ptr(), data, nv12.len());
            buffer.Unlock()?;
            buffer.SetCurrentLength(nv12.len() as u32)?;
            let sample = MFCreateSample()?;
            sample.AddBuffer(&buffer)?;
            let duration = 10_000_000 / self.fps as i64;
            sample.SetSampleTime(self.frame * duration)?;
            sample.SetSampleDuration(duration)?;
            self.frame += 1;
            self.transform.ProcessInput(0, &sample, 0)
        }
    }

    unsafe fn take(&mut self, out: &mut Vec<u8>) -> Result<()> {
        unsafe {
            let sample = if self.provides_samples {
                None
            } else {
                let sample = MFCreateSample()?;
                sample.AddBuffer(&MFCreateMemoryBuffer(self.output_size)?)?;
                Some(sample)
            };
            let mut output = [MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: ManuallyDrop::new(sample),
                dwStatus: 0,
                pEvents: ManuallyDrop::new(None),
            }];
            let mut status = 0;
            let result = self.transform.ProcessOutput(0, &mut output, &mut status);
            let sample = ManuallyDrop::take(&mut output[0].pSample);
            drop(ManuallyDrop::take(&mut output[0].pEvents));
            result?;
            let Some(sample) = sample else { return Ok(()) };
            let buffer = sample.ConvertToContiguousBuffer()?;
            let mut data = std::ptr::null_mut();
            let mut length = 0;
            buffer.Lock(&mut data, None, Some(&mut length))?;
            out.extend_from_slice(std::slice::from_raw_parts(data, length as usize));
            buffer.Unlock()
        }
    }
}

impl Drop for HwEncoder {
    fn drop(&mut self) {
        unsafe {
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
            if let Ok(shutdown) = self.transform.cast::<IMFShutdown>() {
                let _ = shutdown.Shutdown();
            }
        }
    }
}

/// The hardware H.264 encoders Windows knows about, best first.
fn hardware_encoders() -> Result<Vec<IMFActivate>> {
    let output = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_H264,
    };
    let mut list = std::ptr::null_mut();
    let mut count = 0;
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
            None,
            Some(&output),
            &mut list,
            &mut count,
        )?;
        let found = (0..count as usize)
            .filter_map(|i| std::ptr::read(list.add(i)))
            .collect();
        CoTaskMemFree(Some(list as *const _));
        Ok(found)
    }
}

fn friendly_name(activate: &IMFActivate) -> String {
    unsafe {
        let mut text = windows::core::PWSTR::null();
        let mut length = 0;
        if activate
            .GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &mut text, &mut length)
            .is_ok()
        {
            let name = text.to_string().unwrap_or_default();
            CoTaskMemFree(Some(text.0 as *const _));
            return name;
        }
        "hardware encoder".into()
    }
}

fn variant_u32(value: u32) -> VARIANT {
    let mut v = VARIANT::default();
    unsafe {
        (*v.Anonymous.Anonymous).vt = VT_UI4;
        (*v.Anonymous.Anonymous).Anonymous.ulVal = value;
    }
    v
}

fn variant_bool(value: bool) -> VARIANT {
    let mut v = VARIANT::default();
    unsafe {
        (*v.Anonymous.Anonymous).vt = VT_BOOL;
        (*v.Anonymous.Anonymous).Anonymous.boolVal = value.into();
    }
    v
}

/// BGRA to NV12 (BT.601, limited range, which is what H.264 decoders assume
/// when the stream doesn't say otherwise). Width and height must be even.
pub fn bgra_to_nv12(bgra: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut out = vec![0u8; width * height * 3 / 2];
    let (luma, chroma) = out.split_at_mut(width * height);
    for y in 0..height {
        let row = &bgra[y * width * 4..(y + 1) * width * 4];
        let dst = &mut luma[y * width..(y + 1) * width];
        for x in 0..width {
            let (b, g, r) = (
                row[x * 4] as i32,
                row[x * 4 + 1] as i32,
                row[x * 4 + 2] as i32,
            );
            dst[x] = (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16) as u8;
        }
    }
    for y in (0..height).step_by(2) {
        for x in (0..width).step_by(2) {
            let (mut r, mut g, mut b) = (0, 0, 0);
            for (dy, dx) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                let i = ((y + dy) * width + x + dx) * 4;
                b += bgra[i] as i32;
                g += bgra[i + 1] as i32;
                r += bgra[i + 2] as i32;
            }
            let (r, g, b) = (r / 4, g / 4, b / 4);
            let i = (y / 2) * width + x;
            chroma[i] = (((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128) as u8;
            chroma[i + 1] = (((112 * r - 94 * g - 18 * b + 128) >> 8) + 128) as u8;
        }
    }
    out
}

#[allow(dead_code)]
const _: GUID = MFVideoFormat_NV12;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nv12_has_the_right_size_and_colours() {
        let white = vec![255u8; 4 * 4 * 4];
        let nv12 = bgra_to_nv12(&white, 4, 4);
        assert_eq!(nv12.len(), 4 * 4 * 3 / 2);
        assert_eq!(nv12[0], 235); // white is Y=235 in limited range
        assert_eq!(nv12[16], 128); // and has no colour
    }

    /// Needs a graphics card with an H.264 encoder: `cargo test hardware -- --ignored`
    #[test]
    #[ignore]
    fn hardware_encoder_encodes_a_frame() {
        let mut encoder = HwEncoder::new(1280, 720, 60, 8_000_000).expect("no hardware encoder");
        println!("using {}", encoder.name);
        let frame = bgra_to_nv12(&vec![90u8; 1280 * 720 * 4], 1280, 720);
        let mut total = 0;
        let start = Instant::now();
        for i in 0..60 {
            total += encoder.encode(&frame, i == 0).unwrap().len();
        }
        println!("60 frames in {:?}, {total} bytes", start.elapsed());
        assert!(total > 0);
    }
}
