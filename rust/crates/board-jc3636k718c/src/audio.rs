//! Audio output: a task pinned to core 1 pulls blocks from `render` and writes
//! them to the PCM5100A through the C BSP.
use crate::convert::frames_to_i16;
use crate::ffi::{bsp_audio_out_init, bsp_audio_out_write, BspAudioOutConfig};
use esp_idf_svc::hal::cpu::Core;
use esp_idf_svc::hal::task::thread::ThreadSpawnConfiguration;
use esp_idf_svc::sys::{self, esp_err_t, ESP_OK};
use os::{AudioError, AudioOut, RenderFn};

pub const SAMPLE_RATE: u32 = 48_000;
/// Frames per render call.
pub const BLOCK_FRAMES: usize = 128;
/// FreeRTOS priority of the audio task (the main task runs at 1).
const AUDIO_PRIORITY: u8 = 20;
const AUDIO_STACK: usize = 8192;
/// Log core-1 load every this many blocks (~2 s at 48 kHz / 128).
const LOAD_LOG_BLOCKS: u32 = 750;

pub struct JcAudio {
    started: bool,
}

impl JcAudio {
    /// Bring up I2S TX and unmute the DAC (48 kHz, 16-bit, stereo).
    pub fn new() -> Result<JcAudio, AudioError> {
        let cfg = BspAudioOutConfig { sample_rate: SAMPLE_RATE, bits: 16, stereo: true };
        let err = unsafe { bsp_audio_out_init(&cfg) };
        if err != ESP_OK {
            return Err(AudioError::Backend(err));
        }
        Ok(JcAudio { started: false })
    }
}

impl AudioOut for JcAudio {
    fn start(&mut self, sr: u32, mut render: RenderFn) -> Result<(), AudioError> {
        if self.started {
            return Err(AudioError::AlreadyStarted);
        }
        if sr != SAMPLE_RATE {
            return Err(AudioError::UnsupportedSampleRate(sr));
        }
        ThreadSpawnConfiguration {
            name: Some(c"audio"),
            stack_size: AUDIO_STACK,
            priority: AUDIO_PRIORITY,
            pin_to_core: Some(Core::Core1),
            ..Default::default()
        }
        .set()
        .map_err(|_| AudioError::Spawn)?;
        let spawned = std::thread::Builder::new().stack_size(AUDIO_STACK).spawn(move || {
            // All buffers live on the task stack: no allocation in the loop.
            let mut frames = [[0.0f32; 2]; BLOCK_FRAMES];
            let mut pcm = [0i16; BLOCK_FRAMES * 2];
            let period_us = (BLOCK_FRAMES as u64 * 1_000_000) / SAMPLE_RATE as u64;
            let total = core::mem::size_of_val(&pcm);
            let mut busy_us: u64 = 0;
            let mut max_us: u64 = 0;
            let mut blocks: u32 = 0;
            let mut errors: u32 = 0;
            let mut short: u32 = 0;
            let mut first_err: esp_err_t = ESP_OK;
            let mut stack_logged = false;
            loop {
                let t0 = unsafe { sys::esp_timer_get_time() };
                render(&mut frames);
                frames_to_i16(&frames, &mut pcm);
                let t1 = unsafe { sys::esp_timer_get_time() };
                let dt = (t1 - t0) as u64;
                busy_us += dt;
                max_us = max_us.max(dt);

                // Blocks until the DMA queue has room; this paces the loop.
                // A short write is completed with the remainder; if the
                // driver errors, the rest of the block is dropped (underrun).
                let base = pcm.as_ptr().cast::<u8>();
                let mut off = 0usize;
                let mut failed = false;
                while off < total {
                    let mut written = 0usize;
                    let err = unsafe {
                        bsp_audio_out_write(
                            base.add(off).cast(),
                            total - off,
                            &mut written,
                            u32::MAX,
                        )
                    };
                    off += written;
                    if err != ESP_OK {
                        errors += 1;
                        if first_err == ESP_OK {
                            first_err = err;
                        }
                        failed = true;
                        break;
                    }
                    if off < total {
                        short += 1;
                    }
                }
                if failed {
                    // Do not spin at full priority if the driver rejects writes.
                    unsafe { sys::vTaskDelay(1) };
                }

                blocks += 1;
                if blocks == LOAD_LOG_BLOCKS {
                    // Core-1 audio load = render+convert time / block period.
                    let pct10 = busy_us * 1000 / (period_us * blocks as u64);
                    let max_pct10 = max_us * 1000 / period_us;
                    log::info!(
                        "audio: core1 load mean {}.{}% max {}.{}% ({} us of {} us); write errors {} (first {}), short writes {}",
                        pct10 / 10, pct10 % 10, max_pct10 / 10, max_pct10 % 10,
                        max_us, period_us, errors, first_err, short
                    );
                    if !stack_logged {
                        let hw = unsafe { sys::uxTaskGetStackHighWaterMark(core::ptr::null_mut()) };
                        log::info!("audio: stack high-water mark {} bytes free", hw);
                        stack_logged = true;
                    }
                    busy_us = 0;
                    max_us = 0;
                    blocks = 0;
                    errors = 0;
                    short = 0;
                    first_err = ESP_OK;
                }
            }
        });
        // Restore defaults for threads spawned later.
        let _ = ThreadSpawnConfiguration::default().set();
        spawned.map_err(|_| AudioError::Spawn)?;
        self.started = true;
        Ok(())
    }
}
