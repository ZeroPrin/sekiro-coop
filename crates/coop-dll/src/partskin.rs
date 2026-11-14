//! Wardrobe outfits made of new part files (`reed.rs`, `red.rs`): the machinery they share.
//!
//! Such an outfit is shipped as part files with part ids no vanilla file has, in the optional
//! parts package (`<dll dir>\mod\parts\`, FORK.md "Parts package"). The game builds a part
//! path from the protector row's `equipModelId` (`parts:/BD_M_%04d.partsbnd.dcx`), so the DLL
//! points existing, never-owned EquipParamProtector rows at the new ids, in memory only (no new
//! rows: a save stays valid without the mod). One outfit = one [`Def`] (rows, files, replica
//! fallback); a new outfit is one more `PartSkin` static in [`ALL`] plus its wardrobe skin.
//!
//! Rules (Reed SPEC 1.3, the same for every outfit): patch on every machine, once the params
//! are loaded and before the first world load (the frame task runs from the title screen);
//! only when all the outfit's files exist (a patched row without its file makes the game ask
//! for a path nobody serves); only when every patched field holds its vanilla value (or
//! already the new one): an unexpected value turns the outfit off for the run and nothing of
//! it is written. Each machine renders the friend's character from its own param, so a
//! friend's rows of an outfit this machine lacks are drawn as a vanilla outfit
//! (`Def::replica_ids`).

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering::Relaxed};

use fromsoftware_shared::FromStatic;
use sekiro::param::{EQUIP_PARAM_PROTECTOR_ST, ParamDef};
use sekiro::sprj::SoloParamRepository;

use crate::log;

/// Every outfit, in patch order.
pub static ALL: [&PartSkin; 2] = [&crate::reed::REED, &crate::red::RED];

/// EquipParamProtector row size (Reed SPEC 1.3: 0x1FC, `equipModelId` u16 at 0xA0).
pub const ROW_SIZE: usize = 0x1FC;
/// SoloParamRepository holder of EquipParamProtector (vendored `solo_param_repository.rs`).
const PROTECTOR_HOLDER: usize = 1;

/// A u16 field of an EquipParamProtector row the patch may write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    /// `equipModelId` (0xA0).
    Model,
    /// `resistPoison` (0xC0).
    ResistPoison,
    /// `resistDisease` (0xC2).
    ResistDisease,
    /// `resistBlood` (0xC4).
    ResistBlood,
}

impl Field {
    /// The paramdef name (log lines).
    pub fn name(self) -> &'static str {
        match self {
            Field::Model => "equipModelId",
            Field::ResistPoison => "resistPoison",
            Field::ResistDisease => "resistDisease",
            Field::ResistBlood => "resistBlood",
        }
    }

    pub fn get(self, r: &EQUIP_PARAM_PROTECTOR_ST) -> u16 {
        match self {
            Field::Model => r.equip_model_id(),
            Field::ResistPoison => r.resist_poison(),
            Field::ResistDisease => r.resist_disease(),
            Field::ResistBlood => r.resist_blood(),
        }
    }

    pub fn set(self, r: &mut EQUIP_PARAM_PROTECTOR_ST, v: u16) {
        match self {
            Field::Model => r.set_equip_model_id(v),
            Field::ResistPoison => r.set_resist_poison(v),
            Field::ResistDisease => r.set_resist_disease(v),
            Field::ResistBlood => r.set_resist_blood(v),
        }
    }
}

/// A field other than `equipModelId` the patch writes in a row (vanilla value -> `value`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldPatch {
    pub field: Field,
    pub vanilla: u16,
    pub value: u16,
}

/// One protector row the patch repoints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatchRow {
    pub row: u32,
    /// `equipModelId`: vanilla and the outfit's.
    pub vanilla: u16,
    pub model: u16,
    pub role: &'static str,
    /// Further fields written with the model (gameplay-neutral rows), usually none.
    pub also: &'static [FieldPatch],
}

impl PatchRow {
    /// Every field the row patch writes: the model first.
    fn fields(&self) -> impl Iterator<Item = FieldPatch> + '_ {
        std::iter::once(FieldPatch { field: Field::Model, vanilla: self.vanilla, value: self.model }).chain(self.also.iter().copied())
    }
}

/// A friend's protector id this machine draws as another when it lacks the outfit: slot
/// `slot` (12 head .. 15 legs) holding row `from` becomes `to`, only when the friend's
/// (unchanged) slot `with.0` holds row `with.1` too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fallback {
    pub slot: usize,
    pub from: i32,
    pub to: i32,
    pub with: Option<(usize, i32)>,
}

/// One outfit (pure data).
#[derive(Debug)]
pub struct Def {
    /// Log prefix (`reed`).
    pub tag: &'static str,
    /// The outfit's name in log lines (`Reed`).
    pub name: &'static str,
    /// What the wardrobe offers with it (`skin`, `skin and head`).
    pub offers: &'static str,
    /// What a friend's outfit is drawn as without it (`the Wolf`).
    pub drawn_as: &'static str,
    pub rows: &'static [PatchRow],
    /// The part files, relative to the DLL's folder (the package's `mod\` is the game's root).
    pub files: &'static [&'static str],
    pub fallback: &'static [Fallback],
}

/// What a row holds now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowNow {
    Vanilla,
    /// Already the outfit's values (a second run of the patch, or another patcher did the same).
    Patched,
}

const P: i32 = 0x1000_0000;

// ===================================================================== pure, unit-tested

impl Def {
    /// The files that `exists` says are missing.
    pub fn missing_files(&self, exists: impl Fn(&str) -> bool) -> Vec<&'static str> {
        self.files.iter().copied().filter(|f| !exists(f)).collect()
    }

    /// Checks every row before anything is written: each patched field must hold its vanilla
    /// value or already the outfit's. `read(row, field)` = the row's value, None = no such row.
    /// A row is `Patched` only when all its fields are.
    pub fn check_rows(&self, read: impl Fn(u32, Field) -> Option<u16>) -> Result<Vec<RowNow>, String> {
        let mut out = Vec::with_capacity(self.rows.len());
        for r in self.rows {
            let mut patched = true;
            for f in r.fields() {
                match read(r.row, f.field) {
                    None => return Err(format!("row {} is not in EquipParamProtector", r.row)),
                    Some(v) if v == f.value => {}
                    Some(v) if v == f.vanilla => patched = false,
                    Some(v) => {
                        return Err(format!(
                            "row {} ({}) has {} {v}, expected {} (vanilla) or {} ({})",
                            r.row,
                            r.role,
                            f.field.name(),
                            f.vanilla,
                            f.value,
                            self.name
                        ));
                    }
                }
            }
            out.push(if patched { RowNow::Patched } else { RowNow::Vanilla });
        }
        Ok(out)
    }

    /// A friend's equipped ids (`Loadout.slots`, category bits included) as this machine can
    /// draw them. With the outfit here (`here`) they are drawn as sent; without it the rows'
    /// vanilla models would show, so `fallback` maps them to a vanilla outfit. Only protector
    /// slots are touched. None = nothing to change.
    pub fn replica_ids(&self, ids: &[i32; 25], here: bool) -> Option<[i32; 25]> {
        if here {
            return None;
        }
        let mut out = *ids;
        for f in self.fallback {
            if out[f.slot] == P | f.from && f.with.is_none_or(|(s, r)| ids[s] == P | r) {
                out[f.slot] = P | f.to;
            }
        }
        (out != *ids).then_some(out)
    }
}

// ===================================================================== game side

const PENDING: u8 = 0;
const WAIT_PARAMS: u8 = 1;
const READY: u8 = 2;
const OFF: u8 = 3;

/// An outfit and its state for this run.
pub struct PartSkin {
    def: Def,
    state: AtomicU8,
    replica_logged: AtomicBool,
}

impl std::ops::Deref for PartSkin {
    type Target = Def;
    fn deref(&self) -> &Def {
        &self.def
    }
}

impl PartSkin {
    pub const fn new(def: Def) -> Self {
        Self { def, state: AtomicU8::new(PENDING), replica_logged: AtomicBool::new(false) }
    }

    /// The outfit can be worn and drawn on this machine: files present and rows patched.
    pub fn available(&self) -> bool {
        self.state.load(Relaxed) == READY
    }

    fn off(&self, why: &str) {
        self.state.store(OFF, Relaxed);
        log::line(&format!("{}: {} not offered: {why}", self.tag, self.offers));
    }

    fn tick(&self) {
        match self.state.load(Relaxed) {
            PENDING => self.check_files(),
            WAIT_PARAMS => self.patch(),
            _ => {}
        }
    }

    fn check_files(&self) {
        let Some(dir) = crate::settings::own_dll_path().and_then(|p| p.parent().map(std::path::Path::to_path_buf)) else {
            return self.off("cannot find the DLL's folder");
        };
        let missing = self.missing_files(|f| dir.join(f).is_file());
        if !missing.is_empty() {
            return self.off(&format!(
                "{} of {} part files missing in {} (first: {}); param rows left vanilla",
                missing.len(),
                self.files.len(),
                dir.join(r"mod\parts").display(),
                missing[0]
            ));
        }
        log::line(&format!("{}: all {} part files present in {}", self.tag, self.files.len(), dir.join(r"mod\parts").display()));
        self.state.store(WAIT_PARAMS, Relaxed);
    }

    fn patch(&self) {
        if !crate::items::solo_param_ready(PROTECTOR_HOLDER) {
            return;
        }
        if std::mem::size_of::<EQUIP_PARAM_PROTECTOR_ST>() != ROW_SIZE {
            return self.off("EquipParamProtector row size is not 0x1FC (crate / game version?); param rows left vanilla");
        }
        let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return };
        let Some(file) = repo.params_mut().find(|p| p.struct_name() == EQUIP_PARAM_PROTECTOR_ST::NAME).map(|p| &mut *p.data) else {
            return;
        };
        // SAFETY: the file is EquipParamProtector (struct name), rows are that struct.
        let now = match self.check_rows(|id, f| unsafe { file.get_row_by_id::<EQUIP_PARAM_PROTECTOR_ST>(id) }.map(|r| f.get(r))) {
            Ok(n) => n,
            Err(e) => return self.off(&format!("{e}; nothing written")),
        };
        let mut done = Vec::new();
        for (r, n) in self.rows.iter().zip(now) {
            let Some(row) = (unsafe { file.get_row_by_id_mut::<EQUIP_PARAM_PROTECTOR_ST>(r.row) }) else {
                return self.off(&format!("row {} vanished between check and write", r.row));
            };
            if n == RowNow::Vanilla {
                for f in r.fields() {
                    f.field.set(row, f.value);
                }
            }
            if let Some(f) = r.fields().find(|f| f.field.get(row) != f.value) {
                return self.off(&format!("row {} reads {} {} after the write", r.row, f.field.name(), f.field.get(row)));
            }
            let also: Vec<String> = r.also.iter().map(|f| format!("{} {}->{}", f.field.name(), f.vanilla, f.value)).collect();
            done.push(format!(
                "{} {}->{}{}{}",
                r.row,
                r.vanilla,
                r.model,
                if also.is_empty() { String::new() } else { format!(" ({})", also.join(", ")) },
                if n == RowNow::Patched { " (already)" } else { "" }
            ));
        }
        self.state.store(READY, Relaxed);
        log::line(&format!(
            "{}: EquipParamProtector equipModelId patched in memory: {}; {} offered{}",
            self.tag,
            done.join(", "),
            self.offers,
            if crate::game::player().is_some() {
                " (patched while in the world: parts already loaded keep their model until the next load)"
            } else {
                ""
            }
        ));
    }

    /// The ids to put on a friend's replica (see `Def::replica_ids`). Logs the first fallback
    /// of the run.
    fn for_replica(&self, ids: &[i32; 25], who: &str) -> Option<[i32; 25]> {
        let out = self.replica_ids(ids, self.available())?;
        if !self.replica_logged.swap(true, Relaxed) {
            log::line(&format!(
                "{tag}: {who} wears {name} but {name} is not available on this PC (state {}): drawn as {} (logged once)",
                match self.state.load(Relaxed) {
                    PENDING | WAIT_PARAMS => "params not patched yet".to_owned(),
                    _ => format!("files missing or patch refused, see the {}: lines above", self.tag),
                },
                self.drawn_as,
                tag = self.tag,
                name = self.name,
            ));
        }
        Some(out)
    }
}

/// lib.rs frame task, every frame from the title screen (game thread). Cheap once settled.
pub fn tick() {
    for s in ALL {
        s.tick();
    }
}

/// loadout.rs `apply`: a friend's ids with every outfit this machine lacks drawn as its
/// fallback. None = nothing to change.
pub fn for_replica(ids: &[i32; 25], who: &str) -> Option<[i32; 25]> {
    let mut out: Option<[i32; 25]> = None;
    for s in ALL {
        if let Some(n) = s.for_replica(out.as_ref().unwrap_or(ids), who) {
            out = Some(n);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The byte offset a field's setter writes in a zeroed row.
    pub(crate) fn offset_of(f: Field) -> Vec<usize> {
        // SAFETY: a plain-data param row; all-zero bytes are a valid value.
        let mut row: EQUIP_PARAM_PROTECTOR_ST = unsafe { std::mem::zeroed() };
        f.set(&mut row, 0xBEEF);
        assert_eq!(f.get(&row), 0xBEEF);
        let bytes = unsafe { std::slice::from_raw_parts((&row as *const EQUIP_PARAM_PROTECTOR_ST).cast::<u8>(), ROW_SIZE) };
        (0..ROW_SIZE - 1).filter(|&i| bytes[i] == 0xEF && bytes[i + 1] == 0xBE).collect()
    }

    #[test]
    fn fields_are_the_u16s_of_the_paramdef() {
        assert_eq!(std::mem::size_of::<EQUIP_PARAM_PROTECTOR_ST>(), ROW_SIZE);
        for (f, at) in [(Field::Model, 0xA0), (Field::ResistPoison, 0xC0), (Field::ResistDisease, 0xC2), (Field::ResistBlood, 0xC4)] {
            assert_eq!(offset_of(f), vec![at], "{f:?}");
        }
    }

    /// Two outfits never patch the same row or ship the same file, and every row's new
    /// model id has its files.
    #[test]
    fn outfits_do_not_overlap() {
        let mut rows = Vec::new();
        let mut files = Vec::new();
        for s in ALL {
            for r in s.rows {
                assert!(!rows.contains(&r.row), "row {} twice", r.row);
                rows.push(r.row);
            }
            for f in s.files {
                assert!(!files.contains(f), "{f} twice");
                files.push(*f);
                assert!(f.starts_with(r"mod\parts\") && f.ends_with(".partsbnd.dcx"), "{f}");
            }
            assert!(!s.tag.is_empty() && !s.name.is_empty());
        }
    }

    #[test]
    fn fallbacks_chain_over_every_outfit() {
        // Nothing here: a friend wearing a mix of both outfits is drawn as vanilla outfits.
        let mut ids = [-1i32; 25];
        ids[12] = P | 600_000; // Red head
        ids[13] = P | 641_000; // Reed torso
        ids[14] = P | 602_000;
        ids[15] = P | 603_000;
        let mut out: Option<[i32; 25]> = None;
        for s in ALL {
            if let Some(n) = s.replica_ids(out.as_ref().unwrap_or(&ids), false) {
                out = Some(n);
            }
        }
        assert_eq!(&out.unwrap()[12..16], &[P | 400_000, P | 101_000, P | 102_000, P | 103_000]);
    }
}
