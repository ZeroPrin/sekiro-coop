//! The Reed outfit (wardrobe skin "Reed"; design `E:\RE\work_skin\game\SPEC.md` section 1).
//!
//! Reed is a recoloured Wolf outfit shipped as eight part files with part ids no vanilla
//! file has (`bd_m_9045`, `bd_m_9005`, `am_m_9005`, `lg_m_9005`, each with `_l`) in the
//! optional parts package (`<dll dir>\mod\parts\`, FORK.md "Parts package"). The game builds a
//! part path from the protector row's `equipModelId` (`parts:/BD_M_%04d.partsbnd.dcx`), so
//! the DLL points four existing, never-owned EquipParamProtector rows at the new ids, in
//! memory only (no new rows: a save stays valid without the mod):
//!
//! | row    | vanilla | Reed | role                               |
//! |--------|---------|------|------------------------------------|
//! | 641000 | 9120    | 9045 | torso with the Mortal Blade        |
//! | 201000 | 9100    | 9005 | torso with the sword / no sword    |
//! | 602000 | 9120    | 9005 | prosthetic arm                     |
//! | 603000 | 9120    | 9005 | legs                               |
//!
//! Rules (SPEC 1.3): patch on every machine, once the params are loaded and before the first
//! world load (the frame task runs from the title screen); only when all eight files exist
//! (a patched row without its file makes the game ask for a path nobody serves); only when
//! every row holds its vanilla value (or already the Reed one): an unexpected value turns
//! Reed off for the run and nothing is written. Each machine renders the friend's character
//! from its own param, so a friend's Reed rows on a machine without Reed are drawn as the
//! Wolf (`replica_ids`).
//!
//! Verified in game (2026-11-14, user): FLVER albedo paths override MTD paths. NOT verified:
//! a part id that exists only in the package (first DLL test), `_l` low detail.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering::Relaxed};

use fromsoftware_shared::FromStatic;
use sekiro::param::{EQUIP_PARAM_PROTECTOR_ST, ParamDef};
use sekiro::sprj::SoloParamRepository;

use crate::log;

/// One protector row the patch repoints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatchRow {
    pub row: u32,
    pub vanilla: u16,
    pub reed: u16,
    pub role: &'static str,
}

pub const ROWS: [PatchRow; 4] = [
    PatchRow { row: 641_000, vanilla: 9120, reed: 9045, role: "torso, Mortal Blade" },
    PatchRow { row: 201_000, vanilla: 9100, reed: 9005, role: "torso, sword / no sword" },
    PatchRow { row: 602_000, vanilla: 9120, reed: 9005, role: "prosthetic arm" },
    PatchRow { row: 603_000, vanilla: 9120, reed: 9005, role: "legs" },
];

/// The part files, relative to the DLL's folder (the package's `mod\` is the game's root).
pub const FILES: [&str; 8] = [
    r"mod\parts\bd_m_9045.partsbnd.dcx",
    r"mod\parts\bd_m_9045_l.partsbnd.dcx",
    r"mod\parts\bd_m_9005.partsbnd.dcx",
    r"mod\parts\bd_m_9005_l.partsbnd.dcx",
    r"mod\parts\am_m_9005.partsbnd.dcx",
    r"mod\parts\am_m_9005_l.partsbnd.dcx",
    r"mod\parts\lg_m_9005.partsbnd.dcx",
    r"mod\parts\lg_m_9005_l.partsbnd.dcx",
];

/// EquipParamProtector row size (SPEC 1.3: 0x1FC, `equipModelId` u16 at 0xA0).
const ROW_SIZE: usize = 0x1FC;
/// SoloParamRepository holder of EquipParamProtector (vendored `solo_param_repository.rs`).
const PROTECTOR_HOLDER: usize = 1;

// ===================================================================== pure, unit-tested

/// The files of `FILES` that `exists` says are missing.
pub fn missing_files(exists: impl Fn(&str) -> bool) -> Vec<&'static str> {
    FILES.into_iter().filter(|f| !exists(f)).collect()
}

/// What a row holds now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowNow {
    Vanilla,
    /// Already the Reed id (a second run of the patch, or another patcher did the same).
    Patched,
}

/// Checks every row before anything is written: all must be vanilla or already Reed.
/// `model(row)` = the row's `equipModelId`, None = no such row.
pub fn check_rows(model: impl Fn(u32) -> Option<u16>) -> Result<[RowNow; 4], String> {
    let mut out = [RowNow::Vanilla; 4];
    for (o, r) in out.iter_mut().zip(ROWS) {
        *o = match model(r.row) {
            None => return Err(format!("row {} is not in EquipParamProtector", r.row)),
            Some(m) if m == r.vanilla => RowNow::Vanilla,
            Some(m) if m == r.reed => RowNow::Patched,
            Some(m) => {
                return Err(format!("row {} ({}) has equipModelId {m}, expected {} (vanilla) or {} (Reed)", r.row, r.role, r.vanilla, r.reed));
            }
        };
    }
    Ok(out)
}

/// A friend's equipped ids (`Loadout.slots`, category bits included) as this machine can draw
/// them. With Reed here (`reed`) they are drawn as sent. Without it the Reed rows would show
/// their vanilla models (Ashina-ninja torso / arm / legs, the Sculptor's cutscene torso), so
/// they become the Wolf's: 641000 -> 101000, 602000 -> 102000, 603000 -> 103000, and 201000
/// -> 131000 when it comes with Reed's legs (alone it is an older build's Sculptor torso).
/// None = nothing to change.
pub fn replica_ids(ids: &[i32; 25], reed: bool) -> Option<[i32; 25]> {
    if reed {
        return None;
    }
    const P: i32 = 0x1000_0000;
    let reed_legs = ids[15] == P | 603_000;
    let mut out = *ids;
    for (slot, from, to) in [(13, 641_000, 101_000), (13, 201_000, 131_000), (14, 602_000, 102_000), (15, 603_000, 103_000)] {
        if out[slot] == P | from && (from != 201_000 || reed_legs) {
            out[slot] = P | to;
        }
    }
    (out != *ids).then_some(out)
}

// ===================================================================== game side

const PENDING: u8 = 0;
const WAIT_PARAMS: u8 = 1;
const READY: u8 = 2;
const OFF: u8 = 3;

static STATE: AtomicU8 = AtomicU8::new(PENDING);
static REPLICA_LOGGED: AtomicBool = AtomicBool::new(false);

/// Reed can be worn and drawn on this machine: files present and rows patched.
pub fn available() -> bool {
    STATE.load(Relaxed) == READY
}

fn off(why: &str) {
    STATE.store(OFF, Relaxed);
    log::line(&format!("reed: skin not offered: {why}"));
}

/// lib.rs frame task, every frame from the title screen (game thread). Cheap once settled.
pub fn tick() {
    match STATE.load(Relaxed) {
        PENDING => check_files(),
        WAIT_PARAMS => patch(),
        _ => {}
    }
}

fn check_files() {
    let Some(dir) = crate::settings::own_dll_path().and_then(|p| p.parent().map(std::path::Path::to_path_buf)) else {
        return off("cannot find the DLL's folder");
    };
    let missing = missing_files(|f| dir.join(f).is_file());
    if !missing.is_empty() {
        return off(&format!(
            "{} of {} part files missing in {} (first: {}); param rows left vanilla",
            missing.len(),
            FILES.len(),
            dir.join(r"mod\parts").display(),
            missing[0]
        ));
    }
    log::line(&format!("reed: all {} part files present in {}", FILES.len(), dir.join(r"mod\parts").display()));
    STATE.store(WAIT_PARAMS, Relaxed);
}

fn patch() {
    if !crate::items::solo_param_ready(PROTECTOR_HOLDER) {
        return;
    }
    if std::mem::size_of::<EQUIP_PARAM_PROTECTOR_ST>() != ROW_SIZE {
        return off("EquipParamProtector row size is not 0x1FC (crate / game version?); param rows left vanilla");
    }
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return };
    let Some(file) = repo.params_mut().find(|p| p.struct_name() == EQUIP_PARAM_PROTECTOR_ST::NAME).map(|p| &mut *p.data) else {
        return;
    };
    // SAFETY: the file is EquipParamProtector (struct name), rows are that struct.
    let now = match check_rows(|id| unsafe { file.get_row_by_id::<EQUIP_PARAM_PROTECTOR_ST>(id) }.map(|r| r.equip_model_id())) {
        Ok(n) => n,
        Err(e) => return off(&format!("{e}; nothing written")),
    };
    let mut done = Vec::new();
    for (r, n) in ROWS.into_iter().zip(now) {
        let Some(row) = (unsafe { file.get_row_by_id_mut::<EQUIP_PARAM_PROTECTOR_ST>(r.row) }) else {
            return off(&format!("row {} vanished between check and write", r.row));
        };
        if n == RowNow::Vanilla {
            row.set_equip_model_id(r.reed);
        }
        if row.equip_model_id() != r.reed {
            return off(&format!("row {} reads {} after the write", r.row, row.equip_model_id()));
        }
        done.push(format!("{} {}->{}{}", r.row, r.vanilla, r.reed, if n == RowNow::Patched { " (already)" } else { "" }));
    }
    STATE.store(READY, Relaxed);
    log::line(&format!(
        "reed: EquipParamProtector equipModelId patched in memory: {}; skin offered{}",
        done.join(", "),
        if crate::game::player().is_some() {
            " (patched while in the world: parts already loaded keep their model until the next load)"
        } else {
            ""
        }
    ));
}

/// loadout.rs `apply`: the ids to put on a friend's replica (see `replica_ids`). Logs the
/// first fallback of the run.
pub fn for_replica(ids: &[i32; 25], who: &str) -> Option<[i32; 25]> {
    let out = replica_ids(ids, available())?;
    if !REPLICA_LOGGED.swap(true, Relaxed) {
        log::line(&format!(
            "reed: {who} wears Reed but Reed is not available on this PC (state {}): drawn as the Wolf (logged once)",
            match STATE.load(Relaxed) {
                PENDING | WAIT_PARAMS => "params not patched yet",
                _ => "files missing or patch refused, see the reed: lines above",
            }
        ));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: i32 = 0x1000_0000;

    #[test]
    fn files_match_the_reed_model_ids() {
        // Every Reed model id has its full and `_l` file under the row's category prefix.
        for (prefix, row) in [("bd", 641_000), ("bd", 201_000), ("am", 602_000), ("lg", 603_000)] {
            let id = ROWS.iter().find(|r| r.row == row).unwrap().reed;
            for l in ["", "_l"] {
                let f = format!(r"mod\parts\{prefix}_m_{id}{l}.partsbnd.dcx");
                assert!(FILES.contains(&f.as_str()), "{f}");
            }
        }
        assert!(missing_files(|_| true).is_empty());
        assert_eq!(missing_files(|_| false).len(), 8);
        assert_eq!(missing_files(|f| !f.ends_with("am_m_9005_l.partsbnd.dcx")), vec![r"mod\parts\am_m_9005_l.partsbnd.dcx"]);
    }

    #[test]
    fn equip_model_id_is_the_u16_at_0xa0() {
        assert_eq!(std::mem::size_of::<EQUIP_PARAM_PROTECTOR_ST>(), ROW_SIZE);
        // SAFETY: a plain-data param row; all-zero bytes are a valid value.
        let mut row: EQUIP_PARAM_PROTECTOR_ST = unsafe { std::mem::zeroed() };
        row.set_equip_model_id(0xBEEF);
        let bytes = unsafe { std::slice::from_raw_parts((&row as *const EQUIP_PARAM_PROTECTOR_ST).cast::<u8>(), ROW_SIZE) };
        let at: Vec<usize> = (0..ROW_SIZE - 1).filter(|&i| bytes[i] == 0xEF && bytes[i + 1] == 0xBE).collect();
        assert_eq!(at, vec![0xA0]);
    }

    #[test]
    fn rows_are_checked_before_any_write() {
        let vanilla = |id: u32| ROWS.iter().find(|r| r.row == id).map(|r| r.vanilla);
        assert_eq!(check_rows(vanilla), Ok([RowNow::Vanilla; 4]));
        let patched = |id: u32| ROWS.iter().find(|r| r.row == id).map(|r| r.reed);
        assert_eq!(check_rows(patched), Ok([RowNow::Patched; 4]));
        // Mixed (idempotent re-run after a partial state).
        let mixed = |id: u32| ROWS.iter().find(|r| r.row == id).map(|r| if id == 602_000 { r.reed } else { r.vanilla });
        assert_eq!(check_rows(mixed), Ok([RowNow::Vanilla, RowNow::Vanilla, RowNow::Patched, RowNow::Vanilla]));
        // Another mod changed a row: refused.
        let other = |id: u32| ROWS.iter().find(|r| r.row == id).map(|r| if id == 201_000 { 9130 } else { r.vanilla });
        let e = check_rows(other).unwrap_err();
        assert!(e.contains("201000") && e.contains("9130"), "{e}");
        // A missing row: refused.
        assert!(check_rows(|id| if id == 603_000 { None } else { vanilla(id) }).unwrap_err().contains("603000"));
    }

    #[test]
    fn replica_without_reed_wears_the_wolf() {
        let mut ids = [-1i32; 25];
        ids[12] = P | 100_000;
        ids[13] = P | 641_000;
        ids[14] = P | 602_000;
        ids[15] = P | 603_000;
        assert_eq!(replica_ids(&ids, true), None, "Reed here: as sent");
        let w = replica_ids(&ids, false).unwrap();
        assert_eq!(&w[12..16], &[P | 100_000, P | 101_000, P | 102_000, P | 103_000]);
        // The sword torso with Reed's legs.
        ids[13] = P | 201_000;
        assert_eq!(replica_ids(&ids, false).unwrap()[13], P | 131_000);
        // 201000 without Reed's legs (an older build's Sculptor): left alone.
        let mut sculptor = ids;
        sculptor[15] = P | 203_000;
        sculptor[14] = P | 102_000;
        assert_eq!(replica_ids(&sculptor, false), None);
        // A Wolf / Tengu outfit: nothing to change.
        let mut wolf = [-1i32; 25];
        wolf[12..16].copy_from_slice(&[P | 100_000, P | 131_000, P | 102_000, P | 103_000]);
        assert_eq!(replica_ids(&wolf, false), None);
        // Only protector slots are touched (a weapon id with the same number stays).
        let mut weapon = wolf;
        weapon[0] = 602_000;
        assert_eq!(replica_ids(&weapon, false), None);
    }
}
