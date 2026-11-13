//! Starlids and Eno applets: an audio half (`VoiceApplet`) and a UI half
//! (`ParamUi`) that share only `os::Params`.
#![cfg_attr(not(test), no_std)]
extern crate alloc;

mod ui;
mod voice_applet;

pub use ui::{ParamUi, AMP_STEP, MAX_HZ, MIN_HZ};
pub use voice_applet::VoiceApplet;

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use dsp::{eno::Eno, starlids::Starlids};
use os::{Applet, AppletUi, Params};

/// Starlids and Eno as (audio half, UI half) pairs sharing `params`.
/// Task 10: pass the audio halves to `Dispatcher::with_params(.., params)`
/// and keep the UI halves.
pub fn all(seed: u64, sr: f32, params: Arc<Params>) -> Vec<(Box<dyn Applet + Send>, Box<dyn AppletUi>)> {
    let pair = |name, a: Box<dyn Applet + Send>| -> (Box<dyn Applet + Send>, Box<dyn AppletUi>) {
        (a, Box::new(ParamUi::new(name, params.clone())))
    };
    alloc::vec![
        pair("Starlids", Box::new(VoiceApplet::new("Starlids", Starlids::new(seed, sr), params.clone()))),
        pair("Eno", Box::new(VoiceApplet::new("Eno", Eno::new(seed, sr), params.clone()))),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_returns_matching_pairs() {
        let v = all(1, 48_000.0, Arc::new(Params::default()));
        let names: Vec<_> = v.iter().map(|(a, u)| (a.info().name, u.info().name)).collect();
        assert_eq!(names, [("Starlids", "Starlids"), ("Eno", "Eno")]);
    }
}
