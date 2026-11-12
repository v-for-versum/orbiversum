//! Raw bindings to the C BSP audio output (`components/bsp_jc3636k718c`).
use esp_idf_svc::sys::esp_err_t;

#[repr(C)]
pub struct BspAudioOutConfig {
    pub sample_rate: u32,
    pub bits: u8,
    pub stereo: bool,
}

unsafe extern "C" {
    pub fn bsp_audio_out_init(cfg: *const BspAudioOutConfig) -> esp_err_t;
    pub fn bsp_audio_out_write(
        data: *const core::ffi::c_void,
        bytes: usize,
        written: *mut usize,
        timeout_ms: u32,
    ) -> esp_err_t;
}
