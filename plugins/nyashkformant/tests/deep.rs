//! Pitch accuracy of Transpose / Detune in every mode.

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

/// median pitch (MIDI) of the middle of a signal by autocorrelation over 40 ms windows
fn median_pitch(y: &[f32]) -> f64 {
    let w = (0.04 * FS) as usize;
    let mut ps = vec![];
    let mut i = y.len() / 4;
    while i + 2 * w < y.len() * 3 / 4 {
        let seg = &y[i..i + 2 * w];
        let (lo, hi) = ((FS / 900.0) as usize, (FS / 70.0) as usize);
        let mut best = (0usize, 0.0f64);
        let e0: f64 = seg[..w].iter().map(|v| (*v as f64).powi(2)).sum();
        for lag in lo..hi {
            let mut c = 0.0f64;
            let mut e1 = 0.0f64;
            for k in 0..w {
                c += seg[k] as f64 * seg[k + lag] as f64;
                e1 += (seg[k + lag] as f64).powi(2);
            }
            let r = c / (e0 * e1).sqrt().max(1e-12);
            if r > best.1 {
                best = (lag, r);
            }
        }
        // parabolic refinement
        let l = best.0 as f64;
        let f0 = FS / l;
        ps.push(69.0 + 12.0 * (f0 / 440.0).log2());
        i += w;
    }
    ps.sort_by(|a, b| a.partial_cmp(b).unwrap());
    ps[ps.len() / 2]
}

#[test]
fn transpose_and_detune_land_on_pitch() {
    let base = 57.0; // A3
    let x = vowel(base, 2.0, &[(700.0, 80.0), (1200.0, 90.0), (2600.0, 120.0)]);
    let p_in = median_pitch(&x);
    for mode in [FormantMode::Classic, FormantMode::Smooth, FormantMode::Detailed, FormantMode::Lpc, FormantMode::Bend] {
        for (st, dc) in [(7.0f32, 0.0f32), (-5.0, 0.0), (12.0, 0.0), (-12.0, 0.0), (0.0, 50.0)] {
            let mut p = params(mode, 0.0, st);
            p.fine_cents = dc;
            let y = run(&x, &p);
            let got = median_pitch(&y) - p_in;
            let want = st as f64 + dc as f64 / 100.0;
            println!("{mode:?} transpose {st:+} st detune {dc:+} c: moved {got:+.2} st");
            assert!((got - want).abs() < 0.25, "{mode:?} {st} {dc}: moved {got:.2}, wanted {want:.2}");
        }
    }
}

fn amp_at(y: &[f32], f: f64) -> f64 {
    let (mut re, mut im) = (0.0f64, 0.0f64);
    let n = y.len();
    for (i, v) in y.iter().enumerate() {
        let w = 0.5 - 0.5 * (2.0 * PI * i as f64 / n as f64).cos();
        let ph = 2.0 * PI * f * i as f64 / FS;
        re += *v as f64 * w * ph.cos();
        im += *v as f64 * w * ph.sin();
    }
    (re * re + im * im).sqrt()
}

#[test]
fn octave_down_really_is_an_octave_down() {
    // steady A3 (no vibrato) so the partials are sharp lines
    let n = (2.0 * FS) as usize;
    let x: Vec<f32> = (0..n).map(|i| {
        let t = i as f64 / FS;
        (0.3 * ((2.0 * PI * 220.0 * t).sin() + 0.5 * (2.0 * PI * 440.0 * t).sin() + 0.3 * (2.0 * PI * 660.0 * t).sin())) as f32
    }).collect();
    for mode in [FormantMode::Classic, FormantMode::Smooth, FormantMode::Detailed, FormantMode::Lpc, FormantMode::Bend] {
        let y = run(&x, &params(mode, 0.0, -12.0));
        let mid = &y[n / 4..n * 3 / 4];
        let (a110, a330, a220) = (amp_at(mid, 110.0), amp_at(mid, 330.0), amp_at(mid, 220.0));
        let rms = (mid.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / mid.len() as f64).sqrt();
        println!("{mode:?} rms {rms:.4} a220 {a220:.3}");
        println!("{mode:?} -12 st: 110 Hz {:.1} dB, 330 Hz {:.1} dB rel. 220 Hz", 20.0 * (a110 / a220).log10(), 20.0 * (a330 / a220).log10());
        assert!(a110 > a220 * 0.1, "{mode:?}: no 110 Hz fundamental after an octave down");
    }
}

