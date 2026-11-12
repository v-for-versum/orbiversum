//! JC3636K718C firmware: two applets (Starlids, Sine), a hold-to-open applet
//! menu, audio on core 1, UI on core 0.
use board_jc3636k718c::menu::{Menu, Outcome};
use board_jc3636k718c::JcBoard;
use esp_idf_svc::sys::{self, esp_err_t, EspError};
use os::{AppletUi, Board, Canvas, Dirty, Dispatcher, Params, DEFAULT_SAMPLE_RATE};
use std::sync::Arc;
use std::time::Duration;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BspFeatures {
    pub display: bool,
    pub touch: bool,
    pub encoder: bool,
    pub led_ring: bool,
    pub audio_out: bool,
    pub audio_in: bool,
    pub sdcard: bool,
    pub battery: bool,
}

unsafe extern "C" {
    fn bsp_init(features: *const BspFeatures) -> esp_err_t;
    fn bsp_display_backlight_show();
}

/// UI tick. Short enough for the knob, long enough to leave core 0 mostly idle.
const TICK: Duration = Duration::from_millis(16);

extern "C" {
    /// Xtensa CPU cycle counter (ESP-IDF HAL).
    fn xthal_get_ccount() -> u32;
}

fn cycles() -> u32 {
    unsafe { xthal_get_ccount() }
}

/// CPU cycles per microsecond (CONFIG_ESP_DEFAULT_CPU_FREQ_MHZ_240).
const CYCLES_PER_US: u32 = 240;

fn now_us() -> i64 {
    unsafe { sys::esp_timer_get_time() }
}

fn main() {
    sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    // Display, touch and knob (the C BSP default set). Audio is brought up by JcBoard.
    let features = BspFeatures { display: true, touch: true, encoder: true, ..Default::default() };
    EspError::convert(unsafe { bsp_init(&features) }).expect("bsp_init failed");
    log::info!("rust: firmware {}", env!("FW_GIT_DESC"));
    log::info!("rust: bsp ok");

    // Takes the panel from LVGL (stopped), allocates the DMA strip buffer, starts audio hardware.
    let mut board = JcBoard::new().expect("board init failed");

    dsp::prof::set_clock(cycles);
    let params = Arc::new(Params::default());
    let seed = now_us() as u64;
    let (audio_halves, mut uis): (Vec<_>, Vec<Box<dyn AppletUi>>) =
        applets::all(seed, DEFAULT_SAMPLE_RATE as f32, params.clone()).into_iter().unzip();
    let mut disp = Dispatcher::with_params(audio_halves, params);
    let mut path = disp.take_audio();
    board
        .audio_out()
        .start(DEFAULT_SAMPLE_RATE, Box::new(move |out| path.audio(out)))
        .expect("audio start failed");
    log::info!("rust: audio started");

    let names: Vec<&'static str> = (0..disp.len()).map(|i| disp.info(i).unwrap().name).collect();
    let mut menu = Menu::new(names);

    let psram = unsafe { sys::heap_caps_get_total_size(sys::MALLOC_CAP_SPIRAM) };
    log::info!("rust: psram total = {psram} bytes");

    let (mut flushes, mut shown_backlight) = (0u32, false);
    let mut last_log = now_us();
    let mut window_flush_us = 0i64;
    let mut window_flush_px = 0u64;
    let mut window_frames = 0u32;
    loop {
        // Completes an applet switch (stop/start happen here, never on the audio thread).
        disp.service();

        while let Some(ev) = board.poll_input() {
            match menu.handle(ev, disp.selected()) {
                Outcome::Pass(e) => uis[disp.selected()].input(e),
                Outcome::Consumed => {}
                Outcome::Select(i) => {
                    disp.select(i);
                    // The menu painted over the framebuffer: repaint the chosen UI fully.
                    uis[i].invalidate();
                }
            }
        }

        let mut dirty = Dirty::new();
        {
            let mut canvas = Canvas(board.display().surface());
            if menu.is_open() {
                menu.draw(&mut canvas, &mut dirty);
            } else {
                uis[disp.selected()].draw(&mut canvas, &mut dirty);
            }
        }
        if !dirty.is_empty() {
            let t0 = now_us();
            let sent_before = board.display_mut().pixels_sent();
            board.display().flush(&dirty);
            let (us, px) = (now_us() - t0, board.display_mut().pixels_sent() - sent_before);
            flushes += 1;
            if dirty.is_full() || flushes <= 3 {
                log::info!("display: {} flush {} px in {} us", if dirty.is_full() { "full" } else { "partial" }, px, us);
            }
            window_flush_us += us;
            window_flush_px += px;
            window_frames += 1;
            if !shown_backlight {
                unsafe { bsp_display_backlight_show() };
                shown_backlight = true;
            }
        }

        if now_us() - last_log > 5_000_000 {
            if window_frames > 0 {
                log::info!(
                    "display: {} flushes in 5 s, mean {} us / {} px; panel errors {}; free internal heap {} B",
                    window_frames,
                    window_flush_us / window_frames as i64,
                    window_flush_px / window_frames as u64,
                    board.display_mut().send_errors(),
                    unsafe { sys::heap_caps_get_free_size(sys::MALLOC_CAP_INTERNAL) },
                );
            }
            let (blocks, t) = dsp::prof::take();
            if blocks > 0 {
                let mut line = std::string::String::new();
                let mut total = 0u32;
                for (name, v) in dsp::prof::NAMES.iter().zip(t.iter()) {
                    let us = v / CYCLES_PER_US / blocks;
                    total += us;
                    line.push_str(&format!(" {name} {us}"));
                }
                log::info!("prof: us/block over {blocks} blocks:{line} | sum {total}");
            }
            log::info!("rust: alive, selected {}", disp.selected());
            window_flush_us = 0;
            window_flush_px = 0;
            window_frames = 0;
            last_log = now_us();
        }
        std::thread::sleep(TICK);
    }
}
