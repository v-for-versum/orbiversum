//! Spike: Rust firmware linking the C BSP (`components/bsp_jc3636k718c`) via FFI.
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

#[repr(C)]
pub struct BspAudioOutConfig {
    pub sample_rate: u32,
    pub bits: u8,
    pub stereo: bool,
}

unsafe extern "C" {
    fn bsp_init(features: *const BspFeatures) -> esp_err_t;
    fn bsp_audio_out_init(cfg: *const BspAudioOutConfig) -> esp_err_t;
    fn bsp_audio_out_write(
        data: *const core::ffi::c_void,
        bytes: usize,
        written: *mut usize,
        timeout_ms: u32,
    ) -> esp_err_t;
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

    // Reference the audio entry points so the linker proves they resolve.
    std::hint::black_box((
        bsp_audio_out_init as unsafe extern "C" fn(_) -> _,
        bsp_audio_out_write as unsafe extern "C" fn(_, _, _, _) -> _,
    ));

    let psram = unsafe { sys::heap_caps_get_total_size(sys::MALLOC_CAP_SPIRAM) };
    log::info!("rust: psram total = {psram} bytes");

    loop {
        std::thread::sleep(std::time::Duration::from_secs(5));
        log::info!("rust: alive");
    }
}
