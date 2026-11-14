//! NyashkFormant - realtime vocal formant and pitch shifter (VST3).
//!
//! The voice is cut into pitch-synchronous grains (PSOLA). Pitch: the grains are laid out closer
//! together or further apart. Formants: every grain is filtered by the ratio of the voice's
//! spectral envelope moved by the formant factor to the envelope itself, so the vocal-tract
//! resonances move while the harmonics stay where the pitch puts them.

use nih_plug::prelude::*;
use nih_plug_egui::EguiState;
use parking_lot::Mutex;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

pub mod engine;
pub mod formant;
mod editor;

// ------------------------------------------------------------------------------------------
// Enums shown in the host / GUI
// ------------------------------------------------------------------------------------------

/// How the formants are moved (see `formant.rs`).
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum FormantAlgo {
    #[name = "Classic"]
    Classic,
    #[name = "Smooth"]
    Smooth,
    #[name = "Detailed"]
    Detailed,
    #[name = "Vocal (LPC)"]
    Lpc,
    #[name = "Bend"]
    Bend,
}

impl FormantAlgo {
    pub fn name(self) -> &'static str {
        Self::variants()[self.to_index()]
    }

    pub fn about(self) -> [&'static str; 3] {
        match self {
            FormantAlgo::Classic => [
                "Every grain is resampled. Cheap and solid,",
                "the old-school PSOLA sound. Shift moves all",
                "formants evenly; Tilt does not apply.",
            ],
            FormantAlgo::Smooth => [
                "Broad spectral envelope: soft, natural and",
                "forgiving. Good for subtle changes and for",
                "noisy or reverberant recordings.",
            ],
            FormantAlgo::Detailed => [
                "Sharp envelope that rests on the harmonic",
                "peaks: keeps the vowel's character. The",
                "most accurate mode - the default.",
            ],
            FormantAlgo::Lpc => [
                "All-pole vocal-tract model: peaky, focused",
                "\"vocal\" resonances. Strong character for",
                "big shifts and creative voices.",
            ],
            FormantAlgo::Bend => [
                "Non-linear warp around 400 Hz: stretches or",
                "squeezes the spacing of the formants instead",
                "of moving them evenly. Odd and characterful.",
            ],
        }
    }
}

/// The singer's range: only sets how low the pitch detector looks (and how high).
#[derive(Enum, PartialEq, Eq, Clone, Copy, Debug)]
pub enum VoiceRange {
    #[name = "Generic"]
    Generic,
    #[name = "Bass"]
    Bass,
    #[name = "Baritone"]
    Baritone,
    #[name = "Tenor"]
    Tenor,
    #[name = "Alto"]
    Alto,
    #[name = "Soprano"]
    Soprano,
}

impl VoiceRange {
    /// (lowest, highest) fundamental searched for, Hz
    pub fn hz(self) -> (f32, f32) {
        match self {
            VoiceRange::Generic => (60.0, 1100.0),
            VoiceRange::Bass => (55.0, 450.0),
            VoiceRange::Baritone => (70.0, 520.0),
            VoiceRange::Tenor => (90.0, 700.0),
            VoiceRange::Alto => (120.0, 900.0),
            VoiceRange::Soprano => (160.0, 1400.0),
        }
    }
}

// ------------------------------------------------------------------------------------------
// Parameters
// ------------------------------------------------------------------------------------------

fn lin(min: f32, max: f32) -> FloatRange {
    FloatRange::Linear { min, max }
}

fn one_decimal() -> Arc<dyn Fn(f32) -> String + Send + Sync> {
    Arc::new(|v| format!("{:.1}", (v * 10.0).round() / 10.0 + 0.0))
}

fn no_decimals() -> Arc<dyn Fn(f32) -> String + Send + Sync> {
    Arc::new(|v| format!("{:.0}", v.round() + 0.0))
}

#[derive(Params)]
pub struct NyashkFormantParams {
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,

    #[id = "bypass"]
    pub bypass: BoolParam,

    // ---- pitch
    #[id = "transpose"]
    pub transpose: FloatParam,
    #[id = "detune"]
    pub detune: FloatParam,

    // ---- formant
    #[id = "formanton"]
    pub formant_on: BoolParam,
    #[id = "keepformants"]
    pub keep_formants: BoolParam,
    #[id = "fmode"]
    pub fmode: EnumParam<FormantAlgo>,
    #[id = "formant"]
    pub shift: FloatParam,
    #[id = "ftilt"]
    pub tilt: FloatParam,
    #[id = "ffollow"]
    pub follow: FloatParam,

    // ---- colour
    #[id = "colouron"]
    pub colour_on: BoolParam,
    #[id = "air"]
    pub air: FloatParam,
    #[id = "drive"]
    pub drive: FloatParam,

    // ---- output
    #[id = "mix"]
    pub mix: FloatParam,
    #[id = "autogain"]
    pub auto_gain: BoolParam,
    #[id = "out"]
    pub out_gain: FloatParam,
    #[id = "range"]
    pub range: EnumParam<VoiceRange>,
}

impl Default for NyashkFormantParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(900, 560),
            bypass: BoolParam::new("Bypass", false).make_bypass(),

            transpose: FloatParam::new("Transpose", 0.0, lin(-12.0, 12.0))
                .with_unit(" st")
                .with_step_size(1.0)
                .with_value_to_string(no_decimals())
                .with_string_to_value(Arc::new(|s| s.trim().trim_end_matches("st").trim().parse().ok())),
            detune: FloatParam::new("Detune", 0.0, lin(-100.0, 100.0))
                .with_unit(" c")
                .with_step_size(0.1)
                .with_value_to_string(one_decimal())
                .with_string_to_value(Arc::new(|s| s.trim().trim_end_matches('c').trim().parse().ok())),

            formant_on: BoolParam::new("Formant On", true),
            keep_formants: BoolParam::new("Keep Formants", true),
            fmode: EnumParam::new("Formant Algorithm", FormantAlgo::Detailed),
            shift: FloatParam::new("Formant Shift", 0.0, lin(-12.0, 12.0))
                .with_unit(" st")
                .with_step_size(0.1)
                .with_value_to_string(one_decimal())
                .with_string_to_value(Arc::new(|s| s.trim().trim_end_matches("st").trim().parse().ok())),
            tilt: FloatParam::new("Formant Tilt", 0.0, lin(-100.0, 100.0))
                .with_unit(" %")
                .with_step_size(1.0)
                .with_value_to_string(no_decimals())
                .with_string_to_value(Arc::new(|s| s.trim().trim_end_matches('%').trim().parse().ok())),
            follow: FloatParam::new("Formant Follow", 100.0, lin(0.0, 100.0))
                .with_unit(" %")
                .with_step_size(1.0)
                .with_value_to_string(no_decimals())
                .with_string_to_value(Arc::new(|s| s.trim().trim_end_matches('%').trim().parse().ok())),

            colour_on: BoolParam::new("Colour On", true),
            air: FloatParam::new("Air", 0.0, lin(-12.0, 12.0))
                .with_unit(" dB")
                .with_step_size(0.1)
                .with_value_to_string(one_decimal())
                .with_string_to_value(Arc::new(|s| s.trim().trim_end_matches("dB").trim().parse().ok())),
            drive: FloatParam::new("Drive", 0.0, lin(0.0, 100.0))
                .with_unit(" %")
                .with_step_size(1.0)
                .with_value_to_string(no_decimals())
                .with_string_to_value(Arc::new(|s| s.trim().trim_end_matches('%').trim().parse().ok())),

            mix: FloatParam::new("Mix", 100.0, lin(0.0, 100.0))
                .with_unit(" %")
                .with_step_size(1.0)
                .with_value_to_string(no_decimals())
                .with_string_to_value(Arc::new(|s| s.trim().trim_end_matches('%').trim().parse().ok())),
            auto_gain: BoolParam::new("Auto Gain", true),
            out_gain: FloatParam::new("Output", 0.0, lin(-24.0, 12.0))
                .with_unit(" dB")
                .with_step_size(0.1)
                .with_value_to_string(one_decimal())
                .with_string_to_value(Arc::new(|s| s.trim().trim_end_matches("dB").trim().parse().ok())),
            range: EnumParam::new("Voice Range", VoiceRange::Generic),
        }
    }
}

impl NyashkFormantParams {
    /// lowest fundamental the detector looks for
    pub fn lowest_hz(&self) -> f32 {
        self.range.value().hz().0
    }

    pub fn engine_params(&self) -> engine::Params {
        let (min_hz, max_hz) = self.range.value().hz();
        engine::Params {
            bypass: self.bypass.value(),
            scale: 0,
            amount: 0.0, // no pitch correction: the pitch only follows Transpose / Detune
            keep_vibrato: false,
            transpose: self.transpose.value(),
            fine_cents: self.detune.value(),
            formant_on: self.formant_on.value(),
            formant_preserve: self.keep_formants.value(),
            formant_mode: formant::FormantMode::from_index(self.fmode.value().to_index()),
            formant_semis: self.shift.value(),
            formant_tilt: self.tilt.value() / 100.0,
            formant_follow: self.follow.value() / 100.0,
            colour_on: self.colour_on.value(),
            air_db: self.air.value(),
            drive: self.drive.value() / 100.0,
            mix: self.mix.value() / 100.0,
            auto_gain: self.auto_gain.value(),
            out_gain_db: self.out_gain.value(),
            min_hz,
            max_hz,
            voices_on: false,
            vib_on: false,
            midi_mode: false,
            ..engine::Params::default()
        }
    }

    /// Formant factor for the given pitch ratio (what the engine applies), for the display.
    pub fn formant_factor(&self) -> f32 {
        if !self.formant_on.value() {
            return 1.0;
        }
        let r = 2f32.powf((self.transpose.value() + self.detune.value() / 100.0) / 12.0);
        let follow = if self.keep_formants.value() { 0.0 } else { self.follow.value() / 100.0 };
        2f32.powf(self.shift.value() / 12.0) * r.powf(follow)
    }
}

// ------------------------------------------------------------------------------------------
// Shared with the GUI
// ------------------------------------------------------------------------------------------

/// Points of the envelope display (log-spaced 80 Hz .. 12 kHz).
pub const ENV_N: usize = 96;

pub fn env_freq(i: usize) -> f64 {
    80.0 * (12000.0f64 / 80.0).powf(i as f64 / (ENV_N - 1) as f64)
}

pub struct Meters {
    pub in_pk: AtomicF32,
    pub out_pk: AtomicF32,
    pub voiced: AtomicU32,
    pub hz: AtomicF32,
    /// input envelope (dB) at `env_freq(i)`, and whether it is fresh
    pub env: Mutex<([f32; ENV_N], bool)>,
}

impl Default for Meters {
    fn default() -> Self {
        Self {
            in_pk: AtomicF32::new(0.0),
            out_pk: AtomicF32::new(0.0),
            voiced: AtomicU32::new(0),
            hz: AtomicF32::new(0.0),
            env: Mutex::new(([0.0; ENV_N], false)),
        }
    }
}

// ------------------------------------------------------------------------------------------
// Plugin
// ------------------------------------------------------------------------------------------

pub struct NyashkFormant {
    params: Arc<NyashkFormantParams>,
    meters: Arc<Meters>,
    engine: Option<engine::Engine>,
    cur_min_hz: f32,
    env_clock: u32,
    sample_rate: f32,
}

impl Default for NyashkFormant {
    fn default() -> Self {
        Self {
            params: Arc::new(NyashkFormantParams::default()),
            meters: Arc::new(Meters::default()),
            engine: None,
            cur_min_hz: 60.0,
            env_clock: 0,
            sample_rate: 48000.0,
        }
    }
}

impl Plugin for NyashkFormant {
    const NAME: &'static str = "NyashkFormant";
    const VENDOR: &'static str = "Nyashk";
    const URL: &'static str = "https://github.com/xslimakx/NyashkFormant";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
    ];

    const SAMPLE_ACCURATE_AUTOMATION: bool = false;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        static HOOK: std::sync::Once = std::sync::Once::new();
        HOOK.call_once(|| {
            let prev = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                baseview::nylog(&format!("PANIC: {info}\n{}", std::backtrace::Backtrace::force_capture()));
                prev(info);
            }));
        });
        editor::create(self.params.clone(), self.meters.clone())
    }

    fn initialize(&mut self, _layout: &AudioIOLayout, config: &BufferConfig, context: &mut impl InitContext<Self>) -> bool {
        self.cur_min_hz = self.params.lowest_hz();
        self.sample_rate = config.sample_rate;
        let eng = engine::Engine::new(config.sample_rate as f64, self.cur_min_hz);
        context.set_latency_samples(eng.latency_samples());
        self.engine = Some(eng);
        true
    }

    fn reset(&mut self) {
        if let Some(e) = self.engine.as_mut() {
            e.reset();
        }
    }

    fn process(&mut self, buffer: &mut Buffer, _aux: &mut AuxiliaryBuffers, context: &mut impl ProcessContext<Self>) -> ProcessStatus {
        demo_voice(buffer);
        let Some(eng) = self.engine.as_mut() else {
            return ProcessStatus::Normal;
        };

        let ep = self.params.engine_params();
        if (ep.min_hz - self.cur_min_hz).abs() > 0.5 {
            self.cur_min_hz = ep.min_hz;
            eng.set_lowest_hz(ep.min_hz);
            context.set_latency_samples(eng.latency_samples());
        }
        eng.set_params(&ep);

        let (mut ipk, mut opk) = (0.0f32, 0.0f32);
        for mut channels in buffer.iter_samples() {
            let nch = channels.len().min(engine::MAX_CH);
            let mut inp = [0.0f32; engine::MAX_CH];
            for (i, s) in channels.iter_mut().enumerate().take(nch) {
                inp[i] = *s;
                ipk = ipk.max(s.abs());
            }
            let out = eng.process_frame(inp, nch);
            for (i, s) in channels.iter_mut().enumerate().take(nch) {
                *s = out[i];
                opk = opk.max(s.abs());
            }
        }

        // publish for the GUI
        let m = &eng.meters;
        self.meters.voiced.store(m.voiced as u32, Ordering::Relaxed);
        self.meters.hz.store(m.hz, Ordering::Relaxed);
        self.meters.in_pk.store(self.meters.in_pk.load(Ordering::Relaxed).max(ipk), Ordering::Relaxed);
        self.meters.out_pk.store(self.meters.out_pk.load(Ordering::Relaxed).max(opk), Ordering::Relaxed);
        self.env_clock += buffer.samples() as u32;
        if self.env_clock as f32 > 0.03 * self.sample_rate {
            if let Some(mut g) = self.meters.env.try_lock() {
                self.env_clock = 0;
                let mut ok = m.voiced;
                for i in 0..ENV_N {
                    match eng.envelope_db(env_freq(i)) {
                        Some(v) => g.0[i] = v as f32,
                        None => ok = false,
                    }
                }
                g.1 = ok;
            }
        }

        ProcessStatus::Normal
    }
}

impl Vst3Plugin for NyashkFormant {
    const VST3_CLASS_ID: [u8; 16] = *b"NyashkFormantVST";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Fx, Vst3SubCategory::PitchShift];
}

nih_export_vst3!(NyashkFormant);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_values_parse() {
        let p = NyashkFormantParams::default();
        let n = p.shift.string_to_normalized_value("3.5").unwrap();
        assert!((p.shift.preview_plain(n) - 3.5).abs() < 0.01);
        let n = p.shift.string_to_normalized_value("-4 st").unwrap();
        assert!((p.shift.preview_plain(n) + 4.0).abs() < 0.01);
        let n = p.transpose.string_to_normalized_value("7").unwrap();
        assert_eq!(p.transpose.preview_plain(n), 7.0);
        let n = p.detune.string_to_normalized_value("-15 c").unwrap();
        assert!((p.detune.preview_plain(n) + 15.0).abs() < 0.01);
        let n = p.tilt.string_to_normalized_value("40%").unwrap();
        assert!((p.tilt.preview_plain(n) - 40.0).abs() < 0.01);
        let n = p.out_gain.string_to_normalized_value("-6 dB").unwrap();
        assert!((p.out_gain.preview_plain(n) + 6.0).abs() < 0.01);
    }

    #[test]
    fn defaults_change_nothing() {
        let p = NyashkFormantParams::default();
        let e = p.engine_params();
        assert_eq!(e.amount, 0.0);
        assert_eq!(e.transpose, 0.0);
        assert_eq!(e.formant_semis, 0.0);
        assert!(e.formant_preserve && e.auto_gain && !e.voices_on && !e.vib_on);
        assert!((p.formant_factor() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn follow_only_without_keep_formants() {
        let p = NyashkFormantParams::default();
        // keep formants (default): transposing does not move the formants
        assert!((p.formant_factor() - 1.0).abs() < 1e-6);
        let e = p.engine_params();
        assert!(e.formant_preserve);
    }
}

/// Developer helper for screenshots: with `NYASHK_DEMO=1` the standalone build sings a short melody
/// (an "ah" vowel with vibrato) instead of using the input.
fn demo_voice(buffer: &mut Buffer) {
    use std::sync::Mutex;
    static ST: Mutex<Option<(bool, u64, f64, [f64; 6])>> = Mutex::new(None);
    let mut g = ST.lock().unwrap();
    let st = g.get_or_insert_with(|| (std::env::var("NYASHK_DEMO").is_ok(), 0, 0.0, [0.0; 6]));
    if !st.0 {
        return;
    }
    let fs = 48000.0f64;
    let melody = [57.0f64, 60.0, 62.0, 60.0, 64.0, 62.0, 60.0, 57.0];
    let formants = [(750.0, 0.985), (1200.0, 0.98), (2600.0, 0.975)];
    for mut frame in buffer.iter_samples() {
        let t = st.1 as f64 / fs;
        st.1 += 1;
        let k = (t / 0.6) as usize;
        let gap = (t / 0.6).fract() > 0.9;
        let m = melody[k % melody.len()] + 0.2 * (2.0 * std::f64::consts::PI * 5.5 * t).sin();
        let f0 = 440.0 * 2f64.powf((m - 69.0) / 12.0);
        st.2 = (st.2 + f0 / fs) % 1.0;
        let src = if st.2 < 0.05 { 1.0 } else { -0.05 };
        let mut sum = 0.0;
        for (j, (f, r)) in formants.iter().enumerate() {
            let th = 2.0 * std::f64::consts::PI * f / fs;
            let y = src * 0.1 + 2.0 * r * th.cos() * st.3[2 * j] - r * r * st.3[2 * j + 1];
            st.3[2 * j + 1] = st.3[2 * j];
            st.3[2 * j] = y;
            sum += y / (j + 1) as f64;
        }
        let v = if gap { 0.0 } else { (sum * 0.08) as f32 };
        for s in frame.iter_mut() {
            *s = v;
        }
    }
}
