//! HXtra's NAM A2 Lite captures: a `.nam` file (a 3-channel "A2 Lite" WaveNet) as the
//! 2,048-sample User IR the NAM A2 Lite block plays, the same samples as HXtra's
//! `tools/nam2ir.py` and `web/nam2ir-core.js`. The `.nam` itself is not kept.

use serde_json::Value as Json;

use crate::{Error, Result};

pub const IR_SAMPLES: usize = 2048;
pub const IR_BYTES: usize = IR_SAMPLES * 4;
const C: usize = 3;
const HEAD_TAPS: usize = 16;
const IR_LIMIT: f64 = 15.8489;
const PEAK_LIMIT: f64 = 15.5;
const KERNEL: [usize; 23] = [6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 15, 15, 6, 6, 6, 6, 6, 6, 6];
const DILATION: [u64; 23] = [1, 3, 7, 17, 41, 101, 239, 1, 3, 7, 17, 41, 101, 239, 1, 13, 1, 3, 7, 17, 41, 101, 239];
const N_WEIGHTS: usize = 1871;
const LOUD_REF: f64 = -18.0;
const SAMPLE_RATE: f64 = 48000.0;
const FILM: [&str; 8] = [
    "conv_pre_film",
    "conv_post_film",
    "input_mixin_pre_film",
    "input_mixin_post_film",
    "activation_pre_film",
    "activation_post_film",
    "layer1x1_post_film",
    "head1x1_post_film",
];

/// A capture converted for an IR slot.
#[derive(Debug, Clone)]
pub struct Capture {
    pub samples: Vec<f32>,
    pub name: Option<String>,
    pub input_dbu: Option<f64>,
    pub loudness: Option<f64>,
    pub exponent: u32,
}

impl Capture {
    /// The 8,192 bytes an IR slot holds (little-endian float32, as the librarian `0x0b` sends them).
    pub fn ir_bytes(&self) -> Vec<u8> {
        self.samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    /// A 48 kHz mono 32-bit float `.wav` of the samples, as HXtra's `nam2ir.py` writes it.
    pub fn wav_bytes(&self) -> Vec<u8> {
        let data = self.ir_bytes();
        let mut out = Vec::with_capacity(44 + data.len());
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&3u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&48000u32.to_le_bytes());
        out.extend_from_slice(&(48000u32 * 4).to_le_bytes());
        out.extend_from_slice(&4u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend(data);
        out
    }
}

/// Convert the text of a `.nam` file, or say why the NAM A2 Lite block cannot run it.
pub fn convert(text: &str) -> Result<Capture> {
    let d: Json = serde_json::from_str(text).map_err(|_| bad("the file is not valid JSON, so not a .nam file"))?;
    let m = pick_model(&d)?;
    let problems = problems(m);
    if !problems.is_empty() {
        return Err(bad(&format!("not an A2 Lite capture the block can run: {}", problems.join("; "))));
    }
    for rate in [d.get("sample_rate"), m.get("sample_rate")].into_iter().flatten().filter_map(Json::as_f64) {
        if rate != SAMPLE_RATE {
            return Err(bad(&format!("trained at {rate} Hz; the Stomp runs at 48 kHz and does not resample")));
        }
    }
    let meta = d.get("metadata");
    let number = |k: &str| meta.and_then(|m| m.get(k)).and_then(Json::as_f64).filter(|x| x.is_finite());
    let (input_dbu, loudness) = (number("input_level_dbu"), number("loudness"));
    let weights: Vec<f64> = m["weights"].as_array().map(|w| w.iter().filter_map(Json::as_f64).collect()).unwrap_or_default();
    let (samples, exponent) = ir_samples(&weights, input_dbu, loudness)?;
    let name = meta.and_then(|m| m.get("name")).and_then(Json::as_str).filter(|s| !s.is_empty()).map(str::to_owned);
    Ok(Capture { samples, name, input_dbu, loudness, exponent })
}

/// Whether IR slot data holds an HXtra capture (its header: 0.5, -0.5, 3, a scale, 1871/1024).
pub fn is_capture(ir: &[u8]) -> bool {
    let word = |i: usize| ir.get(i * 4..i * 4 + 4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let n = N_WEIGHTS as f32 / 1024.0;
    matches!((word(0), word(1), word(2), word(4)), (Some(a), Some(b), Some(c), Some(d)) if a == 0.5 && b == -0.5 && c == C as f32 && d == n)
}

fn bad(why: &str) -> Error {
    Error::Nam(why.to_owned())
}

fn pick_model(d: &Json) -> Result<&Json> {
    let arch = d.get("architecture").and_then(Json::as_str).ok_or_else(|| bad("this is not a NAM model file: it has no architecture"))?;
    let subs: Vec<&Json> = match arch {
        "SlimmableContainer" => d["config"]["submodels"].as_array().map(|s| s.iter().map(|s| &s["model"]).collect()).unwrap_or_default(),
        _ => vec![d],
    };
    let channels = |m: &Json| m["config"]["layers"].get(0).and_then(|l| l["channels"].as_u64());
    let found = subs.iter().copied().find(|m| m["architecture"].as_str() == Some("WaveNet") && channels(m) == Some(C as u64));
    found.ok_or_else(|| {
        let seen: Vec<String> = subs
            .iter()
            .map(|m| match (m["architecture"].as_str(), channels(m)) {
                (Some("WaveNet"), Some(c)) => format!("{c}-channel WaveNet"),
                (Some(a), _) => a.to_owned(),
                (None, _) => "nothing usable".to_owned(),
            })
            .collect();
        bad(&format!("no 3-channel A2 Lite model inside (found: {}); NAM A2 Lite plays A2 Lite captures only", seen.join(", ")))
    })
}

/// What would make the block compute something else than this capture (web/nam2ir-core.js `problems`).
fn problems(m: &Json) -> Vec<String> {
    let mut out = Vec::new();
    let cfg = &m["config"];
    let layers = cfg["layers"].as_array().map(Vec::as_slice).unwrap_or_default();
    let l = layers.first().unwrap_or(&Json::Null);
    let absent = |v: &Json| v.is_null();
    let off = |v: &Json| v.is_null() || v == &Json::Bool(false) || (v.is_object() && !v["active"].as_bool().unwrap_or(false));
    let ints = |v: &Json| v.as_array().map(|a| a.iter().filter_map(Json::as_u64).collect::<Vec<_>>());
    let n = KERNEL.len();
    if layers.len() != 1 {
        out.push(format!("{} layer arrays; A2 Lite has 1", layers.len()));
    }
    if !absent(&cfg["head"]) || !absent(&cfg["condition_dsp"]) {
        out.push("a post-stack head or a condition DSP".into());
    }
    if !cfg["head_scale"].is_number() || (!absent(&cfg["in_channels"]) && cfg["in_channels"].as_u64() != Some(1)) {
        out.push("no head_scale".into());
    }
    if l["input_size"].as_u64() != Some(1) || l["condition_size"].as_u64() != Some(1) {
        out.push("input_size and condition_size are not 1".into());
    }
    if l["bottleneck"].as_u64() != Some(C as u64) {
        out.push(format!("bottleneck is not {C}"));
    }
    if absent(&l["kernel_sizes"]) || absent(&l["dilations"]) {
        out.push("no kernel sizes or dilations".into());
    }
    if l["activation"].as_array().map(Vec::len) != Some(n) {
        out.push("not one activation per layer".into());
    }
    if l["gated"] == Json::Bool(true) {
        out.push("gated".into());
    }
    let secondary_ok = match &l["secondary_activation"] {
        Json::Null => true,
        Json::Array(a) => a.len() == n && a.iter().all(Json::is_null),
        _ => false,
    };
    if !secondary_ok {
        out.push("a secondary activation".into());
    }
    if l["head1x1"]["active"].as_bool() == Some(true) {
        out.push("head1x1 is on".into());
    }
    if l["layer1x1"]["active"].as_bool() != Some(true) || (!absent(&l["layer1x1"]["groups"]) && l["layer1x1"]["groups"].as_u64() != Some(1)) {
        out.push("layer1x1 is not on with groups 1".into());
    }
    let h = &l["head"];
    let head_ok = h["out_channels"].as_u64() == Some(1)
        && h["kernel_size"].as_u64() == Some(HEAD_TAPS as u64)
        && (absent(&h["head_dilation"]) || h["head_dilation"].as_u64() == Some(1))
        && h["bias"].as_bool() == Some(true);
    if !head_ok {
        out.push(format!("the head is not {HEAD_TAPS} taps with a bias"));
    }
    for k in FILM {
        if !off(&l[k]) {
            out.push(format!("FiLM ({k})"));
        }
    }
    let grouped = |k: &str| !absent(&l[k]) && l[k].as_u64() != Some(1);
    if grouped("groups_input") || grouped("groups_input_mixin") {
        out.push("grouped convolutions".into());
    }
    if !absent(&l["slimmable"]) {
        out.push("slimmable layers".into());
    }
    if ints(&l["kernel_sizes"]).is_some_and(|k| k != KERNEL.iter().map(|&x| x as u64).collect::<Vec<_>>()) {
        out.push("kernel sizes differ from A2's".into());
    }
    if ints(&l["dilations"]).is_some_and(|d| d != DILATION) {
        out.push("dilations differ from A2's".into());
    }
    for (i, a) in l["activation"].as_array().map(Vec::as_slice).unwrap_or_default().iter().enumerate() {
        let slope = a["negative_slope"].as_f64().unwrap_or(0.0);
        if a["type"].as_str() != Some("LeakyReLU") || (slope - 0.01).abs() > 1e-7 {
            out.push(format!("layer {} uses {}; the kernel has LeakyReLU 0.01", i + 1, a["type"].as_str().unwrap_or("?")));
        }
    }
    match &l["gating_mode"] {
        Json::Null => {}
        Json::Array(g) => {
            for (i, mode) in g.iter().enumerate().filter(|(_, g)| g.as_str() != Some("none")) {
                out.push(format!("layer {} is gated ({mode})", i + 1));
            }
            if g.len() != n {
                out.push("not one gating mode per layer".into());
            }
        }
        _ => out.push("not one gating mode per layer".into()),
    }
    let weights = m["weights"].as_array().map(Vec::len).unwrap_or(0);
    if weights != N_WEIGHTS {
        out.push(format!("{weights} weights; A2 Lite has {N_WEIGHTS}"));
    }
    out
}

struct Layers<T> {
    blocks: Vec<Vec<T>>,
    head: Vec<T>,
    rechannel: Vec<T>,
    scale: T,
}

/// Per layer: bias and mix-in per output channel, the taps `[tap][input][output]`, then per
/// output channel bias1 and its 1x1 weights; the head (bias, then `[tap][channel]`); the
/// rechannel weights; the head scale (HXtra tools/sharc/nam_lite.py `layer_weights`).
fn layer_weights<T: Copy>(w: &[T]) -> Layers<T> {
    let mut o = C;
    let mut blocks = Vec::with_capacity(KERNEL.len());
    for k in KERNEL {
        let conv = o;
        o += C * C * k;
        let (bias, mixin) = (o, o + C);
        o += 2 * C;
        let w1 = o;
        o += C * C;
        let b1 = o;
        o += C;
        let mut blk = Vec::new();
        for i in 0..C {
            blk.extend([w[bias + i], w[mixin + i]]);
        }
        for t in 0..k {
            for j in 0..C {
                for i in 0..C {
                    blk.push(w[conv + (i * C + j) * k + t]);
                }
            }
        }
        for i in 0..C {
            blk.push(w[b1 + i]);
            blk.extend((0..C).map(|j| w[w1 + i * C + j]));
        }
        blocks.push(blk);
    }
    let mut head = vec![w[o + C * HEAD_TAPS]];
    for t in 0..HEAD_TAPS {
        head.extend((0..C).map(|j| w[o + j * HEAD_TAPS + t]));
    }
    Layers { blocks, head, rechannel: w[..C].to_vec(), scale: w[o + C * HEAD_TAPS + 1] }
}

/// JavaScript's `Math.max` on two floats, zero signs included (+0 wins over -0).
fn js_max(a: f32, b: f32) -> f32 {
    match (a.is_nan() || b.is_nan(), a == 0.0 && b == 0.0) {
        (true, _) => f32::NAN,
        (false, true) if a.is_sign_positive() || b.is_sign_positive() => 0.0,
        (false, true) => -0.0,
        (false, false) if a > b => a,
        (false, false) => b,
    }
}

/// What the kernel's rings hold after silence, in float32 op for op: the head's input (3) and
/// each layer's input (23 x 3), so that a change of capture does not pop (nam_lite.py `silence`).
fn silence(w: &[f64]) -> Vec<f32> {
    let w32: Vec<f32> = w.iter().map(|&x| x as f32).collect();
    let lw = layer_weights(&w32);
    let mut x: Vec<f32> = lw.rechannel.iter().map(|r| r * 0.0).collect();
    let mut hs = [0.0f32; C];
    let mut xs = Vec::with_capacity(KERNEL.len() * C);
    for (n, &k) in KERNEL.iter().enumerate() {
        let blk = &lw.blocks[n];
        xs.extend_from_slice(&x);
        let mut a = [0.0f32; C];
        for (i, ai) in a.iter_mut().enumerate() {
            let mut v = (blk[2 * i] + blk[2 * i + 1] * 0.0) + 0.0;
            for t in 0..k {
                for (j, xj) in x.iter().enumerate() {
                    v += blk[2 * C + (t * C + j) * C + i] * xj;
                }
            }
            *ai = js_max(v, v * 0.01);
        }
        for (h, ai) in hs.iter_mut().zip(a) {
            *h += ai;
        }
        let o = 2 * C + k * C * C;
        for (i, xi) in x.iter_mut().enumerate() {
            let mut u = blk[o + 4 * i];
            for (j, aj) in a.iter().enumerate() {
                u += aj * blk[o + 4 * i + 1 + j];
            }
            *xi += u;
        }
    }
    hs.into_iter().chain(xs).collect()
}

fn loud_word(db: Option<f64>) -> f64 {
    let g = db.map_or(0.0, |db| f64::from((10f64.powf((LOUD_REF - db) / 20.0)) as f32));
    match g > 0.0 && g < IR_LIMIT {
        true => g,
        false => 0.0,
    }
}

fn level_word(dbu: Option<f64>) -> Result<f64> {
    match dbu {
        None => Ok(0.0),
        Some(dbu) => {
            let g = f64::from((10f64.powf(-dbu / 20.0)) as f32);
            match g > 0.0 && g < IR_LIMIT {
                true => Ok(g),
                false => Err(bad(&format!("input level {dbu} dBu is out of range (above -24 dBu)"))),
            }
        }
    }
}

fn ir_samples(w: &[f64], dbu: Option<f64>, loud: Option<f64>) -> Result<(Vec<f32>, u32)> {
    let lw = layer_weights(w);
    let mut stream: Vec<f64> = lw.rechannel.iter().chain(&lw.head).copied().chain([lw.scale]).collect();
    stream.extend(lw.blocks.iter().flatten());
    stream.extend(silence(w).into_iter().map(|v| f64::from(v) / 16.0));
    let peak = stream.iter().fold(0.0f64, |p, x| p.max(x.abs()));
    let exponent = (0..64).find(|&e| peak / 2f64.powi(e) <= PEAK_LIMIT).ok_or_else(|| bad("weights too large to scale"))?;
    let s = 2f64.powi(exponent);
    let head = [0.5, -0.5, C as f64, s, N_WEIGHTS as f64 / 1024.0, level_word(dbu)?, loud_word(loud), 0.0];
    let vals: Vec<f64> = head.into_iter().chain(stream.iter().map(|x| x / s)).collect();
    if vals.len() >= IR_SAMPLES {
        return Err(bad("too many weights for one IR slot"));
    }
    if let Some(v) = vals.iter().find(|v| v.is_nan() || v.abs() >= IR_LIMIT) {
        return Err(bad(&format!("a sample the unit would refuse ({v})")));
    }
    let mut samples = vec![0.0f32; IR_SAMPLES];
    for (s, v) in samples.iter_mut().zip(&vals) {
        *s = *v as f32;
    }
    Ok((samples, exponent as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer() -> Json {
        serde_json::json!({
            "input_size": 1, "condition_size": 1, "channels": 3, "bottleneck": 3,
            "kernel_sizes": KERNEL, "dilations": DILATION,
            "activation": vec![serde_json::json!({"type": "LeakyReLU", "negative_slope": 0.01}); 23],
            "gating_mode": vec!["none"; 23],
            "layer1x1": {"active": true, "groups": 1}, "head1x1": {"active": false},
            "head": {"out_channels": 1, "kernel_size": 16, "bias": true}
        })
    }

    fn model(weights: usize) -> Json {
        let w: Vec<f64> = (0..weights).map(|i| ((i * 37 % 101) as f64 - 50.0) / 40.0).collect();
        serde_json::json!({
            "architecture": "WaveNet", "sample_rate": 48000,
            "config": {"layers": [layer()], "head": null, "head_scale": 0.02},
            "weights": w, "metadata": {"name": "Test", "input_level_dbu": 12.0, "loudness": -20.0}
        })
    }

    #[test]
    fn converts_an_a2_lite_capture_into_a_header_the_unit_recognises() {
        let c = convert(&model(N_WEIGHTS).to_string()).unwrap();
        assert_eq!(c.samples.len(), IR_SAMPLES);
        assert_eq!(&c.samples[..3], &[0.5, -0.5, 3.0]);
        assert_eq!(c.name.as_deref(), Some("Test"));
        assert!(is_capture(&c.ir_bytes()));
        assert!(c.samples[8 + N_WEIGHTS + 72..].iter().all(|&s| s == 0.0));
    }

    #[test]
    fn refuses_other_shapes_and_rates() {
        assert!(matches!(convert(&model(100).to_string()), Err(Error::Nam(e)) if e.contains("100 weights")));
        let mut m = model(N_WEIGHTS);
        m["sample_rate"] = serde_json::json!(44100);
        assert!(matches!(convert(&m.to_string()), Err(Error::Nam(e)) if e.contains("44100")));
        assert!(convert("{}").is_err() && convert("not json").is_err());
    }

    #[test]
    fn a_plain_ir_is_not_a_capture() {
        let ir: Vec<u8> = [0.003f32, 0.0227, 0.0883, 0.2321, 0.4589].iter().flat_map(|s| s.to_le_bytes()).collect();
        assert!(!is_capture(&ir));
    }
}
