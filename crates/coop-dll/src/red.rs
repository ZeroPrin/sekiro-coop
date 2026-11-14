//! The Red Armour outfit (wardrobe skin "Red Armour" and head "Red"; design
//! `E:\RE\work_skin_red\SPEC.md` section 6).
//!
//! Red Armour is a recoloured Ashina outfit (red lacquer) shipped as eight part files with
//! part ids no vanilla file has (`bd_m_9125`, `am_m_9125`, `lg_m_9125`, `fc_m_0225`, each with
//! `_l`) in the optional parts package. Like the Ashina outfit it is four parts with one torso
//! for every sword state. The DLL points four never-owned EquipParamProtector rows at the new
//! ids, in memory only (`partskin.rs`, the same rules as Reed) [VAR rows: `EquipParamProtector.param`,
//! 2026-11-15]:
//!
//! | row    | vanilla | Red  | role                     | notes                                          |
//! |--------|---------|------|--------------------------|------------------------------------------------|
//! | 901200 | 9530    | 9125 | torso, every sword state | debug torso (CharaInitParam 9610 only); its resistPoison / Disease / Blood 16 / 16 / 23 are set to 0 like the Ashina torsos (the row then equals 431000 but for ids and icons) |
//! | 202000 | 9100    | 9125 | prosthetic arm           | the Sculptor's (CharaInitParam 15010); otherwise identical to 402000 |
//! | 203000 | 9100    | 9125 | legs                     | the Sculptor's (CharaInitParam 15010); otherwise identical to 403000 but the icon |
//! | 600000 | 220     | 225  | head (`fc_m_0225`)       | a duplicate of the Ashina head 400000 (only `wanderingEquipId` differs) |
//!
//! The head carries its own face and hair like the Ashina head (hair slot empty). Cosmetic
//! side effect: while patched, the Sculptor's ending scene (CharaInitParam 15010) shows the
//! Red arm and legs (and Reed's torso when Reed is patched too). A friend's Red rows on a
//! machine without Red are drawn as the Ashina outfit (`FALLBACK`).
//!
//! NOT verified live: the DLL patch itself (the files were checked in game under the vanilla
//! Ashina ids, SPEC section 7 / 10).

use crate::partskin::{Fallback, Field, FieldPatch, Def, PartSkin, PatchRow};

pub static RED: PartSkin = PartSkin::new(Def {
    tag: "red",
    name: "Red Armour",
    offers: "skin and head",
    drawn_as: "Ashina",
    rows: &ROWS,
    files: &FILES,
    fallback: &FALLBACK,
});

/// The torso row (all sword states).
pub const TORSO: i32 = 901_200;
pub const ARM: i32 = 202_000;
pub const LEGS: i32 = 203_000;
pub const HEAD: i32 = 600_000;

/// 901200's resistances, set to the Ashina torsos' 0 (gameplay-neutral row).
const TORSO_ALSO: [FieldPatch; 3] = [
    FieldPatch { field: Field::ResistPoison, vanilla: 16, value: 0 },
    FieldPatch { field: Field::ResistDisease, vanilla: 16, value: 0 },
    FieldPatch { field: Field::ResistBlood, vanilla: 23, value: 0 },
];

pub const ROWS: [PatchRow; 4] = [
    PatchRow { row: TORSO as u32, vanilla: 9530, model: 9125, role: "torso", also: &TORSO_ALSO },
    PatchRow { row: ARM as u32, vanilla: 9100, model: 9125, role: "prosthetic arm", also: &[] },
    PatchRow { row: LEGS as u32, vanilla: 9100, model: 9125, role: "legs", also: &[] },
    PatchRow { row: HEAD as u32, vanilla: 220, model: 225, role: "head", also: &[] },
];

/// The part files, relative to the DLL's folder (the package's `mod\` is the game's root).
pub const FILES: [&str; 8] = [
    r"mod\parts\bd_m_9125.partsbnd.dcx",
    r"mod\parts\bd_m_9125_l.partsbnd.dcx",
    r"mod\parts\am_m_9125.partsbnd.dcx",
    r"mod\parts\am_m_9125_l.partsbnd.dcx",
    r"mod\parts\lg_m_9125.partsbnd.dcx",
    r"mod\parts\lg_m_9125_l.partsbnd.dcx",
    r"mod\parts\fc_m_0225.partsbnd.dcx",
    r"mod\parts\fc_m_0225_l.partsbnd.dcx",
];

/// Without Red the rows would show the debug torso, the Sculptor's arm / legs and (the head
/// only) still the Ashina head: they become the Ashina outfit's. The head and the torso always
/// (no other user of them); the arm and legs only with Red's torso (alone they are the
/// Sculptor's).
pub const FALLBACK: [Fallback; 4] = [
    Fallback { slot: 12, from: HEAD, to: 400_000, with: None },
    Fallback { slot: 13, from: TORSO, to: 431_000, with: None },
    Fallback { slot: 14, from: ARM, to: 402_000, with: Some((13, TORSO)) },
    Fallback { slot: 15, from: LEGS, to: 403_000, with: Some((13, TORSO)) },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::partskin::RowNow;

    const P: i32 = 0x1000_0000;

    #[test]
    fn files_match_the_red_model_ids() {
        for (prefix, row) in [("bd", TORSO), ("am", ARM), ("lg", LEGS), ("fc", HEAD)] {
            let id = ROWS.iter().find(|r| r.row == row as u32).unwrap().model;
            for l in ["", "_l"] {
                let f = format!(r"mod\parts\{prefix}_m_{id:04}{l}.partsbnd.dcx");
                assert!(FILES.contains(&f.as_str()), "{f}");
            }
        }
        assert_eq!(RED.missing_files(|_| false).len(), 8);
        assert_eq!(RED.missing_files(|f| !f.ends_with("fc_m_0225_l.partsbnd.dcx")), vec![r"mod\parts\fc_m_0225_l.partsbnd.dcx"]);
    }

    /// The values of `EquipParamProtector.param` (SPEC section 6, re-read 2026-11-15).
    fn vanilla(row: u32, f: Field) -> Option<u16> {
        Some(match (row, f) {
            (901_200, Field::Model) => 9530,
            (901_200, Field::ResistPoison | Field::ResistDisease) => 16,
            (901_200, Field::ResistBlood) => 23,
            (202_000 | 203_000, Field::Model) => 9100,
            (600_000, Field::Model) => 220,
            _ => return None,
        })
    }

    fn red(row: u32, f: Field) -> Option<u16> {
        Some(match (row, f) {
            (901_200 | 202_000 | 203_000, Field::Model) => 9125,
            (901_200, _) => 0,
            (600_000, Field::Model) => 225,
            _ => return None,
        })
    }

    #[test]
    fn rows_and_resistances_are_checked_before_any_write() {
        assert_eq!(RED.check_rows(vanilla), Ok(vec![RowNow::Vanilla; 4]));
        assert_eq!(RED.check_rows(red), Ok(vec![RowNow::Patched; 4]));
        // Model already Red but a resistance still vanilla: written again (not "already").
        let half = |r: u32, f: Field| if (r, f) == (901_200, Field::ResistBlood) { Some(23) } else { red(r, f) };
        assert_eq!(RED.check_rows(half), Ok(vec![RowNow::Vanilla, RowNow::Patched, RowNow::Patched, RowNow::Patched]));
        // Another mod changed a resistance: refused, naming the field.
        let other = |r: u32, f: Field| if (r, f) == (901_200, Field::ResistDisease) { Some(40) } else { vanilla(r, f) };
        let e = RED.check_rows(other).unwrap_err();
        assert!(e.contains("901200") && e.contains("resistDisease 40") && e.contains("Red Armour"), "{e}");
        // Another model on the head row: refused.
        let head = |r: u32, f: Field| if r == 600_000 { Some(230) } else { vanilla(r, f) };
        assert!(RED.check_rows(head).unwrap_err().contains("equipModelId 230"));
        assert!(RED.check_rows(|r, f| if r == 203_000 { None } else { vanilla(r, f) }).unwrap_err().contains("203000"));
    }

    #[test]
    fn replica_without_red_wears_ashina() {
        let mut ids = [-1i32; 25];
        ids[12] = P | HEAD;
        ids[13] = P | TORSO;
        ids[14] = P | ARM;
        ids[15] = P | LEGS;
        assert_eq!(RED.replica_ids(&ids, true), None, "Red here: as sent");
        let a = RED.replica_ids(&ids, false).unwrap();
        assert_eq!(&a[12..16], &[P | 400_000, P | 431_000, P | 402_000, P | 403_000]);
        // The Red head on the Wolf: only the head changes.
        let mut wolf = [-1i32; 25];
        wolf[12..16].copy_from_slice(&[P | HEAD, P | 131_000, P | 102_000, P | 103_000]);
        assert_eq!(&RED.replica_ids(&wolf, false).unwrap()[12..16], &[P | 400_000, P | 131_000, P | 102_000, P | 103_000]);
        // Red Armour before the prosthetic (bare arm) with the Wolf's head.
        let mut early = ids;
        early[12] = P | 100_000;
        early[14] = P | 112_000;
        assert_eq!(&RED.replica_ids(&early, false).unwrap()[12..16], &[P | 100_000, P | 431_000, P | 112_000, P | 403_000]);
        // The Sculptor's arm / legs without Red's torso: left alone.
        let mut sculptor = [-1i32; 25];
        sculptor[12..16].copy_from_slice(&[P | 200_000, P | 201_000, P | ARM, P | LEGS]);
        assert_eq!(RED.replica_ids(&sculptor, false), None);
        // Only protector slots: a weapon id with the same number stays.
        let mut weapon = [-1i32; 25];
        weapon[0] = TORSO;
        assert_eq!(RED.replica_ids(&weapon, false), None);
    }
}
