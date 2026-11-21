use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use hxs_core::backup::{self, Event};
use hxs_core::globals::Globals;
use hxs_core::preset::PresetFile;
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
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
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
    Inspect { file: PathBuf },
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
    match cli.command {
        Command::Info => info(timeout),
        Command::Presets { setlist } => presets(timeout, setlist),
        Command::Backup { dir, setlist } => run_backup(timeout, &dir, setlist),
        Command::Inspect { file } => inspect(&file),
        Command::RestorePreset { file, slot, name, setlist, yes } => restore_preset(timeout, &file, slot, &name, setlist, yes),
        Command::RestoreGlobals { path, yes } => restore_globals(timeout, &path, yes),
        Command::Restore { dir, yes } => restore_all(timeout, &dir, yes),
    }
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
    let edit = PresetFile::parse(stomp.edit_buffer()?)?;
    match edit.device_meta() {
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
        Event::Preset { slot, total, name, bank_b } => {
            println!("preset   {slot:03} ({}/{total}) {name}{}", slot + 1, if bank_b { "  [snapshots 4-6]" } else { "" })
        }
        Event::Ir { done, total, name } => println!("IR       {done}/{total} {name}"),
        Event::Favorite { done, total, name } => println!("favorite {done}/{total} {name}"),
        Event::Globals => println!("globals  done"),
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

fn inspect(file: &std::path::Path) -> Result<()> {
    let bytes = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
    let p = PresetFile::parse(bytes)?;
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
