//! End-to-end checks of the shifter: formants move by the asked factor and the pitch stays,
//! Transpose keeps the formants, Auto Gain keeps the level, and nothing clicks.

use nyashkformant::engine::{Engine, Params};
use nyashkformant::formant::FormantMode;
use std::f64::consts::PI;

const FS: f64 = 48000.0;

/// An "ah" sung at `midi` with a light vibrato: impulse train through three resonators.
fn vowel(midi: f64, secs: f64, formants: &[(f64, f64)]) -> Vec<f32> {
    let n = (secs * FS) as usize;
    let mut ph = 0.0;
    let mut x = vec![0.0f64; n];
    for (i, v) in x.iter_mut().enumerate() {
        let t = i as f64 / FS;
        let m = midi + 0.15 * (2.0 * PI * 5.5 * t).sin();
        let f0 = 440.0 * 2f64.powf((m - 69.0) / 12.0);
        ph += f0 / FS;
        if ph >= 1.0 {
            ph -= 1.0;
            *v = 1.0;
        }
    }
    let mut y = vec![0.0f64; n];
    for &(fc, bw) in formants {
        let r = (-PI * bw / FS).exp();
        let th = 2.0 * PI * fc / FS;
        let (mut y1, mut y2) = (0.0, 0.0);
        for i in 0..n {
            let s = (1.0 - r) * x[i] + 2.0 * r * th.cos() * y1 - r * r * y2;
            y2 = y1;
            y1 = s;
            y[i] += s * fc.sqrt() / 30.0;
        }
    }
    let pk = y.iter().fold(0.0f64, |a, v| a.max(v.abs()));
    y.into_iter().map(|v| (0.5 * v / pk) as f32).collect()
}

fn run(x: &[f32], p: &Params) -> Vec<f32> {
    let mut e = Engine::new(FS, p.min_hz);
    e.set_params(p);
    let lat = e.latency_samples() as usize;
    let mut out = Vec::with_capacity(x.len() + lat);
    for &s in x.iter().chain(std::iter::repeat(&0.0).take(lat)) {
        out.push(e.process_frame([s, s], 1)[0]);
    }
    out[lat..].to_vec()
}

fn params(mode: FormantMode, shift: f32, transpose: f32) -> Params {
    Params {
        scale: 0,
        amount: 0.0,
        keep_vibrato: false,
        formant_mode: mode,
        formant_semis: shift,
        transpose,
        formant_preserve: true,
        auto_gain: true,
        voices_on: false,
        vib_on: false,
        min_hz: 60.0,
        max_hz: 1100.0,
        ..Params::default()
    }
}

/// Magnitude spectrum (Hann, 16384) of the middle second, smoothed by `smooth_hz`.
fn spectrum(x: &[f32], smooth_hz: f64) -> Vec<f64> {
    let n = 16384;
    let start = x.len() / 2 - n / 2;
    let mut re: Vec<f64> = (0..n).map(|i| x[start + i] as f64 * 0.5 * (1.0 - (2.0 * PI * i as f64 / n as f64).cos())).collect();
    let mut im = vec![0.0; n];
    let fft = nyashkformant::formant::Fft::new(n);
    fft.forward(&mut re, &mut im);
    let mag: Vec<f64> = (0..n / 2).map(|k| re[k].hypot(im[k])).collect();
    let w = (smooth_hz * n as f64 / FS) as usize;
    (0..n / 2).map(|k| mag[k.saturating_sub(w)..(k + w).min(n / 2)].iter().sum::<f64>()).collect()
}

/// How far (semitones) the spectral envelope of `y` sits from that of `x`: the shift on a log
/// frequency axis (200 Hz .. 4 kHz) that lines the two smoothed spectra (dB) up best.
fn envelope_shift(x: &[f32], y: &[f32]) -> f64 {
    let (sx, sy) = (spectrum(x, 150.0), spectrum(y, 150.0));
    let n = sx.len() * 2;
    let at = |s: &Vec<f64>, f: f64| {
        let pos = f * n as f64 / FS;
        let i = pos as usize;
        let fr = pos - i as f64;
        20.0 * (s[i] * (1.0 - fr) + s[i + 1] * fr + 1e-9).log10()
    };
    let pts = 400;
    let lx: Vec<f64> = (0..pts).map(|i| at(&sx, 200.0 * 20f64.powf(i as f64 / pts as f64))).collect();
    let (mut best, mut bs) = (f64::MIN, 0.0);
    for k in -160i32..=160 {
        let st = k as f64 * 0.05;
        let ly: Vec<f64> = (0..pts).map(|i| at(&sy, 200.0 * 20f64.powf(i as f64 / pts as f64) * 2f64.powf(st / 12.0))).collect();
        let (mx, my) = (lx.iter().sum::<f64>() / pts as f64, ly.iter().sum::<f64>() / pts as f64);
        let c: f64 = lx.iter().zip(&ly).map(|(a, b)| (a - mx) * (b - my)).sum::<f64>()
            / (lx.iter().map(|a| (a - mx).powi(2)).sum::<f64>() * ly.iter().map(|b| (b - my).powi(2)).sum::<f64>()).sqrt();
        if c > best {
            best = c;
            bs = st;
        }
    }
    bs
}

fn peak_hz(spec: &[f64], lo: f64, hi: f64) -> f64 {
    let n = spec.len() * 2;
    let (a, b) = ((lo * n as f64 / FS) as usize, (hi * n as f64 / FS) as usize);
    let k = (a..b).max_by(|i, j| spec[*i].partial_cmp(&spec[*j]).unwrap()).unwrap();
    k as f64 * FS / n as f64
}

/// fundamental from the strongest autocorrelation peak in 60..1000 Hz
fn f0(x: &[f32]) -> f64 {
    let s = &x[x.len() / 2..x.len() / 2 + 4096];
    let (mut best, mut bl) = (f64::MIN, 0);
    for lag in (FS / 1000.0) as usize..(FS / 60.0) as usize {
        let c: f64 = (0..s.len() - lag).map(|i| s[i] as f64 * s[i + lag] as f64).sum::<f64>() / (s.len() - lag) as f64;
        if c > best {
            best = c;
            bl = lag;
        }
    }
    FS / bl as f64
}

fn rms(x: &[f32]) -> f64 {
    let s = &x[x.len() / 4..3 * x.len() / 4];
    (s.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / s.len() as f64).sqrt()
}

/// Biggest single-sample jump relative to the typical one around it (a click detector).
fn worst_jump(x: &[f32]) -> f64 {
    let d: Vec<f64> = x.windows(2).map(|w| (w[1] - w[0]).abs() as f64).collect();
    let k = 480;
    let mut worst = 0.0f64;
    let mut acc: f64 = d[..k].iter().sum();
    for i in k..d.len() {
        acc += d[i] - d[i - k];
        let local = acc / k as f64 + 1e-5;
        if d[i] > 0.02 {
            worst = worst.max(d[i] / local);
        }
    }
    worst
}

const AH: [(f64, f64); 3] = [(700.0, 90.0), (1150.0, 110.0), (2700.0, 160.0)];

#[test]
fn formant_shift_moves_the_formant_and_keeps_the_pitch() {
    for midi in [45.0, 57.0] {
        let x = vowel(midi, 3.0, &AH);
        let p_in = f0(&x);
        for mode in [FormantMode::Classic, FormantMode::Smooth, FormantMode::Detailed, FormantMode::Lpc] {
            for st in [-4.0f32, 4.0] {
                let y = run(&x, &params(mode, st, 0.0));
                let got = envelope_shift(&x, &y);
                let p_out = f0(&y);
                eprintln!("midi {midi} {mode:?} {st:+}: envelope moved {got:+.2} st, pitch {p_in:.1} -> {p_out:.1}");
                // the all-pole (LPC) model is a coloured approximation: it moves at least half way
                let tol = if mode == FormantMode::Lpc { 0.5 } else { 0.25 };
                assert!((got - st as f64).abs() < tol * st.abs() as f64, "{mode:?} {st}: envelope moved {got:.2} st");
                assert!((p_out / p_in - 1.0).abs() < 0.01, "{mode:?} {st}: pitch {p_in:.1} -> {p_out:.1}");
            }
        }
    }
}

#[test]
fn transpose_keeps_the_formants() {
    let x = vowel(50.0, 3.0, &AH);
    let base = peak_hz(&spectrum(&x, 120.0), 450.0, 950.0);
    let p_in = f0(&x);
    for tr in [-7.0f32, 5.0] {
        let y = run(&x, &params(FormantMode::Detailed, 0.0, tr));
        let got = peak_hz(&spectrum(&y, 160.0), 450.0, 950.0);
        let p_out = f0(&y);
        let want_p = p_in * 2f64.powf(tr as f64 / 12.0);
        eprintln!("transpose {tr:+}: pitch {p_in:.1} -> {p_out:.1} (want {want_p:.1}), F1 {base:.0} -> {got:.0}");
        assert!((p_out / want_p - 1.0).abs() < 0.015, "pitch {p_out:.1}, want {want_p:.1}");
        assert!((got / base - 1.0).abs() < 0.12, "F1 moved {base:.0} -> {got:.0}");
    }
}

#[test]
fn auto_gain_keeps_the_level() {
    let x = vowel(55.0, 3.0, &AH);
    let l_in = rms(&x);
    for (tr, sh) in [(7.0f32, 0.0f32), (12.0, 0.0), (-7.0, 0.0), (0.0, 6.0), (0.0, -6.0)] {
        let mut p = params(FormantMode::Detailed, sh, tr);
        let y = run(&x, &p);
        let db = 20.0 * (rms(&y) / l_in).log10();
        p.auto_gain = false;
        let raw = 20.0 * (rms(&run(&x, &p)) / l_in).log10();
        eprintln!("transpose {tr:+} shift {sh:+}: auto {db:+.2} dB (off {raw:+.2} dB)");
        assert!(db.abs() < 1.5, "level {db:.2} dB");
    }
}

#[test]
fn no_clicks_in_any_mode() {
    let x = vowel(48.0, 2.5, &AH);
    let ref_jump = worst_jump(&x);
    for mode in [FormantMode::Classic, FormantMode::Smooth, FormantMode::Detailed, FormantMode::Lpc, FormantMode::Bend] {
        for (st, tr) in [(5.0f32, 0.0f32), (-5.0, 0.0), (3.0, -5.0), (-3.0, 7.0)] {
            let y = run(&x, &params(mode, st, tr));
            let j = worst_jump(&y);
            assert!(y.iter().all(|v| v.is_finite()));
            assert!(j < (ref_jump * 1.6).max(14.0), "{mode:?} shift {st} transpose {tr}: jump {j:.1} (input {ref_jump:.1})");
        }
    }
}

#[test]
fn neutral_settings_are_transparent() {
    let x = vowel(52.0, 2.0, &AH);
    let y = run(&x, &params(FormantMode::Detailed, 0.0, 0.0));
    let n = x.len();
    let (a, b) = (n / 4, 3 * n / 4);
    let err: f64 = (a..b).map(|i| ((x[i] - y[i]) as f64).powi(2)).sum();
    let sig: f64 = (a..b).map(|i| (x[i] as f64).powi(2)).sum();
    let snr = 10.0 * (sig / err).log10();
    eprintln!("neutral: SNR {snr:.1} dB");
    assert!(snr > 25.0, "neutral settings change the sound: SNR {snr:.1} dB");
}

