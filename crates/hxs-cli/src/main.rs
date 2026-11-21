use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use hxs_core::backup;
use hxs_core::catalog::{self, Catalog, Model, NameStyle};
use hxs_core::device::{DeviceHandle, Event, Options, snapshot_number};
use hxs_core::notify::Notification;
use hxs_core::globals::Globals;
use hxs_core::preset::{Block, PresetFile};
use rmpv::Value;
use hxs_core::restore;
use hxs_core::snapshot::{self, Bank};
use hxs_core::stomp::ListFlags;
use hxs_core::stomp::Stomp;

/// Read-only tools for an HX Stomp running HXtra (close HX Edit first).
#[derive(Parser)]
#[command(name = "hxs", version)]
struct Cli {
    /// Seconds to wait for each reply from the device.
    #[arg(long, default_value_t = 5.0, global = true)]
    timeout: f64,
    /// The model catalog (default: $HXS_CATALOG, else hxs-catalog.json next to hxs or in the data folder).
    #[arg(long, global = true)]
    catalog: Option<PathBuf>,
    /// Which model names to print: as on the unit, Line 6's, or HXtra's real-world names.
    #[arg(long, value_enum, default_value_t = NameArg::Unit, global = true)]
    names: NameArg,
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, ValueEnum)]
enum NameArg {
    Unit,
    Line6,
    Real,
}

impl From<NameArg> for NameStyle {
    fn from(a: NameArg) -> Self {
        match a {
            NameArg::Unit => NameStyle::Unit,
            NameArg::Line6 => NameStyle::Line6,
            NameArg::Real => NameStyle::Real,
        }
    }
}

#[derive(Subcommand)]
enum Command {
    /// Show a stored preset's blocks with model and parameter names (from the catalog) and its snapshots.
    Show {
        slot: usize,
        #[arg(long, default_value_t = 0)]
        setlist: u64,
        /// Also print the parameters the unit does not show (tempo-sync divisions and the like).
        #[arg(long)]
        hidden: bool,
    },
    /// Describe the model catalog in use, or list the models whose names contain SEARCH.
    Catalog { search: Option<String> },
    /// Show the device, its setlists and the current preset.
    Info,
    /// List the presets of a setlist in slot order.
    Presets {
        #[arg(long, default_value_t = 0)]
        setlist: u64,
    },
    /// Back up every preset (with snapshots 4..6), IR, favorite and the globals into a new folder.
    Backup {
        dir: PathBuf,
        #[arg(long, default_value_t = 0)]
        setlist: u64,
    },
    /// Describe a preset file from a backup (no device needed).
    Inspect {
        file: PathBuf,
        /// Also print the parameters the unit does not show.
        #[arg(long)]
        hidden: bool,
    },
    /// Write a preset file from a backup into one slot (a dry run unless --yes), then read it back.
    RestorePreset {
        file: PathBuf,
        #[arg(long)]
        slot: usize,
        /// The name the slot gets (up to 16 printable ASCII characters).
        #[arg(long)]
        name: String,
        #[arg(long, default_value_t = 0)]
        setlist: u64,
        /// Really overwrite the slot.
        #[arg(long)]
        yes: bool,
    },
    /// Empty an IR slot (numbered from 1, as the unit shows them); a dry run unless --yes.
    DeleteIr {
        number: u64,
        /// Really empty the slot.
        #[arg(long)]
        yes: bool,
    },
    /// Print the unit's events live; type `select`, `set`, `param`, `block`, `snapshot` or `quit`.
    Monitor {
        /// Stop after this many seconds (0: until `quit`).
        #[arg(long, default_value_t = 0)]
        seconds: u64,
        /// Print every setting change (by default each setting prints at most once a second).
        #[arg(long)]
        all: bool,
        /// Do not read commands from standard input.
        #[arg(long)]
        no_input: bool,
        /// The unit's MIDI base channel (for `snapshot`, sent as MIDI CC 69).
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=16))]
        midi_channel: u8,
    },
    /// Restore a whole backup: IRs, presets, favorites and global settings that differ
    /// from the unit (a dry run unless --yes), then compare again.
    Restore {
        dir: PathBuf,
        /// Really write what differs.
        #[arg(long)]
        yes: bool,
    },
    /// Write the global settings of a backup (a dry run unless --yes), then read them back.
    RestoreGlobals {
        /// A backup folder, or its globals.bin.
        path: PathBuf,
        /// Really write the settings.
        #[arg(long)]
        yes: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let timeout = Duration::from_secs_f64(cli.timeout);
    let labels = || Labels::load(cli.catalog.as_deref(), cli.names.into());
    match cli.command {
        Command::Info => info(timeout),
        Command::Presets { setlist } => presets(timeout, setlist),
        Command::Show { slot, setlist, hidden } => show(timeout, slot, setlist, labels()?, hidden),
        Command::Catalog { ref search } => describe_catalog(&labels()?, search.as_deref()),
        Command::Backup { dir, setlist } => run_backup(timeout, &dir, setlist),
        Command::Inspect { ref file, hidden } => inspect(file, labels()?, hidden),
        Command::RestorePreset { file, slot, name, setlist, yes } => restore_preset(timeout, &file, slot, &name, setlist, yes),
        Command::RestoreGlobals { path, yes } => restore_globals(timeout, &path, yes),
        Command::Restore { dir, yes } => restore_all(timeout, &dir, yes),
        Command::Monitor { seconds, all, no_input, midi_channel } => monitor(timeout, labels()?, seconds, all, no_input, midi_channel - 1),
        Command::DeleteIr { number, yes } => delete_ir(timeout, number, yes),
    }
}

fn delete_ir(timeout: Duration, number: u64, yes: bool) -> Result<()> {
    let slot = number.checked_sub(1).context("IR slots are numbered from 1")?;
    let mut stomp = connect(timeout)?;
    let ir = stomp.ir_list()?.into_iter().find(|i| i.index == slot).with_context(|| format!("IR {number:03} is already empty"))?;
    println!("IR {number:03} \"{}\" would be emptied", ir.name);
    if !yes {
        println!("dry run: nothing written (add --yes to empty the slot)");
        return Ok(());
    }
    stomp.batch_begin()?;
    let deleted = stomp.delete_ir(slot);
    let ended = stomp.batch_end();
    deleted.context("emptying the IR slot")?;
    ended?;
    match stomp.ir_list()?.iter().any(|i| i.index == slot) {
        true => anyhow::bail!("IR {number:03} is still there after the delete"),
        false => println!("checked: IR {number:03} is empty"),
    }
    Ok(())
}

fn monitor(timeout: Duration, labels: Labels, seconds: u64, all: bool, no_input: bool, midi_channel: u8) -> Result<()> {
    let (handle, events) = DeviceHandle::usb(timeout, Options { midi_channel });
    let snapshot = handle.connect().wait().context("connecting to the HX Stomp")?;
    let globals = Globals::parse(snapshot.globals.clone())?;
    let edit = PresetFile::parse(snapshot.edit_buffer.clone()).context("reading the edit buffer")?;
    let labels = Arc::new(labels.for_firmware(edit.device_meta().map(|m| m.firmware)));
    let mut blocks = edit.blocks()?;
    let start = Instant::now();
    println!(
        "connected (USB {:04x}): setlist {} slot {:03} \"{}\"{}, {} presets",
        snapshot.product_id,
        snapshot.current.setlist,
        snapshot.current.slot,
        snapshot.current.name,
        if snapshot.current.modified { " (modified)" } else { "" },
        snapshot.presets.len()
    );
    let quit = Arc::new(AtomicBool::new(false));
    if !no_input {
        println!("commands: {MONITOR_COMMANDS}");
        let (handle, quit, setlist, labels) = (handle.clone(), quit.clone(), snapshot.current.setlist, labels.clone());
        thread::spawn(move || read_commands(&handle, &globals, &labels, setlist, &quit));
    }
    let deadline = (seconds > 0).then(|| start + Duration::from_secs(seconds));
    let mut limiter = RateLimit::new(all);
    while !quit.load(Ordering::Relaxed) && deadline.is_none_or(|d| Instant::now() < d) {
        let Ok(event) = events.recv_timeout(Duration::from_millis(100)) else { continue };
        let t = start.elapsed().as_secs_f64();
        match &event {
            Event::SettingChanged { id, value } => {
                if limiter.allows(format!("setting {id}")) {
                    println!("{t:9.3}  setting {id} = {}", value_text(value));
                }
            }
            Event::PresetLoaded { current, edit_buffer } => {
                println!("{t:9.3}  preset loaded: setlist {} slot {:03} \"{}\" ({} bytes)", current.setlist, current.slot, current.name, edit_buffer.len());
                match PresetFile::parse(edit_buffer.clone()).and_then(|p| p.blocks()) {
                    Ok(b) => blocks = b,
                    Err(e) => println!("{t:9.3}  problem: reading the new edit buffer's blocks: {e}"),
                }
            }
            Event::PresetRenamed { setlist, slot, name } => println!("{t:9.3}  preset renamed: setlist {setlist} slot {slot:03} \"{name}\""),
            Event::Notified(Notification::Unknown { kind, note }) => {
                if limiter.allows(format!("unknown {}", signature(*kind, note))) {
                    println!("{t:9.3}  UNKNOWN notification {}: {note}", signature(*kind, note));
                }
            }
            Event::Notified(Notification::ParameterChanged { block, path, index, value }) => {
                if limiter.allows(format!("parameter {block}/{path}/{index}")) {
                    let named = (*path == 0)
                        .then(|| usize::try_from(*block).ok())
                        .flatten()
                        .and_then(|b| blocks.iter().find(|x| x.position == b))
                        .and_then(|b| labels.param_text(b, usize::try_from(*index).ok()?, value));
                    match named {
                        Some(text) => println!("{t:9.3}  parameter {text}"),
                        None => println!("{t:9.3}  parameter block {block} path {path} index {index} = {}", value_text(value)),
                    }
                }
            }
            Event::Notified(n) => println!("{t:9.3}  {n:?}"),
            Event::Problem { message } => println!("{t:9.3}  problem: {message}"),
            Event::Disconnected { reason } => {
                println!("{t:9.3}  disconnected: {reason}");
                break;
            }
            Event::SnapshotSelected { index, bank } => match snapshot_number(*index, *bank) {
                Some(n) => println!("{t:9.3}  snapshot {n} (bank {bank:?})", bank = bank.expect("a number implies a bank")),
                None => println!("{t:9.3}  snapshot index {index} (bank unknown: {} or {})", index + 1, index + 4),
            },
            Event::Connected(_) => {}
        }
    }
    if let Some(summary) = limiter.summary() {
        println!("not printed (each kind at most once a second; --all prints them): {summary}");
    }
    handle.disconnect().wait()?;
    handle.shutdown();
    Ok(())
}

/// Prints each kind of line at most once a second (unless `all`) and counts the rest.
struct RateLimit {
    all: bool,
    last: HashMap<String, Instant>,
    hidden: BTreeMap<String, u64>,
}

impl RateLimit {
    fn new(all: bool) -> Self {
        Self { all, last: HashMap::new(), hidden: BTreeMap::new() }
    }

    fn allows(&mut self, key: String) -> bool {
        let recent = self.last.get(&key).is_some_and(|t| t.elapsed() < Duration::from_secs(1));
        match self.all || !recent {
            true => {
                self.last.insert(key, Instant::now());
                true
            }
            false => {
                *self.hidden.entry(key).or_default() += 1;
                false
            }
        }
    }

    fn summary(&self) -> Option<String> {
        (!self.hidden.is_empty()).then(|| self.hidden.iter().map(|(k, n)| format!("{k} x{n}")).collect::<Vec<_>>().join(", "))
    }
}

/// `kind/0x44/0x79` of a notification, the fields that tell its sub-kinds apart.
fn signature(kind: Option<u64>, note: &Value) -> String {
    let body = hxs_core::msg::notification(note).map(|(_, b)| b);
    let field = |k| body.and_then(|b| hxs_core::msg::get(b, k)).and_then(Value::as_u64).map_or("?".into(), |v| format!("{v:#x}"));
    format!("kind {}/{}/{}", kind.map_or("?".into(), |k| format!("{k:#04x}")), field(0x44), field(0x79))
}

fn read_commands(handle: &DeviceHandle, globals: &Globals, labels: &Labels, setlist: u64, quit: &AtomicBool) {
    for line in std::io::stdin().lines() {
        let Ok(line) = line else { break };
        let line = line.trim_start_matches('\u{feff}');
        let words: Vec<&str> = line.split_whitespace().collect();
        let outcome = match words.as_slice() {
            ["quit"] | ["exit"] => {
                quit.store(true, Ordering::Relaxed);
                return;
            }
            ["select", slot] => slot.parse().map_err(anyhow::Error::from).and_then(|s| Ok(handle.select_preset(setlist, s).wait()?)),
            ["set", id, value] => id
                .parse()
                .map_err(anyhow::Error::from)
                .and_then(|id| typed_value(globals, id, value).map(|v| (id, v)))
                .and_then(|(id, v)| Ok(handle.set_setting(id, v).wait()?)),
            ["param", block, param, value] => (|| {
                let block: u64 = block.parse().context("BLOCK is a grid position (0..19)")?;
                let (index, v) = parameter_value(handle, labels, block, param, value)?;
                Ok(handle.set_parameter(block, 0, index, v).wait()?)
            })(),
            ["block", block, state @ ("on" | "off")] => {
                block.parse().map_err(anyhow::Error::from).and_then(|b| Ok(handle.set_block_enabled(b, *state == "on").wait()?))
            }
            ["save"] => handle
                .state()
                .wait()
                .map_err(anyhow::Error::from)
                .and_then(|s| Ok(handle.save_preset(s.current.setlist, s.current.slot as usize, &s.current.name).wait()?)),
            ["snapshot", n] => match n.parse::<u8>() {
                Ok(n @ 1..=6) => handle.recall_snapshot(n).wait().map_err(anyhow::Error::from),
                _ => Err(anyhow::anyhow!("snapshot takes 1..6 (4..6 are HXtra's bank B)")),
            },
            [] => continue,
            _ => Err(anyhow::anyhow!("commands: {MONITOR_COMMANDS}")),
        };
        match outcome {
            Ok(()) => println!("ok: {line}"),
            Err(e) => println!("failed: {line}: {e:#}"),
        }
    }
}

const MONITOR_COMMANDS: &str = "select SLOT | set ID VALUE | param BLOCK PARAM VALUE (PARAM an index or a name; VALUE in the catalog's units, e.g. 0.074 s or a list label; without a catalog 0.5 float, 3 int, true bool) | block BLOCK on|off | snapshot 1-6 (MIDI CC 69) | save (stores the current preset in its slot) | quit";

/// The parameter index and typed value for `param`: from the catalog and the model in that
/// block of the current edit buffer, else by how the value is written.
fn parameter_value(handle: &DeviceHandle, labels: &Labels, block: u64, param: &str, value: &str) -> Result<(u64, Value)> {
    let Some(catalog) = &labels.catalog else {
        let index = param.parse().context("without a catalog PARAM is an index")?;
        return Ok((index, literal_value(value)?));
    };
    let edit = PresetFile::parse(handle.state().wait()?.edit_buffer)?;
    let found = edit.blocks()?.into_iter().find(|b| b.position as u64 == block);
    let b = found.with_context(|| format!("no block at position {block} in the current preset"))?;
    let model = catalog.model(b.preset_id).with_context(|| format!("model {} is not in the catalog", b.preset_id))?;
    let p = param
        .parse()
        .ok()
        .and_then(|i| model.param(i))
        .or_else(|| model.param_named(param))
        .with_context(|| format!("{} has no parameter {param} (it has {})", model.display_name(labels.style), parameter_names(model)))?;
    let v = p.value_for(value)?;
    println!("-> {} / {} = {}", model.display_name(labels.style), p.title, p.format(&v));
    Ok((p.index as u64, v))
}

fn parameter_names(m: &Model) -> String {
    m.params.iter().filter(|p| p.visible).map(|p| format!("{} {}", p.index, p.title)).collect::<Vec<_>>().join(", ")
}

/// The catalog in use (if any) and the name style commands print with.
struct Labels {
    catalog: Option<Catalog>,
    style: NameStyle,
}

impl Labels {
    fn load(path: Option<&Path>, style: NameStyle) -> Result<Self> {
        let found = Catalog::find(path)?;
        let catalog = found.map(|(p, c)| {
            eprintln!("catalog: {} ({} models)", p.display(), c.models.len());
            c
        });
        Ok(Self { catalog, style })
    }

    /// Drop a catalog made for another firmware than the unit's, saying so.
    fn for_firmware(self, firmware: Option<u64>) -> Self {
        let problem = match (&self.catalog, firmware) {
            (Some(c), Some(f)) => c.check_firmware(f).err(),
            _ => None,
        };
        match problem {
            Some(e) => {
                eprintln!("not using the catalog: {e}");
                Self { catalog: None, ..self }
            }
            None => self,
        }
    }

    fn model(&self, preset_id: u64) -> Option<&Model> {
        self.catalog.as_ref()?.model(preset_id)
    }

    fn model_name(&self, preset_id: u64) -> String {
        match self.model(preset_id) {
            Some(m) => m.display_name(self.style).to_owned(),
            None => format!("model {preset_id}"),
        }
    }

    /// `A4 Deluxe Comp / Attack = 74.0 ms` for a value of a block's parameter, if the catalog knows it.
    fn param_text(&self, b: &Block, index: usize, value: &Value) -> Option<String> {
        let m = self.model(b.preset_id)?;
        let p = m.param(index)?;
        Some(format!("{} {} / {} = {}", position_label(b.position), m.display_name(self.style), p.title, p.format(value)))
    }
}

fn position_label(position: usize) -> String {
    snapshot::slot_label(position).unwrap_or_else(|| format!("#{position}"))
}

/// What else to know about a model's name: Line 6's name when another is shown, and HXtra's additions.
fn model_notes(m: &Model, style: NameStyle) -> String {
    let other = match style {
        NameStyle::Line6 => Some(("on the unit", m.title.as_str())),
        NameStyle::Unit | NameStyle::Real => m.stock_title.as_deref().map(|s| ("Line 6", s)),
    };
    let renamed = other.filter(|(_, n)| *n != m.display_name(style)).map(|(what, n)| format!("{what}: {n}"));
    let added = (m.hxtra_added == Some(true)).then(|| "added by HXtra".to_owned());
    let channels = m.subcategory.as_deref().filter(|s| matches!(*s, "mono" | "stereo")).map(str::to_owned);
    let capture = m.is_capture().then(|| "capture (NAM)".to_owned());
    match capture.into_iter().chain(channels).chain(renamed).chain(added).collect::<Vec<_>>() {
        notes if notes.is_empty() => String::new(),
        notes => format!("  ({})", notes.join("; ")),
    }
}

fn print_blocks(p: &PresetFile, labels: &Labels, hidden: bool) -> Result<()> {
    match labels.catalog {
        Some(_) => println!("\nblocks"),
        None => println!("\nblocks (no catalog: model and parameter numbers only, see `hxs catalog`)"),
    }
    for b in p.blocks()? {
        let model = labels.model(b.preset_id);
        let notes = model.map_or(String::new(), |m| model_notes(m, labels.style));
        println!("  {:<3} {:<24} {}{notes}", position_label(b.position), labels.model_name(b.preset_id), if b.enabled { "on" } else { "off" });
        print_block_values(&b, model, hidden);
    }
    Ok(())
}

fn print_block_values(b: &Block, model: Option<&Model>, hidden: bool) {
    for (i, v) in b.params.iter().enumerate() {
        match model.and_then(|m| m.param(i)) {
            Some(p) if p.visible || hidden => {
                println!("        {:<14} {}{}", p.title, p.format(v), if p.hxtra_added == Some(true) { "  (HXtra)" } else { "" })
            }
            Some(_) => {}
            None => println!("        [{i}] {}", value_text(v)),
        }
    }
    let missing: Vec<String> = model
        .map(|m| {
            m.params
                .iter()
                .filter(|p| p.index >= b.params.len() && (p.visible || hidden))
                .map(|p| format!("{} {}", p.title, p.format(&Value::from(p.default))))
                .collect()
        })
        .unwrap_or_default();
    if !missing.is_empty() {
        println!("        not saved in this preset, the unit loads them at their defaults: {}", missing.join(", "));
    }
    if !b.extras.is_empty() {
        println!("        block values after the model's: {}", b.extras.iter().map(value_text).collect::<Vec<_>>().join(", "));
    }
    if let Some(h) = &b.ir_hash {
        println!("        IR {}", h.get(..16).unwrap_or(h));
    }
}

fn show(timeout: Duration, slot: usize, setlist: u64, labels: Labels, hidden: bool) -> Result<()> {
    let mut stomp = connect(timeout)?;
    let list = stomp.preset_list(setlist)?;
    let entry = list.get(slot).with_context(|| format!("setlist {setlist} has slots 000..{:03}", list.len().saturating_sub(1)))?;
    let p = PresetFile::parse(stomp.preset_file(setlist, slot)?)?;
    let labels = labels.for_firmware(p.device_meta().map(|m| m.firmware));
    println!("setlist {setlist} slot {slot:03} \"{}\"", entry.name);
    print_blocks(&p, &labels, hidden)?;
    print_snapshots(&p)
}

fn describe_catalog(labels: &Labels, search: Option<&str>) -> Result<()> {
    let c = labels.catalog.as_ref().with_context(|| {
        let data = catalog::data_folder().map_or("the data folder".into(), |d| d.display().to_string());
        format!("no catalog: pass --catalog, set {}, or put {} next to hxs or in {data}", catalog::ENV_VAR, catalog::FILE_NAME)
    })?;
    let s = &c.source;
    let firmware = s.firmware.map_or("?".into(), |f| format!("{:x}.{:x}.{:x}", f >> 24, (f >> 16) & 0xFF, (f >> 8) & 0xFF));
    println!("{} firmware {firmware}, file md5 {}{}", s.product.as_deref().unwrap_or("?"), s.md5, s.stock_md5.as_ref().map_or(String::new(), |m| format!(" (stock md5 {m})")));
    let added = c.models.iter().filter(|m| m.hxtra_added == Some(true)).count();
    let added_params = c.models.iter().flat_map(|m| &m.params).filter(|p| p.hxtra_added == Some(true)).count();
    let renamed = c.models.iter().filter(|m| m.stock_title.as_ref().is_some_and(|t| *t != m.title)).count();
    println!("{} models ({added} added by HXtra, {added_params} parameters added, {renamed} renamed), {} browser folders", c.models.len(), c.browser.len());
    let Some(search) = search else { return Ok(()) };
    let needle = search.to_lowercase();
    let names = |m: &Model| [Some(&m.title), m.stock_title.as_ref(), m.real_name.as_ref(), Some(&m.name)].into_iter().flatten().any(|n| n.to_lowercase().contains(&needle));
    for m in c.models.iter().filter(|m| names(m)) {
        let category = m.shown_category();
        println!("{:4}  {:<24} {category:<12} {:2} params{}", m.preset_id, m.display_name(labels.style), m.params.len(), model_notes(m, labels.style));
    }
    Ok(())
}

/// A parameter value typed by how it is written: with a point a float, without one an int,
/// `true`/`false` a bool (the unit drops a preset given a value of the wrong type).
fn literal_value(text: &str) -> Result<Value> {
    Ok(match text {
        "true" => Value::from(true),
        "false" => Value::from(false),
        t if t.contains('.') => Value::F32(t.parse().with_context(|| format!("{t} is not a float"))?),
        t => Value::from(t.parse::<i64>().with_context(|| format!("{t} is not an int (write floats with a point)"))?),
    })
}

/// Parse `text` as the type the setting holds now (the unit drops values of the wrong type).
fn typed_value(globals: &Globals, id: u64, text: &str) -> Result<Value> {
    let current = globals
        .entries()
        .iter()
        .filter(|e| e.id == id)
        .min_by_key(|e| e.group == 3)
        .map(|e| &e.value)
        .with_context(|| format!("setting {id} is not among the unit's settings"))?;
    Ok(match current {
        Value::F32(_) => Value::F32(text.parse().with_context(|| format!("setting {id} holds a float"))?),
        Value::F64(_) => Value::F64(text.parse().with_context(|| format!("setting {id} holds a double"))?),
        Value::Integer(_) => Value::from(text.parse::<i64>().with_context(|| format!("setting {id} holds an int"))?),
        Value::Boolean(_) => Value::from(match text {
            "true" | "1" | "on" => true,
            "false" | "0" | "off" => false,
            _ => anyhow::bail!("setting {id} holds a bool (true/false)"),
        }),
        other => anyhow::bail!("setting {id} holds {other}, which this command does not write"),
    })
}

fn restore_all(timeout: Duration, dir: &std::path::Path, yes: bool) -> Result<()> {
    let b = restore::Backup::open(dir).with_context(|| format!("opening the backup in {}", dir.display()))?;
    println!("backup: {} presets, {} IRs, {} favorites (files match their SHA-256)", b.manifest.presets.len(), b.manifest.irs.len(), b.manifest.favorites.len());
    let mut stomp = connect(timeout)?;
    let plan = restore::plan(&mut stomp, &b)?;
    for note in &plan.notes {
        println!("note: {note}");
    }
    print_plan(&plan);
    if plan.is_empty() {
        println!("the unit already matches the backup");
        return Ok(());
    }
    if !yes {
        println!("dry run: nothing written (add --yes to write these {} actions)", plan.actions.len());
        return Ok(());
    }
    restore::apply(&mut stomp, &plan, b.manifest.setlist, |a| println!("-> {}", a.describe())).context("restoring")?;
    let left = restore::plan(&mut stomp, &b)?;
    match left.is_empty() {
        true => println!("checked: the unit now matches the backup"),
        false => {
            print_plan(&left);
            anyhow::bail!("{} actions still differ after the restore", left.actions.len())
        }
    }
    Ok(())
}

fn print_plan(plan: &restore::Plan) {
    for a in &plan.actions {
        println!("  {}", a.describe());
        if let restore::Action::Globals { changes, .. } = a {
            for c in changes {
                println!("      group {} id {:3}: {} -> {}", c.group, c.id, c.from.as_ref().map_or("(absent)".into(), restore::value_text), restore::value_text(&c.to));
            }
        }
    }
}

fn restore_globals(timeout: Duration, path: &std::path::Path, yes: bool) -> Result<()> {
    let file = match path.is_dir() {
        true => path.join("globals.bin"),
        false => path.to_path_buf(),
    };
    let bytes = std::fs::read(&file).with_context(|| format!("reading {}", file.display()))?;
    let backup = Globals::parse(bytes).with_context(|| format!("parsing {}", file.display()))?;
    let mut stomp = connect(timeout)?;
    let current = Globals::parse(stomp.globals()?)?;
    let changes = backup.changes_from(&current);
    println!("{} settings to restore from {}, {} differ from the device:", backup.restorable().count(), file.display(), changes.len());
    for c in &changes {
        println!("  group {} id {:3}: {} -> {}", c.group, c.id, c.from.as_ref().map_or("(absent)".into(), value_text), value_text(&c.to));
    }
    if changes.is_empty() {
        println!("nothing to change");
        return Ok(());
    }
    if !yes {
        println!("dry run: nothing written (add --yes to write the settings)");
        return Ok(());
    }
    stomp.write_globals(&backup.restore_blob()).context("writing the global settings")?;
    println!("written");
    let after = Globals::parse(stomp.globals()?)?;
    match backup.changes_from(&after).as_slice() {
        [] => println!("read back: every restored setting matches the backup, type included"),
        left => anyhow::bail!(
            "read back: {} settings still differ: {}",
            left.len(),
            left.iter()
                .map(|c| format!("group {} id {} = {} (backup {})", c.group, c.id, c.from.as_ref().map_or("(absent)".into(), value_text), value_text(&c.to)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
    Ok(())
}

fn value_text(v: &Value) -> String {
    restore::value_text(v)
}

fn restore_preset(timeout: Duration, file: &std::path::Path, slot: usize, name: &str, setlist: u64, yes: bool) -> Result<()> {
    let bytes = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
    let preset = PresetFile::parse(bytes)?;
    hxs_core::stomp::validate_name(name)?;
    let mut stomp = connect(timeout)?;
    let list = stomp.preset_list(setlist)?;
    let target = list.get(slot).with_context(|| format!("setlist {setlist} has slots 000..{:03}", list.len().saturating_sub(1)))?;
    println!(
        "slot {slot:03} \"{}\" (file {}) <- {} ({} bytes, snapshots 4-6: {}) as \"{name}\"",
        target.name,
        target.file_id,
        file.display(),
        preset.bytes().len(),
        if preset.has_bank_b() { "yes" } else { "no" }
    );
    let current = stomp.current_preset()?;
    if current.setlist == setlist && current.slot == slot as u64 {
        println!(
            "note: this is the current preset; the device reloads it{}",
            if current.modified { " and its unsaved edits are lost" } else { "" }
        );
    }
    if !yes {
        println!("dry run: nothing written (add --yes to overwrite the slot)");
        return Ok(());
    }
    stomp.batch_begin()?;
    let written = stomp.upload_preset(setlist, slot, name, ListFlags::default(), preset.bytes());
    let ended = stomp.batch_end();
    written.context("uploading the preset")?;
    ended.context("ending the librarian batch")?;
    println!("written");
    let back = stomp.preset_file(setlist, slot)?;
    let listed = stomp.preset_list(setlist)?;
    let now = listed.get(slot).map_or("?", |p| p.name.as_str());
    match back == preset.bytes() {
        true => println!("read back: identical ({} bytes, sha256 {}), slot now \"{now}\"", back.len(), backup::sha256_hex(&back)),
        false => anyhow::bail!(
            "read back differs: wrote {} bytes (sha256 {}), the slot holds {} bytes (sha256 {}), named \"{now}\"",
            preset.bytes().len(),
            backup::sha256_hex(preset.bytes()),
            back.len(),
            backup::sha256_hex(&back)
        ),
    }
    Ok(())
}

fn connect(timeout: Duration) -> Result<Stomp> {
    Stomp::connect(timeout).context("connecting to the HX Stomp")
}

fn info(timeout: Duration) -> Result<()> {
    let mut stomp = connect(timeout)?;
    let current = stomp.current_preset()?;
    let stored = PresetFile::parse(stomp.preset_file(current.setlist, 0)?)?;
    match stored.device_meta() {
        Some(m) => println!("device   {} (USB {:04x}), firmware {} ({})", m.product, stomp.product_id(), m.firmware_text(), m.build),
        None => println!("device   USB {:04x}, firmware unknown", stomp.product_id()),
    }
    for s in stomp.setlists()? {
        println!("setlist  {} {}", s.index, s.name);
    }
    println!(
        "current  setlist {} slot {:03} \"{}\"{}",
        current.setlist,
        current.slot,
        current.name,
        if current.modified { " (modified)" } else { "" }
    );
    Ok(())
}

fn presets(timeout: Duration, setlist: u64) -> Result<()> {
    let mut stomp = connect(timeout)?;
    for p in stomp.preset_list(setlist)? {
        println!("{:03}  {:<16}  (file {})", p.slot, p.name, p.file_id);
    }
    Ok(())
}

fn run_backup(timeout: Duration, dir: &std::path::Path, setlist: u64) -> Result<()> {
    let mut stomp = connect(timeout)?;
    let tool = format!("hxs {}", env!("CARGO_PKG_VERSION"));
    let manifest = backup::run(&mut stomp, dir, setlist, &tool, |e| match e {
        backup::Event::Preset { slot, total, name, bank_b } => {
            println!("preset   {slot:03} ({}/{total}) {name}{}", slot + 1, if bank_b { "  [snapshots 4-6]" } else { "" })
        }
        backup::Event::Ir { done, total, name } => println!("IR       {done}/{total} {name}"),
        backup::Event::Favorite { done, total, name } => println!("favorite {done}/{total} {name}"),
        backup::Event::Globals => println!("globals  done"),
    })
    .with_context(|| format!("backing up into {}", dir.display()))?;
    let with_b = manifest.presets.iter().filter(|p| p.bank_b).count();
    println!(
        "\n{} presets ({} with snapshots 4-6), {} IRs, {} favorites -> {}",
        manifest.presets.len(),
        with_b,
        manifest.irs.len(),
        manifest.favorites.len(),
        dir.display()
    );
    Ok(())
}

fn inspect(file: &std::path::Path, labels: Labels, hidden: bool) -> Result<()> {
    let bytes = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
    let p = PresetFile::parse(bytes)?;
    let labels = labels.for_firmware(p.device_meta().map(|m| m.firmware));
    println!("{}: {} bytes, sha256 {}", file.display(), p.bytes().len(), backup::sha256_hex(p.bytes()));
    if let Some(m) = p.device_meta() {
        println!("made by  {} firmware {} ({})", m.product, m.firmware_text(), m.build);
    }
    for s in p.sections() {
        println!("section  {:#04x}  {:5} bytes", s.key, s.value.len());
    }
    match p.bank_b() {
        Some(b) => println!("snapshots 4-6: present ({} bytes)", b.len()),
        None => println!("snapshots 4-6: none"),
    }
    if p.trailing() > 0 {
        println!("warning: {} bytes after the section map", p.trailing());
    }
    print_blocks(&p, &labels, hidden)?;
    print_snapshots(&p)
}

fn print_snapshots(p: &PresetFile) -> Result<()> {
    let occupied = p.occupied_positions();
    println!("\nsnapshots (blocks off among {}; * = footswitch on)", labels(&occupied));
    for s in snapshot::snapshots(p)? {
        let bank = match s.bank {
            Bank::A => "",
            Bank::B => "  [bank B]",
        };
        if !s.valid {
            println!("  {}  {:<11} unused{bank}", s.number, s.name);
            continue;
        }
        let off: Vec<usize> = occupied.iter().copied().filter(|&i| !s.blocks_on.get(i).copied().unwrap_or(false)).collect();
        let ctl: Vec<String> = s
            .controllers
            .iter()
            .map(|c| format!("{}={:.3}{}", c.index, c.value, if c.footswitch_on { "*" } else { "" }))
            .collect();
        println!("  {}  {:<11} tempo {:6.2}  off: {:<10} controllers {}{bank}", s.number, s.name, s.tempo, labels(&off), ctl.join(" "));
    }
    Ok(())
}

fn labels(positions: &[usize]) -> String {
    match positions.iter().filter_map(|&i| snapshot::slot_label(i)).collect::<Vec<_>>() {
        l if l.is_empty() => "-".into(),
        l => l.join(","),
    }
}
