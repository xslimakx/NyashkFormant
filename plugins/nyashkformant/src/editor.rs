//! NyashkFormant GUI: pitch, formant and colour panels on top; the live spectral envelope of the
//! voice (in and out) with the formant peaks marked, the algorithm notes and the output section
//! below.

use nih_plug::prelude::*;
use nih_plug_egui::{
    create_egui_editor,
    egui::{self, epaint::PathStroke, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2},
};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use crate::{env_freq, FormantAlgo, Meters, NyashkFormantParams, VoiceRange, ENV_N};

// palette
const BG: Color32 = Color32::from_rgb(0x0a, 0x0a, 0x0a);
const PANEL: Color32 = Color32::from_rgb(0x11, 0x11, 0x11);
const PANEL2: Color32 = Color32::from_rgb(0x1c, 0x1c, 0x1c);
const LINE: Color32 = Color32::from_rgb(0x2e, 0x2e, 0x2e);
const WHITE: Color32 = Color32::from_rgb(0xf2, 0xf2, 0xf2);
const DIM: Color32 = Color32::from_rgb(0x80, 0x80, 0x80);
const ILLEGAL: Color32 = Color32::from_rgb(0xd8, 0x62, 0x7a);
const IN_COL: Color32 = Color32::from_rgb(0xe2, 0xb0, 0x58);

const BASE_W: f32 = 900.0;
const BASE_H: f32 = 560.0;

fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

struct Gui {
    in_lv: f32,
    out_lv: f32,
    env: [f32; ENV_N],
    env_ok: bool,
    last_env: f64,
    hz: f32,
    last_hz: f64,
}

fn set_norm<P: Param>(setter: &ParamSetter, p: &P, v: f32) {
    setter.begin_set_parameter(p);
    setter.set_parameter_normalized(p, v.clamp(0.0, 1.0));
    setter.end_set_parameter(p);
}
fn set_b(setter: &ParamSetter, p: &BoolParam, v: bool) {
    setter.begin_set_parameter(p);
    setter.set_parameter(p, v);
    setter.end_set_parameter(p);
}
fn set_e<E: Enum + PartialEq>(setter: &ParamSetter, p: &EnumParam<E>, v: E) {
    setter.begin_set_parameter(p);
    setter.set_parameter(p, v);
    setter.end_set_parameter(p);
}
/// ♯ and ♭ are not in egui's built-in fonts: DejaVu Sans as a fall-back after the primary font.
fn install_fonts(ctx: &egui::Context) {
    use egui::{FontData, FontFamily};
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("dejavu".into(), Arc::new(FontData::from_static(include_bytes!("fonts/DejaVuSans.ttf"))));
    for fam in [FontFamily::Monospace, FontFamily::Proportional] {
        if let Some(list) = fonts.families.get_mut(&fam) {
            let at = list.len().min(1);
            list.insert(at, "dejavu".into());
        }
    }
    ctx.set_fonts(fonts);
}

fn pill(ui: &mut Ui, rect: Rect, text: &str, on: bool, tip: &str) -> bool {
    let mut resp = ui.allocate_rect(rect, Sense::click());
    if !tip.is_empty() {
        resp = resp.on_hover_text(tip);
    }
    let painter = ui.painter();
    let hov = resp.hovered();
    painter.rect_filled(rect, CornerRadius::same(6), if on { WHITE } else if hov { PANEL2 } else { Color32::TRANSPARENT });
    painter.rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, if on { WHITE } else if hov { DIM } else { LINE }), StrokeKind::Inside);
    painter.text(rect.center(), Align2::CENTER_CENTER, text, mono(11.0), if on { Color32::BLACK } else { WHITE });
    resp.clicked()
}

fn panel(ui: &Ui, rect: Rect, title: &str) {
    ui.painter().rect_filled(rect, CornerRadius::same(6), PANEL);
    ui.painter().rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, LINE), StrokeKind::Inside);
    if !title.is_empty() {
        ui.painter().text(Pos2::new(rect.left() + 12.0, rect.top() + 14.0), Align2::LEFT_CENTER, title, mono(10.0), DIM);
    }
}

fn enum_combo<E: Enum + PartialEq + 'static>(ui: &mut Ui, id: &str, rect: Rect, cur: E, shown: &str) -> Option<E> {
    let names = E::variants();
    let idx = cur.to_index();
    let mut pick = None;
    ui.allocate_new_ui(UiBuilder::new().max_rect(rect), |ui| {
        egui::ComboBox::from_id_salt(id)
            .width(rect.width() - 22.0)
            .selected_text(egui::RichText::new(shown).font(mono(12.0)))
            .show_ui(ui, |ui| {
                for (i, n) in names.iter().enumerate() {
                    if ui.selectable_label(i == idx, egui::RichText::new(*n).font(mono(12.0))).clicked() {
                        pick = Some(E::from_index(i));
                    }
                }
            });
    });
    pick
}

/// Knob: drag (shift = fine), wheel, double-click = default, click the value to type.
/// Returns the normalised change made this frame.
#[allow(clippy::too_many_arguments)]
fn knob<P: Param>(ui: &mut Ui, setter: &ParamSetter, p: &P, label: &str, id: &str, c: Pos2, r: f32, tooltip: &str, bipolar: bool) -> f32 {
    let hit = Rect::from_center_size(c, Vec2::splat(2.0 * r + 8.0));
    let resp = ui.allocate_rect(hit, Sense::click_and_drag());
    let resp = if tooltip.is_empty() { resp } else { resp.on_hover_text(tooltip) };
    let norm = p.unmodulated_normalized_value();
    let mut delta = 0.0f32;
    let raw_id = egui::Id::new(("knob_raw", id));
    if resp.drag_started() {
        setter.begin_set_parameter(p);
        ui.data_mut(|d| d.insert_temp(raw_id, norm));
    }
    if resp.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let d = -resp.drag_delta().y / if fine { 1200.0 } else { 220.0 };
        let raw = ui.data(|dd| dd.get_temp::<f32>(raw_id)).unwrap_or(norm) + d;
        ui.data_mut(|dd| dd.insert_temp(raw_id, raw));
        let new = raw.clamp(0.0, 1.0);
        delta = new - norm;
        setter.set_parameter_normalized(p, new);
    }
    if resp.drag_stopped() {
        setter.end_set_parameter(p);
    }
    if resp.double_clicked() {
        delta = p.default_normalized_value() - norm;
        set_norm(setter, p, p.default_normalized_value());
    }
    if resp.hovered() {
        let sc = ui.input(|i| i.smooth_scroll_delta.y);
        if sc != 0.0 {
            let new = (norm + sc / 1500.0).clamp(0.0, 1.0);
            delta = new - norm;
            set_norm(setter, p, new);
        }
    }
    let norm = p.unmodulated_normalized_value();
    let painter = ui.painter();
    let a0 = std::f32::consts::PI * 0.75;
    let sweep = std::f32::consts::PI * 1.5;
    let ring = r - 2.5;
    let pt = |rad: f32, t: f32| -> Pos2 {
        let a = a0 + t * sweep;
        Pos2::new(c.x + rad * a.cos(), c.y + rad * a.sin())
    };
    let arc = |from: f32, to: f32| -> Vec<Pos2> {
        let n = (((to - from).abs() / sweep) * 120.0).ceil().max(2.0) as usize;
        (0..=n).map(|i| pt(ring, from + (to - from) * i as f32 / n as f32)).collect()
    };
    let w = if r > 34.0 { 5.0 } else { 4.0 };
    painter.add(egui::Shape::line(arc(0.0, 1.0), PathStroke::new(w, LINE)));
    let from = if bipolar { 0.5 } else { 0.0 };
    if (norm - from).abs() > 0.002 {
        painter.add(egui::Shape::line(arc(from, norm), PathStroke::new(w, WHITE)));
    }
    let body = r * 0.72;
    painter.circle_filled(c + Vec2::new(0.0, 1.5), body + 1.0, Color32::from_rgb(5, 5, 5));
    painter.circle_filled(c, body, Color32::from_rgb(34, 34, 34));
    painter.circle_filled(c, body * 0.84, Color32::from_rgb(24, 24, 24));
    painter.circle_stroke(c, body, Stroke::new(1.0, Color32::from_rgb(70, 70, 70)));
    painter.line_segment([pt(body * 0.30, norm), pt(body * 0.80, norm)], Stroke::new(3.0, WHITE));

    // value box (click to type) + label
    let val = p.normalized_value_to_string(norm, true);
    let vr = Rect::from_center_size(Pos2::new(c.x, c.y + r + 14.0), Vec2::new(84.0, 18.0));
    let edit_id = egui::Id::new(("knob_edit", id));
    let editing: Option<String> = ui.data(|d| d.get_temp::<String>(edit_id));
    ui.painter().text(Pos2::new(c.x, vr.bottom() + 10.0), Align2::CENTER_CENTER, label, mono(10.0), DIM);
    if let Some(mut buf) = editing {
        let te = egui::TextEdit::singleline(&mut buf).font(mono(11.0)).horizontal_align(egui::Align::Center).desired_width(vr.width()).margin(egui::Margin::symmetric(2, 1));
        let r_te = ui.put(vr, te);
        if !r_te.has_focus() && !r_te.lost_focus() {
            r_te.request_focus();
        }
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
        let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if esc {
            ui.data_mut(|d| d.remove::<String>(edit_id));
        } else if enter || r_te.lost_focus() {
            let txt = buf.trim().replace(',', ".");
            if let Some(nv) = p.string_to_normalized_value(&txt) {
                delta = nv - norm;
                set_norm(setter, p, nv);
            }
            ui.data_mut(|d| d.remove::<String>(edit_id));
        } else {
            ui.data_mut(|d| d.insert_temp(edit_id, buf));
        }
    } else {
        let vresp = ui.allocate_rect(vr, Sense::click()).on_hover_text("Click to type a value");
        let painter = ui.painter();
        painter.rect_stroke(vr, CornerRadius::same(4), Stroke::new(1.0, if vresp.hovered() { DIM } else { LINE }), StrokeKind::Inside);
        let gw = ui.fonts(|f| f.layout_no_wrap(val.clone(), mono(11.0), WHITE).size().x);
        let size = if gw > vr.width() - 6.0 { (11.0 * (vr.width() - 6.0) / gw).max(7.5) } else { 11.0 };
        painter.text(vr.center(), Align2::CENTER_CENTER, &val, mono(size), WHITE);
        if vresp.clicked() {
            let first = val.trim().split(' ').next().unwrap_or("").to_string();
            let same = p.string_to_normalized_value(&first).map(|m| p.normalized_value_to_string(m, true)).as_deref() == Some(val.as_str());
            let plain = if same { first } else { val.trim().to_string() };
            ui.data_mut(|d| d.insert_temp(edit_id, plain));
        }
    }
    delta
}

/// Vertical peak meter, -28 .. 0 dBFS.
fn meter(ui: &Ui, rect: Rect, lin: f32, label: &str) {
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(2), Color32::from_rgb(7, 7, 7));
    let db = 20.0 * lin.max(1e-6).log10();
    let t = ((db + 28.0) / 28.0).clamp(0.0, 1.0);
    let h = rect.height() * t;
    let col = if db > -0.5 { ILLEGAL } else { Color32::from_gray(200) };
    p.rect_filled(Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - h), rect.max), CornerRadius::same(2), col);
    p.rect_stroke(rect, CornerRadius::same(2), Stroke::new(1.0, LINE), StrokeKind::Inside);
    p.text(Pos2::new(rect.center().x, rect.bottom() + 9.0), Align2::CENTER_CENTER, label, mono(9.0), DIM);
}

/// Where the output at `f` Hz takes its envelope from (the same warp the engine applies).
fn source_freq(f: f32, s: f32, tilt: f32, algo: FormantAlgo) -> f32 {
    let x = (f / 8000.0).min(1.0);
    let tilt = if algo == FormantAlgo::Classic { 0.0 } else { tilt };
    let st = (s.ln() * (1.0 + tilt * (2.0 * x - 1.0))).exp();
    if algo == FormantAlgo::Bend {
        400.0 * (f / 400.0).powf(1.0 / st)
    } else {
        f / st
    }
}

/// Envelope (dB) at any frequency, from the display points (log-spaced).
fn env_at(env: &[f32; ENV_N], f: f32) -> f32 {
    let lo = env_freq(0) as f32;
    let hi = env_freq(ENV_N - 1) as f32;
    let t = ((f.clamp(lo, hi) / lo).ln() / (hi / lo).ln()) * (ENV_N - 1) as f32;
    let i = (t.floor() as usize).min(ENV_N - 2);
    let fr = t - i as f32;
    env[i] * (1.0 - fr) + env[i + 1] * fr
}

/// The first few resonance peaks of an envelope curve (Hz).
fn peaks(curve: &[(f32, f32)]) -> Vec<f32> {
    let mut out = Vec::new();
    for k in 1..curve.len() - 1 {
        let (f, v) = curve[k];
        if v > curve[k - 1].1 && v >= curve[k + 1].1 && f > 150.0 && f < 4500.0 {
            // a real bump: at least 3 dB over the lowest point on either side within the neighbours
            let l = curve[k.saturating_sub(24)..k].iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
            let r = curve[k + 1..(k + 25).min(curve.len())].iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
            if v - l > 3.0 || v - r > 3.0 {
                out.push(f);
                if out.len() == 3 {
                    break;
                }
            }
        }
    }
    out
}

fn fmt_hz(f: f32) -> String {
    if f >= 1000.0 {
        let k = f / 1000.0;
        // whole kHz without the decimal ("1k", "10k"), others with one ("2.7k")
        if (k - k.round()).abs() < 0.05 {
            format!("{}k", k.round() as i32)
        } else {
            format!("{k:.1}k")
        }
    } else {
        format!("{:.0}", f)
    }
}

pub fn create(params: Arc<NyashkFormantParams>, meters: Arc<Meters>) -> Option<Box<dyn Editor>> {
    let editor_state = params.editor_state.clone();
    let size_state = editor_state.clone();
    create_egui_editor(
        editor_state,
        Gui { in_lv: 0.0, out_lv: 0.0, env: [-120.0; ENV_N], env_ok: false, last_env: -10.0, hz: 0.0, last_hz: -10.0 },
        |ctx, _| {
            let mut v = egui::Visuals::dark();
            v.panel_fill = BG;
            v.window_fill = PANEL;
            v.extreme_bg_color = PANEL;
            v.override_text_color = Some(WHITE);
            v.window_stroke = Stroke::new(1.0, LINE);
            v.selection.bg_fill = Color32::from_rgb(64, 64, 64);
            v.selection.stroke = Stroke::new(1.0, WHITE);
            for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
                w.corner_radius = CornerRadius::same(6);
                w.bg_stroke = Stroke::new(1.0, LINE);
            }
            v.widgets.inactive.weak_bg_fill = PANEL;
            v.widgets.inactive.bg_fill = PANEL;
            v.widgets.hovered.weak_bg_fill = PANEL2;
            v.widgets.hovered.bg_fill = PANEL2;
            v.widgets.active.weak_bg_fill = PANEL2;
            v.widgets.open.weak_bg_fill = PANEL2;
            v.widgets.hovered.bg_stroke = Stroke::new(1.0, DIM);
            v.popup_shadow = egui::Shadow::NONE;
            v.window_shadow = egui::Shadow::NONE;
            ctx.set_visuals(v);
            install_fonts(ctx);
        },
        move |ctx, setter, st| {
            ctx.request_repaint_after(Duration::from_millis(33));
            let p = &*params;
            let now = ctx.input(|i| i.time);

            // developer helper for screenshots: NYASHK_DEMO_SHIFT=<st> sets the formant shift once
            static DEMO: std::sync::Once = std::sync::Once::new();
            DEMO.call_once(|| {
                if let Some(v) = std::env::var("NYASHK_DEMO_SHIFT").ok().and_then(|v| v.parse::<f32>().ok()) {
                    set_norm(setter, &p.shift, p.shift.preview_normalized(v));
                }
                // NYASHK_DEMO_SET="transpose=7,keep=0,algo=4,..." (screenshots of other states)
                for kv in std::env::var("NYASHK_DEMO_SET").unwrap_or_default().split(',') {
                    let Some((k, v)) = kv.split_once('=') else { continue };
                    let Ok(v) = v.trim().parse::<f32>() else { continue };
                    let fp = match k.trim() {
                        "transpose" => Some(&p.transpose),
                        "detune" => Some(&p.detune),
                        "shift" => Some(&p.shift),
                        "tilt" => Some(&p.tilt),
                        "follow" => Some(&p.follow),
                        "air" => Some(&p.air),
                        "drive" => Some(&p.drive),
                        "mix" => Some(&p.mix),
                        "out" => Some(&p.out_gain),
                        _ => None,
                    };
                    if let Some(fp) = fp {
                        set_norm(setter, fp, fp.preview_normalized(v));
                        continue;
                    }
                    match k.trim() {
                        "formanton" => set_b(setter, &p.formant_on, v > 0.5),
                        "keep" => set_b(setter, &p.keep_formants, v > 0.5),
                        "colour" => set_b(setter, &p.colour_on, v > 0.5),
                        "autogain" => set_b(setter, &p.auto_gain, v > 0.5),
                        "bypass" => set_b(setter, &p.bypass, v > 0.5),
                        "algo" => set_e(setter, &p.fmode, FormantAlgo::from_index(v as usize)),
                        "range" => set_e(setter, &p.range, VoiceRange::from_index(v as usize)),
                        "size" => size_state.set_requested_size((v as u32, (v * BASE_H / BASE_W) as u32)),
                        _ => {}
                    }
                }
            });
            // ---- meters and the envelope
            let fall = |cur: f32, new: f32| if new > cur { new } else { cur * 0.86 };
            st.in_lv = fall(st.in_lv, meters.in_pk.swap(0.0, Ordering::Relaxed));
            st.out_lv = fall(st.out_lv, meters.out_pk.swap(0.0, Ordering::Relaxed));
            if let Some(g) = meters.env.try_lock() {
                if g.1 {
                    // light smoothing so the curve breathes instead of flickering
                    let a = if st.env_ok { 0.45 } else { 1.0 };
                    for i in 0..ENV_N {
                        st.env[i] += (g.0[i] - st.env[i]) * a;
                    }
                    st.env_ok = true;
                    st.last_env = now;
                }
            }
            let live = st.env_ok && now - st.last_env < 0.5;

            egui::CentralPanel::default().frame(egui::Frame::NONE.fill(BG)).show(ctx, |ui| {
                let full = ui.max_rect();
                let o = full.min;
                let rect = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(Pos2::new(o.x + x, o.y + y), Vec2::new(w, h));
                let at = |x: f32, y: f32| Pos2::new(o.x + x, o.y + y);
                let painter = ui.painter().clone();

                // ---- header
                painter.text(at(18.0, 32.0), Align2::LEFT_CENTER, "Nyashk", mono(19.0), WHITE);
                painter.text(at(18.0 + 6.0 * 11.4, 32.0), Align2::LEFT_CENTER, "Formant", mono(19.0), DIM);
                painter.text(at(28.0 + 13.0 * 11.4 + 6.0, 34.0), Align2::LEFT_CENTER, "FORMANT & PITCH SHIFTER", mono(10.5), DIM);
                painter.hline(o.x + 14.0..=o.x + BASE_W - 14.0, o.y + 56.0, Stroke::new(1.0, LINE));
                let byp = p.bypass.value();
                if pill(ui, rect(BASE_W - 14.0 - 84.0, 20.0, 84.0, 24.0), "BYPASS", byp, "Bypass the plugin (the latency stays, so nothing jumps in time).") {
                    set_b(setter, &p.bypass, !byp);
                }
                painter.text(at(BASE_W - 14.0 - 84.0 - 10.0 - 150.0 - 8.0, 32.0), Align2::RIGHT_CENTER, "VOICE", mono(10.0), DIM);
                let vr = p.range.value();
                if let Some(nv) = enum_combo::<VoiceRange>(ui, "voice", rect(BASE_W - 14.0 - 84.0 - 10.0 - 150.0, 20.0, 150.0, 24.0), vr, VoiceRange::variants()[vr.to_index()]) {
                    set_e(setter, &p.range, nv);
                }

                // ---- row 1
                let y1 = 70.0;
                let h1 = 176.0;
                let ky = y1 + 78.0;
                // 01 pitch
                let r01 = rect(14.0, y1, 190.0, h1);
                panel(ui, r01, "№ 01  PITCH");
                knob(ui, setter, &p.transpose, "Transpose", "tr", at(14.0 + 52.0, ky), 30.0, "Pitch shift in semitones.\nWith KEEP FORMANTS on the voice keeps its own timbre.", true);
                knob(ui, setter, &p.detune, "Detune", "dt", at(14.0 + 140.0, ky), 30.0, "Fine pitch shift in cents.", true);

                // 02 formant
                let r02 = rect(214.0, y1, 400.0, h1);
                panel(ui, r02, "№ 02  FORMANT");
                let fon = p.formant_on.value();
                let algo = p.fmode.value();
                if let Some(nv) = enum_combo::<FormantAlgo>(ui, "algo", rect(214.0 + 114.0, y1 + 3.0, 128.0, 22.0), algo, algo.name()) {
                    set_e(setter, &p.fmode, nv);
                }
                if pill(ui, rect(214.0 + 400.0 - 12.0 - 104.0 - 6.0 - 40.0, y1 + 4.0, 40.0, 20.0), "ON", fon, "Formant processing on / off.\nOff: the formants are left alone (Transpose still keeps them);\nShift, Tilt and Follow do nothing.") {
                    set_b(setter, &p.formant_on, !fon);
                }
                let keep = p.keep_formants.value();
                if pill(ui, rect(214.0 + 400.0 - 12.0 - 104.0, y1 + 4.0, 104.0, 20.0), "KEEP FORMANTS", keep, "On: Transpose moves only the pitch, the formants stay put.\nOff: the formants move with the pitch by Follow %.") {
                    set_b(setter, &p.keep_formants, !keep);
                }
                knob(ui, setter, &p.shift, "Shift", "sh", at(214.0 + 70.0, ky), 30.0, "Formant shift in semitones: + smaller, brighter vocal tract, - bigger, darker.", true);
                knob(ui, setter, &p.tilt, "Tilt", "ti", at(214.0 + 200.0, ky), 30.0, "Move the high formants more (+) or the low ones more (-).\nNot used by Classic.", true);
                knob(ui, setter, &p.follow, "Follow", "fo", at(214.0 + 330.0, ky), 30.0, "How much the formants follow Transpose when KEEP FORMANTS is off.", false);
                if !fon {
                    painter.rect_filled(Rect::from_min_max(r02.min + Vec2::new(1.0, 30.0), r02.max - Vec2::splat(1.0)), CornerRadius::same(6), Color32::from_black_alpha(150));
                } else {
                    if keep {
                        let fr = Rect::from_center_size(at(214.0 + 330.0, ky + 18.0), Vec2::new(96.0, 100.0));
                        painter.rect_filled(fr, CornerRadius::same(6), Color32::from_black_alpha(140));
                    }
                    if algo == FormantAlgo::Classic {
                        let fr = Rect::from_center_size(at(214.0 + 200.0, ky + 18.0), Vec2::new(96.0, 100.0));
                        painter.rect_filled(fr, CornerRadius::same(6), Color32::from_black_alpha(140));
                    }
                }

                // 03 colour
                let r03 = rect(624.0, y1, BASE_W - 14.0 - 624.0, h1);
                panel(ui, r03, "№ 03  COLOUR");
                let con = p.colour_on.value();
                if pill(ui, rect(BASE_W - 14.0 - 12.0 - 40.0, y1 + 4.0, 40.0, 20.0), "ON", con, "Air and Drive on / off.") {
                    set_b(setter, &p.colour_on, !con);
                }
                knob(ui, setter, &p.air, "Air", "air", at(624.0 + 70.0, ky), 30.0, "High shelf above 7 kHz on the processed voice.", true);
                knob(ui, setter, &p.drive, "Drive", "drv", at(624.0 + 192.0, ky), 30.0, "Soft saturation of the processed voice.", false);
                if !con {
                    painter.rect_filled(Rect::from_min_max(r03.min + Vec2::new(1.0, 30.0), r03.max - Vec2::splat(1.0)), CornerRadius::same(6), Color32::from_black_alpha(150));
                }

                // ---- row 2
                let y2 = y1 + h1 + 10.0;
                let h2 = BASE_H - 14.0 - y2;
                // 04 the envelope
                let r04 = rect(14.0, y2, 560.0, h2);
                panel(ui, r04, "№ 04  FORMANTS");
                let s = p.formant_factor().clamp(0.4, 2.5);
                let st_sh = 12.0 * s.log2();
                let head = if byp {
                    ("bypassed".to_string(), DIM)
                } else if !fon {
                    ("formant processing off".to_string(), DIM)
                } else {
                    (format!("formants x{:.2}  ({:+.1} st)", s, st_sh), if (s - 1.0).abs() > 0.003 { WHITE } else { DIM })
                };
                painter.text(at(14.0 + 560.0 - 12.0, y2 + 14.0), Align2::RIGHT_CENTER, head.0, mono(10.0), head.1);
                let g = Rect::from_min_max(at(14.0 + 40.0, y2 + 34.0), at(14.0 + 560.0 - 14.0, y2 + h2 - 30.0));
                painter.rect_filled(g, CornerRadius::same(3), Color32::from_rgb(8, 8, 8));
                let (f_lo, f_hi) = (env_freq(0) as f32, env_freq(ENV_N - 1) as f32);
                let fx = |f: f32| g.left() + g.width() * ((f / f_lo).ln() / (f_hi / f_lo).ln());
                let (db_top, db_bot) = (14.0f32, -54.0f32);
                let dy = |d: f32| g.top() + g.height() * ((db_top - d) / (db_top - db_bot)).clamp(0.0, 1.0);
                let grid = |painter: &egui::Painter| {
                    for f in [100.0f32, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0] {
                        painter.vline(fx(f), g.top()..=g.bottom(), Stroke::new(1.0, Color32::from_rgb(30, 30, 30)));
                    }
                    for d in [0.0f32, -12.0, -24.0, -36.0, -48.0] {
                        painter.hline(g.left()..=g.right(), dy(d), Stroke::new(1.0, Color32::from_rgb(30, 30, 30)));
                    }
                };
                for f in [100.0f32, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0] {
                    painter.text(Pos2::new(fx(f), g.bottom() + 10.0), Align2::CENTER_CENTER, fmt_hz(f), mono(9.0), DIM);
                }
                for d in [0.0f32, -12.0, -24.0, -36.0, -48.0] {
                    painter.text(Pos2::new(g.left() - 6.0, dy(d)), Align2::RIGHT_CENTER, format!("{d:.0}"), mono(9.0), DIM);
                }
                grid(&painter);
                if st.env_ok {
                    let top = st.env.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                    let n = 180;
                    let mut cin = Vec::with_capacity(n);
                    let mut cout = Vec::with_capacity(n);
                    let fon_eff = fon && !byp;
                    for i in 0..n {
                        let f = f_lo * (f_hi / f_lo).powf(i as f32 / (n - 1) as f32);
                        let vi = env_at(&st.env, f) - top;
                        let src = if fon_eff { source_freq(f, s, p.tilt.value() / 100.0, algo) } else { f };
                        let vo = env_at(&st.env, src) - top;
                        cin.push((f, vi));
                        cout.push((f, vo));
                    }
                    let alpha = if live { 255 } else { 90 };
                    let in_col = Color32::from_rgba_unmultiplied(IN_COL.r(), IN_COL.g(), IN_COL.b(), alpha);
                    let out_col = Color32::from_rgba_unmultiplied(255, 255, 255, alpha);
                    let pts = |c: &Vec<(f32, f32)>| c.iter().map(|(f, v)| Pos2::new(fx(*f), dy(*v))).collect::<Vec<_>>();
                    // fill under the output curve
                    let po = pts(&cout);
                    let mut mesh = egui::Mesh::default();
                    let fc = if live { Color32::from_rgb(22, 22, 22) } else { Color32::from_rgb(14, 14, 14) };
                    for (i, q) in po.iter().enumerate() {
                        mesh.colored_vertex(*q, fc);
                        mesh.colored_vertex(Pos2::new(q.x, g.bottom() - 1.0), fc);
                        if i > 0 {
                            let b = (2 * i) as u32;
                            mesh.add_triangle(b - 2, b - 1, b);
                            mesh.add_triangle(b - 1, b + 1, b);
                        }
                    }
                    painter.add(egui::Shape::mesh(mesh));
                    grid(&painter);
                    painter.add(egui::Shape::line(pts(&cin), PathStroke::new(1.5, in_col)));
                    painter.add(egui::Shape::line(po, PathStroke::new(2.0, out_col)));
                    // formant peaks: where they were and where they go
                    let pi = peaks(&cin);
                    let pk_o = peaks(&cout);
                    for (k, f) in pi.iter().enumerate() {
                        let x = fx(*f);
                        painter.circle_filled(Pos2::new(x, dy(env_at(&st.env, *f) - top)), 3.0, in_col);
                        painter.text(Pos2::new(x, g.top() + 10.0), Align2::CENTER_CENTER, format!("F{} {}", k + 1, fmt_hz(*f)), mono(9.0), in_col);
                    }
                    for f in pk_o.iter() {
                        let src = if fon_eff { source_freq(*f, s, p.tilt.value() / 100.0, algo) } else { *f };
                        let x = fx(*f);
                        painter.circle_filled(Pos2::new(x, dy(env_at(&st.env, src) - top)), 3.5, out_col);
                        if (s - 1.0).abs() > 0.01 && fon_eff {
                            painter.text(Pos2::new(x, g.top() + 22.0), Align2::CENTER_CENTER, fmt_hz(*f), mono(9.0), out_col);
                        }
                    }
                } else {
                    painter.text(g.center(), Align2::CENTER_CENTER, "sing or play a voice to see its formants", mono(11.0), DIM);
                }
                painter.rect_stroke(g, CornerRadius::same(3), Stroke::new(1.0, LINE), StrokeKind::Inside);
                // legend
                let ly = y2 + h2 - 12.0;
                painter.line_segment([at(14.0 + 40.0, ly), at(14.0 + 58.0, ly)], Stroke::new(1.5, IN_COL));
                painter.text(at(14.0 + 64.0, ly), Align2::LEFT_CENTER, "input", mono(9.5), DIM);
                painter.line_segment([at(14.0 + 120.0, ly), at(14.0 + 138.0, ly)], Stroke::new(2.0, WHITE));
                painter.text(at(14.0 + 144.0, ly), Align2::LEFT_CENTER, "output", mono(9.5), DIM);
                let hz_now = meters.hz.load(Ordering::Relaxed);
                if meters.voiced.load(Ordering::Relaxed) != 0 && hz_now > 20.0 {
                    st.hz = if now - st.last_hz > 0.3 { hz_now } else { st.hz + (hz_now - st.hz) * 0.3 };
                    st.last_hz = now;
                }
                let hz = st.hz;
                // hold the last pitch for a moment (the detector drops out between syllables)
                if now - st.last_hz < 0.8 && hz > 20.0 && !byp {
                    let ratio = 2f32.powf((p.transpose.value() + p.detune.value() / 100.0) / 12.0);
                    painter.text(at(14.0 + 560.0 - 14.0, ly), Align2::RIGHT_CENTER, format!("pitch {:.0} Hz -> {:.0} Hz", hz, hz * ratio), mono(9.5), DIM);
                }

                // 05 algorithm
                let x5 = 14.0 + 560.0 + 10.0;
                let w5 = BASE_W - 14.0 - x5;
                let r05 = rect(x5, y2, w5, 120.0);
                panel(ui, r05, "№ 05  ALGORITHM");
                painter.text(at(x5 + 14.0, y2 + 38.0), Align2::LEFT_CENTER, algo.name(), mono(15.0), WHITE);
                for (i, l) in algo.about().iter().enumerate() {
                    painter.text(at(x5 + 14.0, y2 + 62.0 + i as f32 * 15.0), Align2::LEFT_CENTER, *l, mono(10.0), DIM);
                }

                // 06 output
                let y6 = y2 + 130.0;
                let r06 = rect(x5, y6, w5, BASE_H - 14.0 - y6);
                panel(ui, r06, "№ 06  OUTPUT");
                let ag = p.auto_gain.value();
                if pill(ui, rect(x5 + w5 - 12.0 - 84.0, y6 + 4.0, 84.0, 20.0), "AUTO GAIN", ag, "Keep the processed voice as loud as the input.\n(A pitch or formant shift changes how loud a voice sounds.)") {
                    set_b(setter, &p.auto_gain, !ag);
                }
                let ky6 = y6 + 78.0;
                knob(ui, setter, &p.mix, "Mix", "mix", at(x5 + 56.0, ky6), 28.0, "Dry / processed balance.", false);
                knob(ui, setter, &p.out_gain, "Output", "out", at(x5 + 152.0, ky6), 28.0, "Output level.", true);
                let mh = 92.0;
                meter(ui, rect(x5 + w5 - 64.0, ky6 - 46.0, 14.0, mh), st.in_lv, "IN");
                meter(ui, rect(x5 + w5 - 36.0, ky6 - 46.0, 14.0, mh), st.out_lv, "OUT");

                // resize grip
                let grip = Rect::from_min_size(full.max - Vec2::splat(18.0), Vec2::splat(18.0));
                let r = ui.interact(grip, egui::Id::new("formant_grip"), Sense::drag()).on_hover_text("Drag to resize");
                if r.dragged() {
                    if let Some(pos) = r.interact_pointer_pos() {
                        let ppp = if cfg!(target_os = "macos") { size_state.size().0 as f32 / BASE_W } else { ctx.pixels_per_point() };
                        let w = ((pos.x - full.min.x + 9.0) * ppp).clamp(680.0, 2700.0);
                        size_state.set_requested_size((w.round() as u32, (w * BASE_H / BASE_W).round() as u32));
                    }
                }
                let gc = if r.hovered() || r.dragged() { WHITE } else { DIM };
                for k in [5.0f32, 9.0, 13.0] {
                    painter.line_segment([Pos2::new(full.max.x - 3.0, full.max.y - k), Pos2::new(full.max.x - k, full.max.y - 3.0)], Stroke::new(1.0, gc));
                }
            });
        },
    )
}
