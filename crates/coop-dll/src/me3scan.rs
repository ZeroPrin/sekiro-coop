//! me3 file mods next to ours (1.0.11 doctor `pkgs=`, ported): the me3 profile that loads
//! `sekiro_coop.dll` (a `*.me3` next to the DLL) lists `[[packages]]` (loose-file mods that
//! replace game files) and `[[natives]]` (other DLLs). Two players with different file mods
//! can run different params / maps / scripts: the worlds can go out of sync (live 2026-11-08:
//! the friend ran `ps5-glyphs`, the host nothing).
//!
//! Scanned once on a thread of its own at start; the doctor sends `pkgs=` (count and a
//! fingerprint of every enabled package's files), `pkg_names=` and `natives=` and warns on a
//! difference (doctor.rs `compare`). Nothing here changes the game.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

use crate::log;

/// Files up to this size are hashed whole; bigger ones by size + their first and last MB.
const HASH_WHOLE_MAX: u64 = 8 << 20;
const EDGE: usize = 1 << 20;
/// A package folder with more files than this is fingerprinted by names and sizes only.
const MAX_FILES: usize = 20_000;

/// One me3 profile, as far as the doctor needs it.
#[derive(Debug, Default, PartialEq)]
pub struct Profile {
    /// Enabled packages: (id or folder name, path as written).
    pub packages: Vec<(String, String)>,
    /// Enabled natives other than ours (file names).
    pub natives: Vec<String>,
    /// Our DLL is one of its natives.
    pub loads_us: bool,
}

/// `key = "value"` / `key = value` of one TOML line (comments and tables are not values).
fn kv(line: &str) -> Option<(&str, &str)> {
    let line = line.split_once('#').map_or(line, |(a, _)| a).trim();
    let (k, v) = line.split_once('=')?;
    Some((k.trim(), v.trim().trim_matches('"').trim_matches('\'')))
}

/// The last path component, `/` or `\` separated.
fn file_name(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

/// Reads the parts of a me3 profile the doctor uses. Packages name their folder with
/// `path` (or `source` in older profiles) and may carry an `id`; `enabled = false` drops one.
pub fn parse_profile(text: &str, our_dll: &str) -> Profile {
    #[derive(PartialEq)]
    enum Table {
        Other,
        Package,
        Native,
    }
    #[derive(Default)]
    struct Entry {
        id: Option<String>,
        path: Option<String>,
        enabled: bool,
    }
    let mut out = Profile::default();
    let mut table = Table::Other;
    let mut cur = Entry { enabled: true, ..Default::default() };
    let flush = |table: &Table, e: &mut Entry, out: &mut Profile| {
        let taken = std::mem::replace(e, Entry { enabled: true, ..Default::default() });
        let Some(path) = taken.path else { return };
        match table {
            Table::Package if taken.enabled => {
                let name = taken.id.unwrap_or_else(|| file_name(&path).to_string());
                out.packages.push((name, path));
            }
            Table::Native if taken.enabled => {
                let name = file_name(&path).to_string();
                if name.eq_ignore_ascii_case(our_dll) {
                    out.loads_us = true;
                } else {
                    out.natives.push(name);
                }
            }
            _ => {}
        }
    };
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            flush(&table, &mut cur, &mut out);
            table = match line {
                "[[packages]]" | "[[package]]" => Table::Package,
                "[[natives]]" | "[[native]]" => Table::Native,
                _ => Table::Other,
            };
            continue;
        }
        if table == Table::Other {
            continue;
        }
        let Some((k, v)) = kv(line) else { continue };
        match k {
            "id" => cur.id = Some(v.to_string()),
            "path" | "source" => cur.path = Some(v.to_string()),
            "enabled" => cur.enabled = v != "false",
            _ => {}
        }
    }
    flush(&table, &mut cur, &mut out);
    out.packages.sort();
    out.natives.sort();
    out
}

/// FNV-1a 64 (a fingerprint between two friends' files, not a security check).
fn fnv(h: &mut u64, data: &[u8]) {
    for &b in data {
        *h ^= u64::from(b);
        *h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
}

const FNV_START: u64 = 0xcbf2_9ce4_8422_2325;

/// Every file under `dir` (relative path with `/`, lower case; full path), sorted.
fn walk(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        if out.len() > MAX_FILES {
            return;
        }
        let name = e.file_name().to_string_lossy().to_lowercase();
        let r = if rel.is_empty() { name } else { format!("{rel}/{name}") };
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(&e.path(), &r, out),
            Ok(_) => out.push((r, e.path())),
            Err(_) => {}
        }
    }
}

/// The scan's result.
#[derive(Debug, Clone, PartialEq)]
pub struct Scan {
    /// `none` or `<packages>:<fingerprint hex>`; `?` = no profile found.
    pub pkgs: String,
    pub pkg_names: String,
    pub natives: String,
}

/// The doctor's value for a list (`none` when empty).
fn list(v: &[String]) -> String {
    if v.is_empty() { "none".to_string() } else { v.join(",") }
}

/// Fingerprint of the enabled packages' files (paths relative to the profile's folder).
fn fingerprint(base: &Path, p: &Profile) -> (String, usize, u64, usize) {
    if p.packages.is_empty() {
        return ("none".to_string(), 0, 0, 0);
    }
    let mut h = FNV_START;
    let (mut files, mut bytes, mut hashed) = (0usize, 0u64, 0usize);
    for (name, path) in &p.packages {
        fnv(&mut h, name.to_lowercase().as_bytes());
        let root = base.join(path);
        let mut all = Vec::new();
        if root.is_file() {
            all.push((file_name(path).to_lowercase(), root.clone()));
        } else {
            walk(&root, "", &mut all);
        }
        all.sort();
        let names_only = all.len() > MAX_FILES;
        for (rel, full) in all {
            let size = fs::metadata(&full).map(|m| m.len()).unwrap_or(0);
            files += 1;
            bytes += size;
            fnv(&mut h, rel.as_bytes());
            fnv(&mut h, &size.to_le_bytes());
            if names_only {
                continue;
            }
            let Ok(data) = (if size <= HASH_WHOLE_MAX { fs::read(&full) } else { read_edges(&full, size) }) else { continue };
            fnv(&mut h, &data);
            hashed += 1;
        }
    }
    (format!("{}:{h:016x}", p.packages.len()), files, bytes, hashed)
}

fn read_edges(path: &Path, size: u64) -> std::io::Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = fs::File::open(path)?;
    let mut buf = vec![0u8; EDGE];
    f.read_exact(&mut buf)?;
    let mut tail = vec![0u8; EDGE];
    f.seek(SeekFrom::Start(size - EDGE as u64))?;
    f.read_exact(&mut tail)?;
    buf.extend_from_slice(&tail);
    Ok(buf)
}

static RESULT: OnceLock<Scan> = OnceLock::new();

/// The scan, once it finished (doctor.rs; `pkgs=?` meanwhile).
pub fn result() -> Option<&'static Scan> {
    RESULT.get()
}

fn scan_now() -> Scan {
    let unknown = |why: &str| {
        log::line(&format!("doctor: me3 scan: {why}"));
        Scan { pkgs: "?".into(), pkg_names: "?".into(), natives: "?".into() }
    };
    let Some(dll) = crate::settings::own_dll_path() else { return unknown("cannot find our DLL's folder") };
    let (Some(dir), Some(ours)) = (dll.parent(), dll.file_name().map(|n| n.to_string_lossy().into_owned())) else {
        return unknown("cannot find our DLL's folder");
    };
    let Ok(rd) = fs::read_dir(dir) else { return unknown("cannot read our DLL's folder") };
    let mut profiles: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("me3"))).collect();
    profiles.sort();
    let found = profiles.iter().find_map(|p| {
        let text = fs::read_to_string(p).ok()?;
        let prof = parse_profile(&text, &ours);
        prof.loads_us.then_some((p.clone(), prof))
    });
    let Some((path, prof)) = found else { return unknown(&format!("no me3 profile for {ours} in {}", dir.display())) };
    let t0 = Instant::now();
    let (pkgs, files, bytes, hashed) = fingerprint(dir, &prof);
    let names: Vec<String> = prof.packages.iter().map(|(n, _)| n.clone()).collect();
    log::line(&format!(
        "doctor: me3 profile {}: {} package(s) {names:?}, natives {:?}, fingerprint {pkgs} ({files} files, {bytes} bytes, {hashed} hashed, {} ms)",
        path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
        prof.packages.len(),
        prof.natives,
        t0.elapsed().as_millis()
    ));
    Scan { pkgs, pkg_names: list(&names), natives: list(&prof.natives) }
}

/// At start (lib.rs): the scan on a thread of its own (big texture packs take a while).
pub fn start() {
    let spawned = std::thread::Builder::new().name("coop-me3scan".into()).spawn(|| {
        let r = std::panic::catch_unwind(scan_now).unwrap_or_else(|_| {
            log::line("doctor: me3 file mod scan panicked; pkgs=?");
            Scan { pkgs: "?".into(), pkg_names: "?".into(), natives: "?".into() }
        });
        let _ = RESULT.set(r);
    });
    if spawned.is_err() {
        log::line("doctor: me3 file mod scan: cannot start the thread; pkgs=?");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_shipped_profile_has_no_packages() {
        let text = "profileVersion = \"v1\"\nsavefile = \"S0000.coop.sl2\"\n\n[[supports]]\ngame = \"sekiro\"\n\n[[natives]]\npath = \"sekiro_coop.dll\"\noptional = false\nenabled = true\n";
        let p = parse_profile(text, "sekiro_coop.dll");
        assert!(p.loads_us);
        assert!(p.packages.is_empty() && p.natives.is_empty());
    }

    /// The profile in git never carries the parts package; tools\parts\stage-parts.ps1 appends
    /// it (CRLF, as the file is checked out) only at packaging time (FORK.md "Parts package").
    #[test]
    fn parts_package_only_in_the_staged_profile() {
        let shipped = include_str!("../../../ship/sekiro-coop.me3");
        let p = parse_profile(shipped, "sekiro_coop.dll");
        assert!(p.loads_us && p.packages.is_empty());
        let staged = format!(
            "{}\r\n\r\n[[packages]]\r\nid = \"sekiro-together-parts\"\r\npath = 'mod'\r\n",
            shipped.trim_end()
        );
        let p = parse_profile(&staged, "sekiro_coop.dll");
        assert!(p.loads_us);
        assert_eq!(p.packages, vec![("sekiro-together-parts".to_string(), "mod".to_string())]);
    }

    #[test]
    fn packages_and_other_natives_are_listed() {
        let text = "[[natives]]\npath = 'mods\\\\sekiro_coop.dll'\n\n[[packages]]\nid = \"ps5-glyphs\"\npath = \"ps5-glyphs/\"\n\n[[packages]]\npath = \"old/mod\" # off\nenabled = false\n\n[[packages]]\nsource = \"texture-pack\"\n\n[[natives]]\npath = \"other/fps.dll\"\n[[natives]]\npath = \"x.dll\"\nenabled = false\n";
        let p = parse_profile(text, "SEKIRO_COOP.dll");
        assert!(p.loads_us);
        assert_eq!(p.packages, vec![("ps5-glyphs".to_string(), "ps5-glyphs/".to_string()), ("texture-pack".to_string(), "texture-pack".to_string())]);
        assert_eq!(p.natives, vec!["fps.dll".to_string()]);
        // A profile for another mod does not count as ours.
        assert!(!parse_profile("[[natives]]\npath = \"other.dll\"\n", "sekiro_coop.dll").loads_us);
    }

    #[test]
    fn fingerprint_changes_with_a_file() {
        let dir = std::env::temp_dir().join(format!("coop-me3scan-test-{}", std::process::id()));
        let pkg = dir.join("pkg").join("menu");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("a.gfx"), b"one").unwrap();
        let prof = Profile { packages: vec![("pkg".into(), "pkg".into())], natives: vec![], loads_us: true };
        let (a, files, bytes, hashed) = fingerprint(&dir, &prof);
        assert!(a.starts_with("1:"));
        assert_eq!((files, bytes, hashed), (1, 3, 1));
        assert_eq!(fingerprint(&dir, &prof).0, a, "stable");
        fs::write(pkg.join("a.gfx"), b"two").unwrap();
        assert_ne!(fingerprint(&dir, &prof).0, a, "same size, other content");
        assert_eq!(fingerprint(&dir, &Profile::default()).0, "none");
        let _ = fs::remove_dir_all(&dir);
    }
}
