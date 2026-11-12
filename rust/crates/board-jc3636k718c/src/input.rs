//! Input: encoder steps and touch state -> `os::InputEvent`.
//!
//! The board has NO encoder push contact (the BSP says so), so `Press` is
//! produced by holding a finger on the touch screen for `LONG_PRESS_MS`.
//! A shorter touch is a `Tap` at the point where it landed. `InputFilter` is
//! pure (host-testable); `JcInput` (device only) feeds it from the C BSP.
use alloc::collections::VecDeque;
use os::InputEvent;

/// Hold time that turns a touch into `Press`.
pub const LONG_PRESS_MS: u32 = 600;

/// One snapshot of the raw inputs.
#[derive(Debug, Clone, Copy)]
pub struct RawInput {
    /// Cumulative encoder steps (`bsp_encoder_get_steps`).
    pub steps: i32,
    /// Fingers landed so far (`bsp_touch_contact`).
    pub contact: u32,
    /// Finger currently down.
    pub pressed: bool,
    /// Last known touch position.
    pub x: i16,
    pub y: i16,
}

#[derive(Debug, Clone, Copy)]
struct Down {
    since_ms: u32,
    x: i16,
    y: i16,
    fired: bool,
}

pub struct InputFilter {
    steps: i32,
    contact: u32,
    down: Option<Down>,
    out: VecDeque<InputEvent>,
}

impl InputFilter {
    /// `first` is the state at start-up (so old counts do not become events).
    pub fn new(first: RawInput) -> InputFilter {
        InputFilter { steps: first.steps, contact: first.contact, down: None, out: VecDeque::new() }
    }

    /// Feed one snapshot taken at `now_ms`; events queue up for `next`.
    pub fn update(&mut self, now_ms: u32, r: RawInput) {
        let d = r.steps.wrapping_sub(self.steps);
        self.steps = r.steps;
        if d != 0 {
            self.out.push_back(InputEvent::Knob(d));
        }

        let landed = r.contact != self.contact;
        self.contact = r.contact;

        // A finger that went up and down again between two snapshots ends the
        // previous touch.
        if landed {
            if let Some(dn) = self.down.take() {
                if !dn.fired {
                    self.out.push_back(InputEvent::Tap { x: dn.x, y: dn.y });
                }
            }
            if !r.pressed {
                // Landed and lifted between two snapshots: a quick tap.
                self.out.push_back(InputEvent::Tap { x: r.x, y: r.y });
            } else {
                self.down = Some(Down { since_ms: now_ms, x: r.x, y: r.y, fired: false });
            }
            return;
        }

        match (&mut self.down, r.pressed) {
            (Some(dn), true) => {
                if !dn.fired && now_ms.wrapping_sub(dn.since_ms) >= LONG_PRESS_MS {
                    dn.fired = true;
                    self.out.push_back(InputEvent::Press);
                }
            }
            (Some(_), false) => {
                let dn = self.down.take().unwrap();
                if !dn.fired {
                    self.out.push_back(InputEvent::Tap { x: dn.x, y: dn.y });
                }
            }
            (None, _) => {}
        }
    }

    pub fn next(&mut self) -> Option<InputEvent> {
        self.out.pop_front()
    }
}

/// Builds one snapshot from the three BSP reads. `contact` MUST be read before
/// `live`: a finger landing between the two reads then shows up as "pressed with
/// the old contact count" (picked up at the next poll with its new position),
/// instead of "released at the old position with a new contact count", which the
/// filter would report as a quick Tap at stale coordinates.
pub fn sample(
    steps: impl FnOnce() -> i32,
    live: impl FnOnce() -> (bool, i16, i16),
    contact: impl FnOnce() -> u32,
) -> RawInput {
    let steps = steps();
    let contact = contact();
    let (pressed, x, y) = live();
    RawInput { steps, contact, pressed, x, y }
}

#[cfg(target_os = "espidf")]
pub use device::JcInput;

#[cfg(target_os = "espidf")]
mod device {
    use super::*;
    use crate::ffi::{bsp_encoder_get_steps, bsp_touch_contact, bsp_touch_get_live};
    use esp_idf_svc::sys;

    fn now_ms() -> u32 {
        (unsafe { sys::esp_timer_get_time() } / 1000) as u32
    }

    fn read() -> RawInput {
        sample(
            || unsafe { bsp_encoder_get_steps() },
            || {
                let (mut x, mut y) = (0u16, 0u16);
                let pressed = unsafe { bsp_touch_get_live(&mut x, &mut y) };
                (pressed, x as i16, y as i16)
            },
            || unsafe { bsp_touch_contact() },
        )
    }

    /// Polls the C BSP; call `poll` every UI tick.
    pub struct JcInput {
        filter: InputFilter,
    }

    impl JcInput {
        pub fn new() -> JcInput {
            JcInput { filter: InputFilter::new(read()) }
        }
        pub fn poll(&mut self) -> Option<InputEvent> {
            if let Some(e) = self.filter.next() {
                return Some(e);
            }
            self.filter.update(now_ms(), read());
            self.filter.next()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> RawInput {
        RawInput { steps: 0, contact: 0, pressed: false, x: 0, y: 0 }
    }
    fn drain(f: &mut InputFilter) -> alloc::vec::Vec<InputEvent> {
        core::iter::from_fn(|| f.next()).collect()
    }

    #[test]
    fn touch_landing_between_the_two_reads_is_not_a_stale_tap() {
        use core::cell::Cell;
        // World: finger absent at (0,0), contact count 0. It lands at (10,10)
        // right after the first touch read of the sample, whichever that is.
        let reads = Cell::new(0u32);
        let mut f = InputFilter::new(idle());
        let raw = sample(
            || 0,
            || {
                let landed = reads.get() >= 1;
                reads.set(reads.get() + 1);
                if landed { (true, 10, 10) } else { (false, 0, 0) }
            },
            || {
                let landed = reads.get() >= 1;
                reads.set(reads.get() + 1);
                if landed { 1 } else { 0 }
            },
        );
        f.update(0, raw);
        assert!(drain(&mut f).is_empty(), "no event from a half-seen touch: {raw:?}");
        // The next poll sees the whole touch: contact 1, pressed at (10,10).
        f.update(16, RawInput { contact: 1, pressed: true, x: 10, y: 10, ..idle() });
        f.update(40, RawInput { contact: 1, pressed: false, x: 10, y: 10, ..idle() });
        assert_eq!(drain(&mut f), [InputEvent::Tap { x: 10, y: 10 }]);
    }

    #[test]
    fn knob_reports_step_difference_only() {
        let mut f = InputFilter::new(RawInput { steps: 100, ..idle() });
        f.update(0, RawInput { steps: 100, ..idle() });
        assert!(drain(&mut f).is_empty());
        f.update(10, RawInput { steps: 103, ..idle() });
        f.update(20, RawInput { steps: 101, ..idle() });
        assert_eq!(drain(&mut f), [InputEvent::Knob(3), InputEvent::Knob(-2)]);
    }

    #[test]
    fn short_touch_is_a_tap_at_landing_point() {
        let mut f = InputFilter::new(idle());
        f.update(0, RawInput { contact: 1, pressed: true, x: 10, y: 20, ..idle() });
        f.update(50, RawInput { contact: 1, pressed: true, x: 12, y: 22, ..idle() });
        assert!(drain(&mut f).is_empty());
        f.update(100, RawInput { contact: 1, pressed: false, x: 12, y: 22, ..idle() });
        assert_eq!(drain(&mut f), [InputEvent::Tap { x: 10, y: 20 }]);
    }

    #[test]
    fn touch_between_two_polls_is_still_a_tap() {
        let mut f = InputFilter::new(idle());
        f.update(0, RawInput { contact: 1, pressed: false, x: 7, y: 8, ..idle() });
        assert_eq!(drain(&mut f), [InputEvent::Tap { x: 7, y: 8 }]);
    }

    #[test]
    fn long_hold_is_one_press_and_no_tap() {
        let mut f = InputFilter::new(idle());
        let down = RawInput { contact: 1, pressed: true, x: 1, y: 2, ..idle() };
        f.update(0, down);
        f.update(LONG_PRESS_MS - 1, down);
        assert!(drain(&mut f).is_empty());
        f.update(LONG_PRESS_MS, down);
        f.update(LONG_PRESS_MS + 500, down);
        assert_eq!(drain(&mut f), [InputEvent::Press]);
        f.update(LONG_PRESS_MS + 600, RawInput { pressed: false, ..down });
        assert!(drain(&mut f).is_empty());
    }

    #[test]
    fn two_quick_touches_between_polls_while_down_give_two_taps() {
        let mut f = InputFilter::new(idle());
        f.update(0, RawInput { contact: 1, pressed: true, x: 1, y: 1, ..idle() });
        f.update(30, RawInput { contact: 2, pressed: false, x: 5, y: 5, ..idle() });
        assert_eq!(drain(&mut f), [InputEvent::Tap { x: 1, y: 1 }, InputEvent::Tap { x: 5, y: 5 }]);
    }
}
