//! The formant display gets its data: while a voice plays, the engine reports a voiced input
//! and an envelope, in every formant mode and with or without a pitch shift.
use nyashkformant::engine::{Engine, Params};
use nyashkformant::formant::FormantMode;
use std::f64::consts::PI;

const FS: f64 = 48000.0;

fn vowel(midi: f64, secs: f64) -> Vec<f32> {
    let n = (secs * FS) as usize;
    let mut ph = 0.0;
    let mut x = vec![0.0f64; n];
    for (i, v) in x.iter_mut().enumerate() {
        let f0 = 440.0 * 2f64.powf((midi + 0.15 * (2.0 * PI * 5.5 * i as f64 / FS).sin() - 69.0) / 12.0);
        ph += f0 / FS;
        if ph >= 1.0 {
            ph -= 1.0;
            *v = 1.0;
        }
    }
    let mut y = vec![0.0f64; n];
    for &(fc, bw) in &[(700.0, 80.0), (1200.0, 90.0), (2600.0, 120.0)] {
        let r = (-PI * bw / FS).exp();
        let th = 2.0 * PI * fc / FS;
        let (mut y1, mut y2) = (0.0, 0.0);
        for i in 0..n {
            let s = (1.0 - r) * x[i] + 2.0 * r * th.cos() * y1 - r * r * y2;
            y2 = y1;
            y1 = s;
            y[i] += s * fc / 30.0;
        }
    }
    let pk = y.iter().fold(0.0f64, |a, v| a.max(v.abs()));
    y.into_iter().map(|v| (0.4 * v / pk) as f32).collect()
}

#[test]
fn display_data_while_singing() {
    let v = vowel(57.0, 1.5);
    for mode in [FormantMode::Classic, FormantMode::from_index(1), FormantMode::from_index(2), FormantMode::from_index(3)] {
        for (tr, sh) in [(0.0f32, 3.1f32), (0.0, 0.0), (5.0, 0.0), (-7.0, 2.0)] {
            let mut e = Engine::new(FS, 70.0);
            e.set_params(&Params { formant_mode: mode, formant_semis: sh, transpose: tr, formant_on: true, ..Params::default() });
            let (mut voiced, mut env) = (0, 0);
            for (i, s) in v.iter().enumerate() {
                e.process_frame([*s, *s], 2);
                if i > 24000 && i % 1440 == 0 {
                    voiced += e.meters.voiced as usize;
                    env += e.envelope_db(1000.0).is_some() as usize;
                }
            }
            println!("{mode:?} tr {tr} sh {sh}: voiced {voiced}, env {env}");
            assert!(voiced > 10 && env > 10, "{mode:?} transpose {tr} shift {sh}: voiced {voiced}, envelope {env} of 17 checks");
        }
    }
}
