//! The device side: panel over `bsp_display_draw_rect`, and `JcBoard`.
use crate::audio::JcAudio;
use crate::display::{FbDisplay, Panel, Rect, STRIP_PIXELS};
use crate::ffi::{bsp_display_draw_rect, bsp_display_raw_begin, bsp_display_raw_max_pixels};
use crate::input::JcInput;
use esp_idf_svc::sys::{self, esp_err_t, ESP_OK};
use os::{AudioError, AudioOut, Board, Display, InputEvent};

/// ST77916 behind the C BSP. The strip buffer is internal, DMA-capable RAM
/// (the framebuffer itself lives in PSRAM and is only read for dirty rows).
pub struct BspPanel {
    buf: *mut u16,
}

impl BspPanel {
    /// Takes the panel from LVGL (`bsp_display_raw_begin`) and allocates the strip buffer.
    pub fn new() -> Result<BspPanel, esp_err_t> {
        let max = unsafe { bsp_display_raw_max_pixels() } as usize;
        assert!(STRIP_PIXELS <= max, "strip ({STRIP_PIXELS} px) exceeds the BSP transfer limit ({max} px)");
        let buf = unsafe {
            sys::heap_caps_malloc(STRIP_PIXELS * 2, sys::MALLOC_CAP_DMA | sys::MALLOC_CAP_INTERNAL | sys::MALLOC_CAP_8BIT)
        } as *mut u16;
        if buf.is_null() {
            return Err(sys::ESP_ERR_NO_MEM);
        }
        let err = unsafe { bsp_display_raw_begin() };
        if err != ESP_OK {
            unsafe { sys::heap_caps_free(buf.cast()) };
            return Err(err);
        }
        Ok(BspPanel { buf })
    }
}

impl Drop for BspPanel {
    fn drop(&mut self) {
        unsafe { sys::heap_caps_free(self.buf.cast()) };
    }
}

impl Panel for BspPanel {
    fn strip_buf(&mut self) -> &mut [u16] {
        // SAFETY: `buf` is a live allocation of STRIP_PIXELS u16, uniquely owned by self.
        unsafe { core::slice::from_raw_parts_mut(self.buf, STRIP_PIXELS) }
    }
    fn send(&mut self, r: Rect) -> Result<(), i32> {
        let err = unsafe { bsp_display_draw_rect(r.x as i32, r.y as i32, r.w as i32, r.h as i32, self.buf) };
        if err == ESP_OK { Ok(()) } else { Err(err) }
    }
}

pub type JcDisplay = FbDisplay<BspPanel>;

pub struct JcBoard {
    audio: JcAudio,
    display: JcDisplay,
    input: JcInput,
}

impl JcBoard {
    /// Call after `bsp_init` (display, touch and encoder enabled).
    pub fn new() -> Result<JcBoard, AudioError> {
        let panel = BspPanel::new().map_err(AudioError::Backend)?;
        Ok(JcBoard { audio: JcAudio::new()?, display: FbDisplay::new(panel), input: JcInput::new() })
    }
    pub fn display_mut(&mut self) -> &mut JcDisplay {
        &mut self.display
    }
}

impl Board for JcBoard {
    fn poll_input(&mut self) -> Option<InputEvent> {
        self.input.poll()
    }
    fn audio_out(&mut self) -> &mut dyn AudioOut {
        &mut self.audio
    }
    fn display(&mut self) -> &mut dyn Display {
        &mut self.display
    }
}
