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
//! Rules (SPEC 1.3, shared with the other part-file outfits: `partskin.rs`): patch on every
//! machine, once the params are loaded and before the first world load; only when all eight
//! files exist; only when every row holds its vanilla value (or already the Reed one): an
//! unexpected value turns Reed off for the run and nothing is written. Each machine renders
//! the friend's character from its own param, so a friend's Reed rows on a machine without
//! Reed are drawn as the Wolf (`FALLBACK`).
//!
//! Verified in game (2026-11-14, user): FLVER albedo paths override MTD paths. NOT verified:
//! a part id that exists only in the package (first DLL test), `_l` low detail.

use crate::partskin::{Def, Fallback, PartSkin, PatchRow};

pub static REED: PartSkin = PartSkin::new(Def {
    tag: "reed",
    name: "Reed",
    offers: "skin",
    drawn_as: "the Wolf",
    rows: &ROWS,
    files: &FILES,
    fallback: &FALLBACK,
});

pub const ROWS: [PatchRow; 4] = [
    PatchRow { row: 641_000, vanilla: 9120, model: 9045, role: "torso, Mortal Blade", also: &[] },
    PatchRow { row: 201_000, vanilla: 9100, model: 9005, role: "torso, sword / no sword", also: &[] },
    PatchRow { row: 602_000, vanilla: 9120, model: 9005, role: "prosthetic arm", also: &[] },
    PatchRow { row: 603_000, vanilla: 9120, model: 9005, role: "legs", also: &[] },
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

/// A friend's Reed rows on a machine without Reed would show their vanilla models
/// (Ashina-ninja torso / arm / legs, the Sculptor's cutscene torso), so they become the
/// Wolf's: 641000 -> 101000, 602000 -> 102000, 603000 -> 103000, and 201000 -> 131000 when it
/// comes with Reed's legs (alone it is an older build's Sculptor torso).
pub const FALLBACK: [Fallback; 4] = [
    Fallback { slot: 13, from: 641_000, to: 101_000, with: None },
    Fallback { slot: 13, from: 201_000, to: 131_000, with: Some((15, 603_000)) },
    Fallback { slot: 14, from: 602_000, to: 102_000, with: None },
    Fallback { slot: 15, from: 603_000, to: 103_000, with: None },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::partskin::{ROW_SIZE, RowNow};
    use sekiro::param::EQUIP_PARAM_PROTECTOR_ST;

    const P: i32 = 0x1000_0000;

    #[test]
    fn files_match_the_reed_model_ids() {
        // Every Reed model id has its full and `_l` file under the row's category prefix.
        for (prefix, row) in [("bd", 641_000), ("bd", 201_000), ("am", 602_000), ("lg", 603_000)] {
            let id = ROWS.iter().find(|r| r.row == row).unwrap().model;
            for l in ["", "_l"] {
                let f = format!(r"mod\parts\{prefix}_m_{id}{l}.partsbnd.dcx");
                assert!(FILES.contains(&f.as_str()), "{f}");
            }
        }
        assert!(REED.missing_files(|_| true).is_empty());
        assert_eq!(REED.missing_files(|_| false).len(), 8);
        assert_eq!(REED.missing_files(|f| !f.ends_with("am_m_9005_l.partsbnd.dcx")), vec![r"mod\parts\am_m_9005_l.partsbnd.dcx"]);
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
        assert_eq!(REED.check_rows(|id, _| vanilla(id)), Ok(vec![RowNow::Vanilla; 4]));
        let patched = |id: u32| ROWS.iter().find(|r| r.row == id).map(|r| r.model);
        assert_eq!(REED.check_rows(|id, _| patched(id)), Ok(vec![RowNow::Patched; 4]));
        // Mixed (idempotent re-run after a partial state).
        let mixed = |id: u32| ROWS.iter().find(|r| r.row == id).map(|r| if id == 602_000 { r.model } else { r.vanilla });
        assert_eq!(REED.check_rows(|id, _| mixed(id)), Ok(vec![RowNow::Vanilla, RowNow::Vanilla, RowNow::Patched, RowNow::Vanilla]));
        // Another mod changed a row: refused.
        let other = |id: u32| ROWS.iter().find(|r| r.row == id).map(|r| if id == 201_000 { 9130 } else { r.vanilla });
        let e = REED.check_rows(|id, _| other(id)).unwrap_err();
        assert!(e.contains("201000") && e.contains("9130"), "{e}");
        // A missing row: refused.
        assert!(REED.check_rows(|id, _| if id == 603_000 { None } else { vanilla(id) }).unwrap_err().contains("603000"));
    }

    #[test]
    fn replica_without_reed_wears_the_wolf() {
        let mut ids = [-1i32; 25];
        ids[12] = P | 100_000;
        ids[13] = P | 641_000;
        ids[14] = P | 602_000;
        ids[15] = P | 603_000;
        assert_eq!(REED.replica_ids(&ids, true), None, "Reed here: as sent");
        let w = REED.replica_ids(&ids, false).unwrap();
        assert_eq!(&w[12..16], &[P | 100_000, P | 101_000, P | 102_000, P | 103_000]);
        // The sword torso with Reed's legs.
        ids[13] = P | 201_000;
        assert_eq!(REED.replica_ids(&ids, false).unwrap()[13], P | 131_000);
        // 201000 without Reed's legs (an older build's Sculptor): left alone.
        let mut sculptor = ids;
        sculptor[15] = P | 203_000;
        sculptor[14] = P | 102_000;
        assert_eq!(REED.replica_ids(&sculptor, false), None);
        // A Wolf / Tengu outfit: nothing to change.
        let mut wolf = [-1i32; 25];
        wolf[12..16].copy_from_slice(&[P | 100_000, P | 131_000, P | 102_000, P | 103_000]);
        assert_eq!(REED.replica_ids(&wolf, false), None);
        // Only protector slots are touched (a weapon id with the same number stays).
        let mut weapon = wolf;
        weapon[0] = 602_000;
        assert_eq!(REED.replica_ids(&weapon, false), None);
    }
}
