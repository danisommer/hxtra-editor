//! A full read-only backup: every preset file as stored (so HXtra's snapshots
//! 4..6 survive), the IRs, the favorites and the globals blob, plus a manifest.
//!
//! Layout of a backup directory:
//! `manifest.json`, `presets/NNN.l6p`, `irs/NNN.bin` + `irs/NNN.info.msgpack`,
//! `favorites/NNN.msgpack`, `globals.bin`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::preset::{DeviceMeta, PresetFile};
use crate::stomp::{ListFlags, Stomp};
use crate::{Error, Result, msg};

pub const FORMAT: &str = "hxs-backup";
pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
pub struct Manifest {
    pub format: &'static str,
    pub format_version: u32,
    pub tool: String,
    pub created_unix: u64,
    pub usb_product_id: u16,
    pub device: Option<DeviceMeta>,
    pub setlist: u64,
    pub presets: Vec<PresetRecord>,
    pub irs: Vec<IrRecord>,
    pub favorites: Vec<NamedFile>,
    pub globals: FileRecord,
}

#[derive(Debug, Serialize)]
pub struct FileRecord {
    pub file: String,
    pub bytes: usize,
    pub sha256: String,
}

#[derive(Debug, Serialize)]
pub struct PresetRecord {
    pub slot: usize,
    pub file_id: u64,
    pub name: String,
    pub list_flags: ListFlags,
    pub bank_b: bool,
    #[serde(flatten)]
    pub file: FileRecord,
}

#[derive(Debug, Serialize)]
pub struct IrRecord {
    pub index: u64,
    pub name: String,
    pub data: FileRecord,
    pub info: FileRecord,
}

#[derive(Debug, Serialize)]
pub struct NamedFile {
    pub index: u64,
    pub name: String,
    #[serde(flatten)]
    pub file: FileRecord,
}

#[derive(Debug)]
pub enum Event<'a> {
    Preset { slot: usize, total: usize, name: &'a str, bank_b: bool },
    Ir { done: usize, total: usize, name: &'a str },
    Favorite { done: usize, total: usize, name: &'a str },
    Globals,
}

pub fn run(stomp: &mut Stomp, dir: &Path, setlist: u64, tool: &str, mut progress: impl FnMut(Event)) -> Result<Manifest> {
    prepare_dir(dir)?;
    stomp.batch_begin()?;
    let result = collect(stomp, dir, setlist, tool, &mut progress);
    let end = stomp.batch_end();
    let manifest = result?;
    end?;
    write(dir, "manifest.json", &serde_json::to_vec_pretty(&manifest)?)?;
    Ok(manifest)
}

fn collect(stomp: &mut Stomp, dir: &Path, setlist: u64, tool: &str, progress: &mut impl FnMut(Event)) -> Result<Manifest> {
    let list = stomp.preset_list(setlist)?;
    let mut device = None;
    let mut presets = Vec::with_capacity(list.len());
    for entry in &list {
        let bytes = stomp.preset_file(setlist, entry.slot)?;
        let parsed = PresetFile::parse(bytes)?;
        device = device.or_else(|| parsed.device_meta());
        let bank_b = parsed.has_bank_b();
        progress(Event::Preset { slot: entry.slot, total: list.len(), name: &entry.name, bank_b });
        presets.push(PresetRecord {
            slot: entry.slot,
            file_id: entry.file_id,
            name: entry.name.clone(),
            list_flags: entry.flags,
            bank_b,
            file: write(dir, &format!("presets/{:03}.l6p", entry.slot), parsed.bytes())?,
        });
    }

    let ir_list = stomp.ir_list()?;
    let mut irs = Vec::with_capacity(ir_list.len());
    for (done, ir) in ir_list.iter().enumerate() {
        let info = msg::encode_value(&stomp.ir_info(ir.index)?);
        let data = stomp.ir_data(ir.index)?;
        progress(Event::Ir { done: done + 1, total: ir_list.len(), name: &ir.name });
        irs.push(IrRecord {
            index: ir.index,
            name: ir.name.clone(),
            data: write(dir, &format!("irs/{:03}.bin", ir.index), &data)?,
            info: write(dir, &format!("irs/{:03}.info.msgpack", ir.index), &info)?,
        });
    }

    let fav_list = stomp.favorite_list()?;
    let mut favorites = Vec::with_capacity(fav_list.len());
    for (done, fav) in fav_list.iter().enumerate() {
        let value = msg::encode_value(&stomp.favorite(fav.index)?);
        progress(Event::Favorite { done: done + 1, total: fav_list.len(), name: &fav.name });
        favorites.push(NamedFile { index: fav.index, name: fav.name.clone(), file: write(dir, &format!("favorites/{:03}.msgpack", fav.index), &value)? });
    }

    let globals = write(dir, "globals.bin", &stomp.globals()?)?;
    progress(Event::Globals);

    Ok(Manifest {
        format: FORMAT,
        format_version: FORMAT_VERSION,
        tool: tool.to_owned(),
        created_unix: SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()),
        usb_product_id: stomp.product_id(),
        device,
        setlist,
        presets,
        irs,
        favorites,
        globals,
    })
}

fn prepare_dir(dir: &Path) -> Result<()> {
    match fs::read_dir(dir) {
        Ok(mut entries) => match entries.next() {
            None => Ok(()),
            Some(_) => Err(Error::Backup(format!("{} is not empty; choose a new folder", dir.display()))),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => fs::create_dir_all(dir).map_err(Error::from),
        Err(e) => Err(e.into()),
    }
}

fn write(dir: &Path, rel: &str, data: &[u8]) -> Result<FileRecord> {
    let path: PathBuf = dir.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, data)?;
    Ok(FileRecord { file: rel.to_owned(), bytes: data.len(), sha256: sha256_hex(data) })
}

pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_like_sha256sum() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn refuses_a_non_empty_folder() {
        let dir = std::env::temp_dir().join(format!("hxs-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("x"), b"x").unwrap();
        assert!(matches!(prepare_dir(&dir), Err(Error::Backup(_))));
        fs::remove_dir_all(&dir).unwrap();
        prepare_dir(&dir).unwrap();
        fs::remove_dir_all(&dir).unwrap();
    }
}
