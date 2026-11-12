//! Audio half: a `dsp::Voice` driven by the shared `os::Params`.
use alloc::sync::Arc;
use dsp::voice::Voice;
use os::{Applet, AppletInfo, Params};

/// Start/reset policy: applets do NOT own hz/amp. All applets share one
/// `Params` (the UI half writes it), so the voice is never reset to a private
/// default. `start` only pushes the *current* Params into the voice, so the
/// first block already runs at the live values: no jump between applets.
/// The click-free hand-over itself is the dispatcher's fade-in/out.
pub struct VoiceApplet<V: Voice> {
    name: &'static str,
    voice: V,
    params: Arc<Params>,
}

impl<V: Voice> VoiceApplet<V> {
    pub fn new(name: &'static str, voice: V, params: Arc<Params>) -> VoiceApplet<V> {
        VoiceApplet { name, voice, params }
    }

    fn sync(&mut self) {
        self.voice.set_params(self.params.hz(), self.params.amp());
    }
}

impl<V: Voice> Applet for VoiceApplet<V> {
    fn info(&self) -> AppletInfo {
        AppletInfo { name: self.name }
    }

    fn start(&mut self) {
        self.sync();
    }

    fn audio(&mut self, out: &mut [[f32; 2]]) {
        self.sync();
        self.voice.process(out);
    }

    fn stop(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use dsp::sine::Sine;
    use os::Applet;

    fn peak(a: &mut impl Applet) -> f32 {
        let mut buf = [[0.0f32; 2]; 480];
        a.audio(&mut buf);
        buf.iter().map(|f| f[0].abs()).fold(0.0, f32::max)
    }

    #[test]
    fn audio_reflects_params() {
        let p = Arc::new(Params::new(440.0, 0.5));
        let mut a = VoiceApplet::new("Sine", Sine::new(48_000.0), p.clone());
        a.start();
        let loud = peak(&mut a);
        assert!(loud > 0.3, "{loud}");
        p.set_amp(0.1);
        let quiet = peak(&mut a);
        assert!(quiet > 0.0 && quiet < 0.15, "{quiet}");
        p.set_amp(0.0);
        assert_eq!(peak(&mut a), 0.0);
        // hz: 4800 Hz at 48 kHz has period 10 samples -> count zero crossings in 480 frames.
        p.set_amp(0.5);
        p.set_hz(4800.0);
        let mut buf = [[0.0f32; 2]; 480];
        a.audio(&mut buf);
        let zc = buf.windows(2).filter(|w| (w[0][0] < 0.0) != (w[1][0] < 0.0)).count();
        assert!(zc > 80, "{zc}");
    }

    #[test]
    fn start_syncs_to_current_params_without_step() {
        let p = Arc::new(Params::new(220.0, 0.5));
        let mut a = VoiceApplet::new("Sine", Sine::new(48_000.0), p.clone());
        // Params change before start (another applet was running): first block already uses them.
        p.set_amp(0.25);
        a.start();
        let pk = peak(&mut a);
        assert!(pk > 0.1 && pk <= 0.25 + 1e-3, "{pk}");
    }
}
