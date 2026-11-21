//! Restoring a backup made by [`crate::backup`]. [`plan`] compares it with the
//! device and lists only what differs; [`apply`] writes that list: IRs first
//! (presets may use them), then presets, favorites and global settings.
//! Backup files are checked against their SHA-256 before anything is planned.

use std::fs;
use std::path::{Path, PathBuf};

use rmpv::Value;

use crate::backup::{self, FileRecord, Manifest};
use crate::globals::{Change, Globals};
use crate::preset::PresetFile;
use crate::stomp::{self, ListFlags, Stomp, favorite_without_number};
use crate::{Error, Result, msg};

pub struct Backup {
    dir: PathBuf,
    pub manifest: Manifest,
}

impl Backup {
    pub fn open(dir: &Path) -> Result<Self> {
        let manifest: Manifest = serde_json::from_slice(&fs::read(dir.join("manifest.json"))?)?;
        if manifest.format != backup::FORMAT || manifest.format_version != backup::FORMAT_VERSION {
            return Err(Error::Backup(format!(
                "{} holds a {} v{} manifest; this build reads {} v{}",
                dir.display(),
                manifest.format,
                manifest.format_version,
                backup::FORMAT,
                backup::FORMAT_VERSION
            )));
        }
        let b = Self { dir: dir.to_path_buf(), manifest };
        for rec in b.files() {
            b.read(rec)?;
        }
        Ok(b)
    }

    fn files(&self) -> impl Iterator<Item = &FileRecord> {
        let m = &self.manifest;
        m.presets
            .iter()
            .map(|p| &p.file)
            .chain(m.irs.iter().flat_map(|i| [&i.data, &i.info]))
            .chain(m.favorites.iter().map(|f| &f.file))
            .chain(std::iter::once(&m.globals))
    }

    pub fn read(&self, rec: &FileRecord) -> Result<Vec<u8>> {
        let bytes = fs::read(self.dir.join(&rec.file))?;
        match backup::sha256_hex(&bytes) == rec.sha256 {
            true => Ok(bytes),
            false => Err(Error::Backup(format!("{} does not match its SHA-256 in the manifest", rec.file))),
        }
    }

    fn read_value(&self, rec: &FileRecord) -> Result<Value> {
        let bytes = self.read(rec)?;
        Ok(rmpv::decode::read_value(&mut &bytes[..])?)
    }
}

#[derive(Debug)]
pub enum Action {
    DeleteIr { slot: u64, name: String },
    UploadIr { slot: u64, name: String, info: Value, data: Vec<u8> },
    Preset { slot: usize, name: String, flags: ListFlags, file: Vec<u8>, was: String },
    Favorite { index: u64, name: String, value: Value },
    Globals { changes: Vec<Change>, blob: Vec<u8> },
}

impl Action {
    pub fn describe(&self) -> String {
        match self {
            Action::DeleteIr { slot, name } => format!("delete IR {:03} \"{name}\" (to write the backup's IR there)", slot + 1),
            Action::UploadIr { slot, name, .. } => format!("write IR {:03} \"{name}\"", slot + 1),
            Action::Preset { slot, name, was, file, .. } => {
                let b = PresetFile::parse(file.clone()).map(|p| p.has_bank_b()).unwrap_or(false);
                format!("write preset {slot:03} \"{name}\" (now \"{was}\"){}", if b { " with snapshots 4-6" } else { "" })
            }
            Action::Favorite { index, name, .. } => format!("write favorite {:02} \"{name}\"", index + 1),
            Action::Globals { changes, .. } => format!("write global settings ({} differ)", changes.len()),
        }
    }

    fn is_librarian(&self) -> bool {
        !matches!(self, Action::Globals { .. })
    }
}

#[derive(Debug, Default)]
pub struct Plan {
    pub actions: Vec<Action>,
    /// Things the restore leaves as they are, and why.
    pub notes: Vec<String>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }
}

pub fn plan(stomp: &mut Stomp, b: &Backup) -> Result<Plan> {
    let mut plan = Plan::default();
    check_device(stomp, b, &mut plan)?;
    plan_irs(stomp, b, &mut plan)?;
    plan_presets(stomp, b, &mut plan)?;
    plan_favorites(stomp, b, &mut plan)?;
    plan_globals(stomp, b, &mut plan)?;
    Ok(plan)
}

/// Run every action in order; `progress` sees each one before it runs.
pub fn apply(stomp: &mut Stomp, plan: &Plan, setlist: u64, mut progress: impl FnMut(&Action)) -> Result<()> {
    let (librarian, rest): (Vec<&Action>, Vec<&Action>) = plan.actions.iter().partition(|a| a.is_librarian());
    if !librarian.is_empty() {
        stomp.batch_begin()?;
        let written = librarian.iter().try_for_each(|a| {
            progress(a);
            run(stomp, a, setlist)
        });
        let ended = stomp.batch_end();
        written?;
        ended?;
    }
    rest.iter().try_for_each(|a| {
        progress(a);
        run(stomp, a, setlist)
    })
}

fn run(stomp: &mut Stomp, action: &Action, setlist: u64) -> Result<()> {
    match action {
        Action::DeleteIr { slot, .. } => stomp.delete_ir(*slot),
        Action::UploadIr { slot, info, data, .. } => stomp.upload_ir(*slot, info, data),
        Action::Preset { slot, name, flags, file, .. } => stomp.upload_preset(setlist, *slot, name, *flags, file),
        Action::Favorite { index, value, .. } => stomp.write_favorite(*index, value),
        Action::Globals { blob, .. } => stomp.write_globals(blob),
    }
}

fn check_device(stomp: &mut Stomp, b: &Backup, plan: &mut Plan) -> Result<()> {
    let device = PresetFile::parse(stomp.edit_buffer()?)?.device_meta();
    match (&b.manifest.device, &device) {
        (Some(want), Some(have)) if want.product != have.product => Err(Error::Backup(format!(
            "the backup is from a {} and the connected unit is a {}",
            want.product, have.product
        ))),
        (Some(want), Some(have)) if want.build != have.build || want.firmware != have.firmware => {
            plan.notes.push(format!(
                "the backup was made on firmware {} ({}), the unit runs {} ({})",
                want.firmware_text(),
                want.build,
                have.firmware_text(),
                have.build
            ));
            Ok(())
        }
        _ => Ok(()),
    }
}

fn plan_irs(stomp: &mut Stomp, b: &Backup, plan: &mut Plan) -> Result<()> {
    let mut device = Vec::new();
    for ir in stomp.ir_list()? {
        device.push((ir.index, ir.name.clone(), stomp.ir_info(ir.index)?, stomp.ir_data(ir.index)?));
    }
    for rec in &b.manifest.irs {
        let info = b.read_value(&rec.info)?;
        let data = b.read(&rec.data)?;
        let upload = Action::UploadIr { slot: rec.index, name: rec.name.clone(), info: info.clone(), data: data.clone() };
        match device.iter().find(|(slot, ..)| *slot == rec.index) {
            Some((_, _, dinfo, ddata)) if *dinfo == info && *ddata == data => {}
            Some((slot, dname, ..)) => {
                plan.actions.push(Action::DeleteIr { slot: *slot, name: dname.clone() });
                plan.actions.push(upload);
            }
            None => plan.actions.push(upload),
        }
    }
    for (slot, name, ..) in device.iter().filter(|(slot, ..)| !b.manifest.irs.iter().any(|r| r.index == *slot)) {
        plan.notes.push(format!("IR {:03} \"{name}\" is not in the backup; left as it is", slot + 1));
    }
    Ok(())
}

fn plan_presets(stomp: &mut Stomp, b: &Backup, plan: &mut Plan) -> Result<()> {
    let setlist = b.manifest.setlist;
    let list = stomp.preset_list(setlist)?;
    if list.len() != b.manifest.presets.len() {
        return Err(Error::Backup(format!(
            "setlist {setlist} has {} slots on the unit and {} in the backup",
            list.len(),
            b.manifest.presets.len()
        )));
    }
    for rec in &b.manifest.presets {
        let file = b.read(&rec.file)?;
        PresetFile::parse(file.clone())?;
        stomp::validate_name(&rec.name)?;
        let entry = &list[rec.slot];
        let same = entry.name == rec.name && flags_match(entry.flags, rec.list_flags) && stomp.preset_file(setlist, rec.slot)? == file;
        if !same {
            plan.actions.push(Action::Preset { slot: rec.slot, name: rec.name.clone(), flags: rec.list_flags, file, was: entry.name.clone() });
        }
    }
    Ok(())
}

/// Each flag the backup recorded must match the unit's (older backups recorded none).
fn flags_match(device: ListFlags, backup: ListFlags) -> bool {
    backup.flag_7b.is_none_or(|v| device.flag_7b == Some(v))
        && backup.flag_7c.is_none_or(|v| device.flag_7c == Some(v))
        && backup.flag_7d.is_none_or(|v| device.flag_7d == Some(v))
}

fn plan_favorites(stomp: &mut Stomp, b: &Backup, plan: &mut Plan) -> Result<()> {
    let mut device = Vec::new();
    for fav in stomp.favorite_list()? {
        device.push((fav.index, fav.name.clone(), stomp.favorite(fav.index)?));
    }
    for rec in &b.manifest.favorites {
        let value = b.read_value(&rec.file)?;
        let same = device.iter().any(|(i, _, v)| *i == rec.index && favorite_without_number(v) == favorite_without_number(&value));
        if !same {
            plan.actions.push(Action::Favorite { index: rec.index, name: rec.name.clone(), value });
        }
    }
    for (index, name, _) in device.iter().filter(|(i, ..)| !b.manifest.favorites.iter().any(|r| r.index == *i)) {
        plan.notes.push(format!("favorite {:02} \"{name}\" is not in the backup; left as it is", index + 1));
    }
    Ok(())
}

fn plan_globals(stomp: &mut Stomp, b: &Backup, plan: &mut Plan) -> Result<()> {
    let backup = Globals::parse(b.read(&b.manifest.globals)?)?;
    let current = Globals::parse(stomp.globals()?)?;
    let changes = backup.changes_from(&current);
    if !changes.is_empty() {
        plan.actions.push(Action::Globals { changes, blob: backup.restore_blob() });
    }
    Ok(())
}

pub fn value_text(v: &Value) -> String {
    match v {
        Value::F32(f) => format!("{f:?} (float)"),
        Value::F64(f) => format!("{f:?} (double)"),
        Value::Integer(i) => format!("{i} (int)"),
        Value::Boolean(b) => format!("{b} (bool)"),
        other => msg::text(other).unwrap_or_else(|| other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_only_count_when_recorded() {
        let device = ListFlags { flag_7b: Some(false), flag_7c: Some(true), flag_7d: Some(0) };
        assert!(flags_match(device, ListFlags::default()));
        assert!(flags_match(device, device));
        assert!(!flags_match(device, ListFlags { flag_7c: Some(false), ..ListFlags::default() }));
    }

    #[test]
    fn librarian_actions_run_before_globals() {
        let g = Action::Globals { changes: Vec::new(), blob: Vec::new() };
        let f = Action::Favorite { index: 0, name: "x".into(), value: Value::Nil };
        assert!(!g.is_librarian() && f.is_librarian());
    }
}
