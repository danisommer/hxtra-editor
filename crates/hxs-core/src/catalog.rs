//! The model catalog, `hxs-catalog.json` (docs/catalog-format.md): the firmware's models and
//! parameters, extracted from the user's own firmware file. It ships next to the executable or
//! sits in the user's data folder; without it everything still works, with numbers for names.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rmpv::Value;
use serde::Deserialize;

use crate::{Error, Result};

pub const FORMAT: &str = "hxs-catalog";
pub const FORMAT_VERSION: u32 = 1;
pub const FILE_NAME: &str = "hxs-catalog.json";
pub const ENV_VAR: &str = "HXS_CATALOG";
const APP_FOLDER: &str = "hxtra-editor";

#[derive(Debug, Clone, Deserialize)]
pub struct Catalog {
    pub format: String,
    pub version: u32,
    pub source: Source,
    pub models: Vec<Model>,
    #[serde(default)]
    pub browser: Vec<Folder>,
    #[serde(skip)]
    by_preset_id: HashMap<u64, usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Source {
    pub md5: String,
    pub product: Option<String>,
    pub device_id: Option<u64>,
    pub firmware: Option<u64>,
    pub stock_md5: Option<String>,
    pub hxtra: Option<Hxtra>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Hxtra {
    pub features: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Model {
    pub preset_id: u64,
    pub model_id: u64,
    pub name: String,
    pub title: String,
    pub stock_title: Option<String>,
    pub real_name: Option<String>,
    pub category: Option<String>,
    pub subcategory: Option<String>,
    pub legacy: Option<bool>,
    pub dsp_cost: Option<u64>,
    pub hxtra_added: Option<bool>,
    #[serde(default)]
    pub role: Option<String>,
    pub params: Vec<Param>,
}

/// The DSP cost every preset pays before its blocks, and the most a preset may total
/// (HXtra docs/research/m4/dsp-cost.md); costs are hundredths of a per cent of the DSP.
pub const DSP_FIXED: u64 = 1500;
pub const DSP_LIMIT: u64 = 9000;

pub const ROLE_CAPTURE: &str = "capture";
pub const ROLE_CAPTURE_SLOT: &str = "capture_slot";

#[derive(Debug, Clone, Deserialize)]
pub struct Param {
    pub index: usize,
    pub slot: u64,
    pub name: String,
    pub title: String,
    pub short_title: Option<String>,
    #[serde(rename = "type")]
    pub kind: ParamKind,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub unit: Option<String>,
    pub display: Option<String>,
    pub choices: Option<Vec<Choice>>,
    pub visible: bool,
    #[serde(default)]
    pub sync: bool,
    pub hxtra_added: Option<bool>,
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamKind {
    Float,
    Int,
    List,
    Bool,
    Data,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Choice {
    pub value: i64,
    pub label: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Folder {
    pub category: Option<String>,
    pub category_index: u64,
    pub folder: String,
    pub folder_index: u64,
    pub items: Vec<FolderItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FolderItem {
    pub title: String,
    pub kind: u64,
    pub model: Option<u64>,
    pub cab: Option<u64>,
}

/// Which of a model's names to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NameStyle {
    /// What the unit shows (real-world names when the HXtra build has them).
    #[default]
    Unit,
    /// Line 6's names.
    Line6,
    /// HXtra's real-world names where it has one, else the unit's.
    Real,
}

impl Catalog {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut c: Catalog = serde_json::from_slice(bytes)?;
        if c.format != FORMAT || c.version != FORMAT_VERSION {
            return Err(Error::Catalog(format!("this is a {} v{} file; this build reads {FORMAT} v{FORMAT_VERSION}", c.format, c.version)));
        }
        c.by_preset_id = c.models.iter().enumerate().map(|(i, m)| (m.preset_id, i)).collect();
        Ok(c)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path).map_err(|e| Error::Catalog(format!("reading {}: {e}", path.display())))?;
        Self::parse(&bytes).map_err(|e| Error::Catalog(format!("{}: {e}", path.display())))
    }

    /// The catalog to use and where it came from: `explicit`, else `HXS_CATALOG` (either must
    /// load), else the first file found next to the executable or in the user's data folder.
    /// `Ok(None)` when no catalog is set up.
    pub fn find(explicit: Option<&Path>) -> Result<Option<(PathBuf, Self)>> {
        let chosen = explicit.map(Path::to_path_buf).or_else(|| std::env::var_os(ENV_VAR).map(PathBuf::from));
        let path = match chosen {
            Some(p) => Some(p),
            None => {
                let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
                [exe_dir, data_folder()].into_iter().flatten().map(|d| d.join(FILE_NAME)).find(|p| p.is_file())
            }
        };
        path.map(|p| Self::load(&p).map(|c| (p, c))).transpose()
    }

    pub fn model(&self, preset_id: u64) -> Option<&Model> {
        self.by_preset_id.get(&preset_id).map(|&i| &self.models[i])
    }

    /// The block kind to add `preset_id` (with `cab` for an amp + cab) as: the model browser's
    /// item kind (17 an amp, 33 an amp + cab, 8 a delay...), which presets keep as key 9.
    pub fn block_kind(&self, preset_id: u64, cab: Option<u64>) -> Option<u64> {
        let items = self.browser.iter().flat_map(|f| &f.items);
        let mut matching = items.filter(|i| i.model == Some(preset_id) && i.cab == cab);
        matching.next().map(|i| i.kind)
    }

    /// The cab the model browser pairs with an amp in its Amp+Cab folders.
    pub fn default_cab(&self, amp: u64) -> Option<u64> {
        self.browser.iter().flat_map(|f| &f.items).find(|i| i.model == Some(amp) && i.cab.is_some()).and_then(|i| i.cab)
    }

    /// A preset's DSP use: the fixed part plus each block's cost, and the blocks whose model
    /// the catalog does not price (their cost is left out of the total).
    pub fn dsp_total(&self, preset_ids: &[u64]) -> (u64, Vec<u64>) {
        let (priced, unknown): (Vec<_>, Vec<_>) = preset_ids.iter().map(|&id| (id, self.model(id).and_then(|m| m.dsp_cost))).partition(|(_, c)| c.is_some());
        let total = DSP_FIXED + priced.iter().filter_map(|(_, c)| *c).sum::<u64>();
        (total, unknown.into_iter().map(|(id, _)| id).collect())
    }

    /// The other models that are the same effect: its mono and stereo versions, and an amp's
    /// preamp (or the reverse), by internal name.
    pub fn variants(&self, of: &Model) -> Vec<&Model> {
        let key = variant_key(&of.name);
        self.models.iter().filter(|m| m.preset_id != of.preset_id && variant_key(&m.name) == key).collect()
    }

    /// Refuse a catalog made from another firmware version than the unit's (from its preset files).
    pub fn check_firmware(&self, unit_firmware: u64) -> Result<()> {
        match self.source.firmware {
            Some(f) if f != unit_firmware => {
                Err(Error::Catalog(format!("the catalog is for firmware {f:#010x}, the unit runs {unit_firmware:#010x}")))
            }
            _ => Ok(()),
        }
    }
}

impl Model {
    pub fn display_name(&self, style: NameStyle) -> &str {
        match style {
            NameStyle::Unit => &self.title,
            NameStyle::Line6 => self.stock_title.as_deref().unwrap_or(&self.title),
            NameStyle::Real => self.real_name.as_deref().unwrap_or(&self.title),
        }
    }

    pub fn is_capture(&self) -> bool {
        self.role.as_deref() == Some(ROLE_CAPTURE)
    }

    /// The category to file the model under: `capture` for captures, else the firmware's.
    pub fn shown_category(&self) -> &str {
        match self.is_capture() {
            true => ROLE_CAPTURE,
            false => self.category.as_deref().unwrap_or("?"),
        }
    }

    pub fn param(&self, index: usize) -> Option<&Param> {
        self.params.iter().find(|p| p.index == index)
    }

    /// A parameter by its title, short title or internal name, ignoring case.
    pub fn param_named(&self, name: &str) -> Option<&Param> {
        let n = name.to_lowercase();
        self.params.iter().find(|p| {
            p.title.to_lowercase() == n || p.short_title.as_deref().is_some_and(|s| s.to_lowercase() == n) || p.name.to_lowercase() == n
        })
    }
}

impl Param {
    /// `text` as this parameter's value, in its type and range: a number, or a list label.
    pub fn value_for(&self, text: &str) -> Result<Value> {
        let shown = |v: f64| self.format(&Value::from(v));
        let out_of_range = |v: f64| Error::Catalog(format!("{} takes {}..{}, not {}", self.title, shown(self.min), shown(self.max), shown(v)));
        match self.kind {
            ParamKind::Float => {
                let v: f64 = text.parse().map_err(|_| Error::Catalog(format!("{} takes a number, not {text}", self.title)))?;
                match (self.min..=self.max).contains(&v) {
                    true => Ok(Value::F32(v as f32)),
                    false => Err(out_of_range(v)),
                }
            }
            ParamKind::Int | ParamKind::List => {
                let by_label = self.choices.iter().flatten().find(|c| c.label.eq_ignore_ascii_case(text)).map(|c| c.value);
                let v = match by_label {
                    Some(v) => v,
                    None => text.parse::<i64>().map_err(|_| Error::Catalog(format!("{} takes {}, not {text}", self.title, self.accepted())))?,
                };
                match (self.min..=self.max).contains(&(v as f64)) {
                    true => Ok(Value::from(v)),
                    false => Err(out_of_range(v as f64)),
                }
            }
            ParamKind::Bool => match text.to_ascii_lowercase().as_str() {
                "on" | "true" | "1" => Ok(Value::from(true)),
                "off" | "false" | "0" => Ok(Value::from(false)),
                _ => Err(Error::Catalog(format!("{} is on or off, not {text}", self.title))),
            },
            ParamKind::Data => Err(Error::Catalog(format!("{} is internal data, not a setting", self.title))),
        }
    }

    /// A float typed as the unit shows it ("6.2" on a knob, "74 ms", "85%", "3.7 kHz") in the
    /// parameter's own units, within its range. A bare number on a time reads in the unit now
    /// shown (ms below 1 s, else s). `None` when it is not a number.
    pub fn parse_display(&self, text: &str, current: f64) -> Option<f64> {
        let t = text.trim().to_lowercase();
        let digits = t.find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))).unwrap_or(t.len());
        let n: f64 = t[..digits].parse().ok()?;
        let unit = t[digits..].trim();
        let timed = matches!(self.display.as_deref(), Some("time" | "time_hi_res" | "timeAB" | "rt60")) || self.unit.as_deref() == Some("sec");
        let v = match (unit, self.display.as_deref(), timed) {
            ("ms", _, _) => n / 1000.0,
            ("s", _, _) => n,
            ("khz", _, _) => n * 1000.0,
            ("%", _, _) => n / 100.0,
            (_, Some("knob"), _) => n / 10.0,
            (_, Some("percent"), _) => n / 100.0,
            (_, _, true) if current.abs() < 1.0 => n / 1000.0,
            _ => n,
        };
        Some(v.clamp(self.min.min(self.max), self.max.max(self.min)))
    }

    /// The default as the value type a preset stores for this parameter.
    pub fn default_value(&self) -> Value {
        self.typed(self.default)
    }

    /// `v` in this parameter's type, within its range.
    pub fn clamped(&self, v: &Value) -> Value {
        let n = v.as_f64().or_else(|| v.as_i64().map(|i| i as f64)).or_else(|| v.as_bool().map(|b| f64::from(u8::from(b))));
        match n {
            Some(n) => self.typed(n.clamp(self.min.min(self.max), self.max.max(self.min))),
            None => self.default_value(),
        }
    }

    fn typed(&self, n: f64) -> Value {
        match self.kind {
            ParamKind::Float | ParamKind::Data => Value::F32(n as f32),
            ParamKind::Int | ParamKind::List => Value::from(n.round() as i64),
            ParamKind::Bool => Value::from(n != 0.0),
        }
    }

    fn accepted(&self) -> String {
        match &self.choices {
            Some(c) => c.iter().map(|c| c.label.as_str()).collect::<Vec<_>>().join(", "),
            None => format!("an integer {}..{}", self.min, self.max),
        }
    }

    /// A value as the unit shows it: list labels, On/Off, knobs 0..10, percent, ms or s, dB, Hz.
    /// Display kinds whose on-screen form is not known yet (pan, blend, mic position, pitch) stay raw numbers.
    pub fn format(&self, v: &Value) -> String {
        let number = v.as_f64().or_else(|| v.as_i64().map(|i| i as f64)).or_else(|| v.as_u64().map(|u| u as f64));
        match (self.kind, number, v.as_bool()) {
            (_, Some(n), _) if self.role.as_deref() == Some(ROLE_CAPTURE_SLOT) => format!("capture in IR slot {n:.0}"),
            (_, _, Some(b)) => on_off(b),
            (ParamKind::Bool, Some(n), _) => on_off(n != 0.0),
            (ParamKind::List | ParamKind::Int, Some(n), _) => {
                let label = self.choices.iter().flatten().find(|c| c.value as f64 == n).map(|c| c.label.clone());
                label.unwrap_or_else(|| format!("{n}"))
            }
            (ParamKind::Float, Some(n), _) => float_text(self.display.as_deref(), self.unit.as_deref(), n),
            _ => v.to_string(),
        }
    }
}

/// `HD2_FlangerDynamixFlangerMono` and `...Stereo` share `HD2_FlangerDynamixFlanger`;
/// `HD2_AmpPVPanama` and `HD2_PreampPVPanama` share `HD2_PVPanama`.
fn variant_key(name: &str) -> String {
    let base = name.strip_suffix("Mono").or_else(|| name.strip_suffix("Stereo")).unwrap_or(name);
    match base.split_once('_') {
        Some((prefix, rest)) => {
            let rest = rest.strip_prefix("Preamp").or_else(|| rest.strip_prefix("Amp")).unwrap_or(rest);
            format!("{prefix}_{rest}")
        }
        None => base.to_owned(),
    }
}

/// The values for `to` when a block changes from model `from` (holding `values`): a parameter
/// both models have (same internal name and type) keeps its value, clamped to `to`'s range;
/// every other one takes `to`'s default. An internal data parameter only `to` has has no
/// default to write, which is an error.
pub fn carry_values(from: &Model, values: &[Value], to: &Model) -> Result<Vec<Value>> {
    to.params
        .iter()
        .map(|p| {
            let kept = from.params.iter().find(|f| f.name == p.name && f.kind == p.kind).and_then(|f| values.get(f.index));
            match (kept, p.kind) {
                (Some(v), ParamKind::Data) => Ok(v.clone()),
                (Some(v), _) => Ok(p.clamped(v)),
                (None, ParamKind::Data) => Err(Error::Catalog(format!("{} has internal data ({}) that {} does not carry", to.title, p.title, from.title))),
                (None, _) => Ok(p.default_value()),
            }
        })
        .collect()
}

/// The same values with each one back at `model`'s default (the "restore defaults" of a block).
pub fn default_values(model: &Model, values: &[Value]) -> Vec<Value> {
    model.params.iter().map(|p| match p.kind {
        ParamKind::Data => values.get(p.index).cloned().unwrap_or(Value::Nil),
        _ => p.default_value(),
    }).collect()
}

fn on_off(b: bool) -> String {
    if b { "On" } else { "Off" }.to_owned()
}

fn float_text(display: Option<&str>, unit: Option<&str>, n: f64) -> String {
    match (display, unit) {
        (Some("knob"), _) => format!("{:.1}", n * 10.0),
        (Some("percent"), _) => format!("{:.0}%", n * 100.0),
        (Some("time" | "time_hi_res" | "timeAB" | "rt60"), _) | (_, Some("sec")) => seconds(n),
        (Some("db"), _) | (_, Some("db")) => format!("{n:.1} dB"),
        (Some("hz"), _) if n >= 1000.0 => format!("{:.1} kHz", n / 1000.0),
        (Some("hz"), _) => format!("{n:.0} Hz"),
        (Some("inches"), _) => format!("{n:.1} in"),
        (Some("ratio"), _) => format!("{n:.1}:1"),
        (Some("micAngle"), _) => format!("{n:.0}°"),
        (Some("tenths"), _) => format!("{n:.1}"),
        _ => format!("{n:.2}"),
    }
}

fn seconds(n: f64) -> String {
    match n.abs() < 1.0 {
        true => format!("{:.1} ms", n * 1000.0),
        false => format!("{n:.2} s"),
    }
}

/// `%APPDATA%\hxtra-editor` on Windows, `$XDG_DATA_HOME/hxtra-editor` or `~/.local/share/hxtra-editor` elsewhere.
pub fn data_folder() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_DATA_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))?;
    Some(base.join(APP_FOLDER))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "format": "hxs-catalog", "version": 1, "generator": {"tool": "t", "version": "1"},
      "source": {"md5": "00", "product": "HX Stomp", "device_id": 2162694, "firmware": 58720256, "stock_md5": null, "hxtra": null},
      "models": [{
        "preset_id": 124, "model_id": 68, "name": "HD2_Comp", "title": "MXR Comp", "stock_title": "Comp", "real_name": "MXR Comp",
        "category": "dynamics", "subcategory": "stereo", "legacy": null, "dsp_cost": 297, "hxtra_added": false,
        "params": [
          {"index": 0, "slot": 1, "name": "Threshold", "title": "Threshold", "short_title": "Thresh", "type": "float",
           "min": -60.0, "max": 0.0, "default": -37.1, "unit": "db", "display": "db", "choices": null, "visible": true, "sync": false},
          {"index": 1, "slot": 2, "name": "Ratio", "title": "Ratio", "short_title": null, "type": "list", "min": 0, "max": 5,
           "default": 3, "unit": null, "display": "list", "choices": [{"value": 0, "label": "2:1"}, {"value": 1, "label": "3:1"}],
           "visible": true, "sync": false},
          {"index": 2, "slot": 3, "name": "Attack", "title": "Attack", "short_title": null, "type": "float", "min": 0.0001,
           "max": 0.2, "default": 0.038, "unit": "sec", "display": "time", "choices": null, "visible": true, "sync": false},
          {"index": 3, "slot": 8, "name": "GRPosition", "title": "Mtr Position", "short_title": null, "type": "list", "min": 0,
           "max": 4, "default": 0, "unit": null, "display": "list", "choices": null, "visible": true, "sync": false, "hxtra_added": true}
        ]}],
      "browser": []
    }"#;

    #[test]
    fn parses_and_indexes_by_preset_id() {
        let c = Catalog::parse(SAMPLE.as_bytes()).unwrap();
        let m = c.model(124).unwrap();
        assert_eq!((m.display_name(NameStyle::Unit), m.display_name(NameStyle::Line6)), ("MXR Comp", "Comp"));
        assert_eq!(m.param_named("thresh").map(|p| p.index), Some(0));
        assert_eq!(m.param(3).and_then(|p| p.hxtra_added), Some(true));
        assert!(c.model(1).is_none());
        assert!(c.check_firmware(0x0380_0000).is_ok() && c.check_firmware(0x0371_0000).is_err());
    }

    #[test]
    fn values_are_typed_and_range_checked() {
        let c = Catalog::parse(SAMPLE.as_bytes()).unwrap();
        let m = c.model(124).unwrap();
        assert_eq!(m.param(2).unwrap().value_for("0.074").unwrap(), Value::F32(0.074));
        let above = m.param(2).unwrap().value_for("0.5").unwrap_err().to_string();
        assert_eq!(above, "catalog: Attack takes 0.1 ms..200.0 ms, not 500.0 ms");
        assert_eq!(m.param(1).unwrap().value_for("3:1").unwrap(), Value::from(1));
        assert_eq!(m.param(1).unwrap().value_for("2").unwrap(), Value::from(2));
        assert!(m.param(1).unwrap().value_for("9").is_err());
    }

    #[test]
    fn values_print_as_the_unit_shows_them() {
        let c = Catalog::parse(SAMPLE.as_bytes()).unwrap();
        let m = c.model(124).unwrap();
        assert_eq!(m.param(2).unwrap().format(&Value::F32(0.027)), "27.0 ms");
        assert_eq!(m.param(0).unwrap().format(&Value::F32(-27.5)), "-27.5 dB");
        assert_eq!(m.param(1).unwrap().format(&Value::from(1)), "3:1");
    }

    fn flanger(id: u64, name: &str, extra: bool) -> serde_json::Value {
        let p = |i: usize, n: &str, kind: &str, min: f64, max: f64, def: f64| {
            serde_json::json!({"index": i, "slot": i + 1, "name": n, "title": n, "short_title": null, "type": kind, "min": min, "max": max,
                "default": def, "unit": null, "display": "knob", "choices": null, "visible": true, "sync": false})
        };
        let mut params = vec![p(0, "Rate", "float", 0.0, 1.0, 0.5), p(1, "Mode", "list", 0.0, 2.0, 0.0)];
        if extra {
            params.push(p(2, "Spread", "float", 0.0, 1.0, 0.25));
        }
        serde_json::json!({"preset_id": id, "model_id": id, "name": name, "title": "Flanger", "stock_title": null, "real_name": null,
            "category": "modulation", "subcategory": null, "legacy": null, "dsp_cost": 700, "hxtra_added": false, "params": params})
    }

    fn two_flangers() -> Catalog {
        let mut c: serde_json::Value = serde_json::from_str(SAMPLE).unwrap();
        c["models"] = serde_json::json!([flanger(304, "HD2_FlangerDynamixFlangerMono", false), flanger(305, "HD2_FlangerDynamixFlangerStereo", true)]);
        Catalog::parse(c.to_string().as_bytes()).unwrap()
    }

    #[test]
    fn a_variant_keeps_the_values_both_models_have() {
        let c = two_flangers();
        let (mono, stereo) = (c.model(304).unwrap(), c.model(305).unwrap());
        assert_eq!(c.variants(mono).iter().map(|m| m.preset_id).collect::<Vec<_>>(), vec![305]);
        let to_stereo = carry_values(mono, &[Value::F32(0.8), Value::from(2)], stereo).unwrap();
        assert_eq!(to_stereo, vec![Value::F32(0.8), Value::from(2), Value::F32(0.25)], "Spread takes the stereo default");
        let back = carry_values(stereo, &[Value::F32(1.7), Value::from(1), Value::F32(0.9)], mono).unwrap();
        assert_eq!(back, vec![Value::F32(1.0), Value::from(1)], "clamped to the range, Spread dropped");
        assert_eq!(default_values(stereo, &to_stereo), vec![Value::F32(0.5), Value::from(0), Value::F32(0.25)]);
        assert_eq!(variant_key("HD2_AmpPVPanama"), variant_key("HD2_PreampPVPanama"));
    }

    #[test]
    fn dsp_total_is_the_fixed_part_plus_known_blocks() {
        let c = two_flangers();
        assert_eq!(c.dsp_total(&[304, 305]), (DSP_FIXED + 1400, vec![]));
        assert_eq!(c.dsp_total(&[304, 999]), (DSP_FIXED + 700, vec![999]));
    }

    #[test]
    fn typed_values_read_in_the_units_shown() {
        let c = Catalog::parse(SAMPLE.as_bytes()).unwrap();
        let m = c.model(124).unwrap();
        let (threshold, attack) = (m.param(0).unwrap(), m.param(2).unwrap());
        assert_eq!(attack.parse_display("74 ms", 0.027), Some(0.074));
        assert_eq!(attack.parse_display("74", 0.027), Some(0.074), "a bare number on a time shown in ms");
        assert_eq!(attack.parse_display("0.1 s", 0.027), Some(0.1));
        assert_eq!(attack.parse_display("900 ms", 0.027), Some(0.2), "clamped to the range");
        assert_eq!(threshold.parse_display("-12 dB", -27.5), Some(-12.0));
        assert_eq!(threshold.parse_display("abc", -27.5), None);
    }

    #[test]
    fn refuses_another_format_version() {
        let other = SAMPLE.replace("\"version\": 1", "\"version\": 2");
        assert!(matches!(Catalog::parse(other.as_bytes()), Err(Error::Catalog(_))));
    }
}
