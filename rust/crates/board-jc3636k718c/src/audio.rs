//! Audio output: a task pinned to core 1 pulls blocks from `render` and writes
//! them to the PCM5100A through the C BSP.
use crate::convert::frames_to_i16;
use crate::ffi::{bsp_audio_out_init, bsp_audio_out_write, BspAudioOutConfig};
use esp_idf_svc::hal::cpu::Core;
use esp_idf_svc::hal::task::thread::ThreadSpawnConfiguration;
use esp_idf_svc::sys::{self, esp_err_t, ESP_OK};

pub const SAMPLE_RATE: u32 = 48_000;
/// Frames per render call.
pub const BLOCK_FRAMES: usize = 128;
/// FreeRTOS priority of the audio task (the main task runs at 1).
const AUDIO_PRIORITY: u8 = 20;
const AUDIO_STACK: usize = 8192;
/// Log core-1 load every this many blocks (~2 s at 48 kHz / 128).
const LOAD_LOG_BLOCKS: u32 = 750;

#[derive(Debug)]
pub enum BoardError {
    /// A BSP call returned a non-OK `esp_err_t`.
    Esp(esp_err_t),
    /// `AudioOut::start` called twice.
    AlreadyStarted,
    /// Requested sample rate differs from the fixed I2S clock.
    UnsupportedSampleRate(u32),
    /// The task could not be spawned.
    Spawn(String),
}

impl core::fmt::Display for BoardError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for BoardError {}

pub type RenderFn = Box<dyn FnMut(&mut [[f32; 2]]) + Send>;

/// Minimal audio-out interface; Task 8 replaces it with the `os` trait.
pub trait AudioOut {
    fn start(&mut self, sr: u32, render: RenderFn) -> Result<(), BoardError>;
}

pub struct JcAudio {
    started: bool,
}

impl JcAudio {
    /// Bring up I2S TX and unmute the DAC (48 kHz, 16-bit, stereo).
    pub fn new() -> Result<JcAudio, BoardError> {
        let cfg = BspAudioOutConfig { sample_rate: SAMPLE_RATE, bits: 16, stereo: true };
        let err = unsafe { bsp_audio_out_init(&cfg) };
        if err != ESP_OK {
            return Err(BoardError::Esp(err));
        }
        Ok(JcAudio { started: false })
    }
}

impl AudioOut for JcAudio {
    fn start(&mut self, sr: u32, mut render: RenderFn) -> Result<(), BoardError> {
        if self.started {
            return Err(BoardError::AlreadyStarted);
        }
        if sr != SAMPLE_RATE {
            return Err(BoardError::UnsupportedSampleRate(sr));
        }
        ThreadSpawnConfiguration {
            name: Some(c"audio"),
            stack_size: AUDIO_STACK,
            priority: AUDIO_PRIORITY,
            pin_to_core: Some(Core::Core1),
            ..Default::default()
        }
        .set()
        .map_err(|e| BoardError::Spawn(format!("{e:?}")))?;
        let spawned = std::thread::Builder::new().stack_size(AUDIO_STACK).spawn(move || {
            // All buffers live on the task stack: no allocation in the loop.
            let mut frames = [[0.0f32; 2]; BLOCK_FRAMES];
            let mut pcm = [0i16; BLOCK_FRAMES * 2];
            let period_us = (BLOCK_FRAMES as u64 * 1_000_000) / SAMPLE_RATE as u64;
            let mut busy_us: u64 = 0;
            let mut blocks: u32 = 0;
            loop {
                let t0 = unsafe { sys::esp_timer_get_time() };
                render(&mut frames);
                frames_to_i16(&frames, &mut pcm);
                let t1 = unsafe { sys::esp_timer_get_time() };
                busy_us += (t1 - t0) as u64;

                // Blocks until the DMA queue has room; this paces the loop.
                let mut written = 0usize;
                let err = unsafe {
                    bsp_audio_out_write(
                        pcm.as_ptr().cast(),
                        core::mem::size_of_val(&pcm),
                        &mut written,
                        u32::MAX,
                    )
                };
                if err != ESP_OK {
                    // Do not spin at full priority if the driver rejects writes.
                    unsafe { sys::vTaskDelay(1) };
                }

                blocks += 1;
                if blocks == LOAD_LOG_BLOCKS {
                    // Core-1 audio load = render+convert time / block period.
                    let pct10 = busy_us * 1000 / (period_us * blocks as u64);
                    log::info!("audio: core1 load {}.{}%", pct10 / 10, pct10 % 10);
                    busy_us = 0;
                    blocks = 0;
                }
            }
        });
        // Restore defaults for threads spawned later.
        let _ = ThreadSpawnConfiguration::default().set();
        spawned.map_err(|e| BoardError::Spawn(e.to_string()))?;
        self.started = true;
        Ok(())
    }
}
