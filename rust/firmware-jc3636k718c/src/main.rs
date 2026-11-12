//! Spike: Rust firmware linking the C BSP (`components/bsp_jc3636k718c`) via FFI.
use board_jc3636k718c::audio::{AudioOut, JcAudio};
use dsp::sine::Sine;
use dsp::voice::Voice;
use esp_idf_svc::sys::{self, esp_err_t, EspError};

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
}

fn main() {
    sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    // Display, touch and knob (the C BSP default set).
    let features = BspFeatures {
        display: true,
        touch: true,
        encoder: true,
        ..Default::default()
    };
    let err = unsafe { bsp_init(&features) };
    EspError::convert(err).expect("bsp_init failed");
    log::info!("rust: bsp ok");

    // 440 Hz test tone on core 1.
    let mut audio = JcAudio::new().expect("audio init failed");
    let mut sine = Sine::new(48_000.0);
    sine.set_params(440.0, 0.25);
    audio
        .start(48_000, Box::new(move |out| sine.process(out)))
        .expect("audio start failed");
    log::info!("rust: audio tone started");

    let psram = unsafe { sys::heap_caps_get_total_size(sys::MALLOC_CAP_SPIRAM) };
    log::info!("rust: psram total = {psram} bytes");

    loop {
        std::thread::sleep(std::time::Duration::from_secs(5));
        log::info!("rust: alive");
    }
}
