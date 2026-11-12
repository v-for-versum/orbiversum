//! Raw bindings to the C BSP (`components/bsp_jc3636k718c`).
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

    pub fn bsp_display_raw_begin() -> esp_err_t;
    pub fn bsp_display_draw_rect(x: i32, y: i32, w: i32, h: i32, rgb565: *const u16) -> esp_err_t;
    pub fn bsp_display_raw_max_pixels() -> u32;

    pub fn bsp_encoder_get_steps() -> i32;
    pub fn bsp_touch_contact() -> u32;
    pub fn bsp_touch_get_live(x: *mut u16, y: *mut u16) -> bool;
}
