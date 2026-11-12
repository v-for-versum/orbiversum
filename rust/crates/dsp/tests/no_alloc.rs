use dsp::starlids::Starlids;
use dsp::voice::Voice;
use std::alloc::{GlobalAlloc, Layout, System};

struct Counting;
thread_local! {
    /// Allocations made by the current thread (other test threads and the harness must not count).
    static ALLOCS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.with(|a| a.set(a.get() + 1));
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        ALLOCS.with(|a| a.set(a.get() + 1));
        System.realloc(p, l, n)
    }
}

/// The allocation counter is global: tests that read it must not overlap.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[global_allocator]
static A: Counting = Counting;

#[test]
fn starlids_no_alloc_in_process() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut v = Starlids::new(1, 48_000.0);
    v.set_params(110.0, 0.8);
    let mut buf = [[0.0f32; 2]; 256];
    v.process(&mut buf);
    let before = ALLOCS.with(|a| a.get());
    for _ in 0..200 {
        v.process(&mut buf);
    }
    v.set_params(220.0, 0.5);
    v.process(&mut buf);
    assert_eq!(ALLOCS.with(|a| a.get()) - before, 0);
}

#[test]
fn eno_no_alloc_in_process() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut v = dsp::eno::Eno::new(1, 48_000.0);
    v.set_params(330.0, 0.8);
    let mut buf = [[0.0f32; 2]; 256];
    v.process(&mut buf);
    let before = ALLOCS.with(|a| a.get());
    // 4 s of blocks after warm-up: covers the first plane switch (~3 s)
    for _ in 0..(4 * 48_000 / 256 + 1) {
        v.process(&mut buf);
    }
    v.set_params(220.0, 0.5);
    v.process(&mut buf);
    assert_eq!(ALLOCS.with(|a| a.get()) - before, 0);
}
