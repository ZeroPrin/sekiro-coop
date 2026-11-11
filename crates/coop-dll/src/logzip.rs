//! Logs packed for a bug report (1.0.11 `send_logs_key` / `logzip:`, ported): one store-only
//! zip in `%LOCALAPPDATA%\sekiro-coop\logs\` with the newest per-run logs, audit.txt, the
//! friend's streamed log and the settings file (password hidden).
//!
//! - `send_logs_key` (default 118 = F7, 0 = off): `bugreport_<local time>.zip`, the path goes on screen.
//! - After a run that crashed or ended with no exit line (log.rs `session_header`):
//!   `crash_<local time>.zip` with that run's log, so the next 20 runs cannot push it out.
//!
//! No dependencies: the zip is written by hand (method 0 = stored, CRC-32, zip32).

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::log;

/// `send_logs_key`: Windows virtual-key code, 0 = off.
pub static SEND_LOGS_KEY: AtomicU32 = AtomicU32::new(118);
/// Zips kept in `logs\` (bug reports and crash bundles together, oldest go first).
const KEEP_ZIPS: usize = 10;
/// Newest per-run logs in a bug report (this run and the ones before it).
const REPORT_RUNS: usize = 3;

static BUSY: AtomicBool = AtomicBool::new(false);
static KEY_DOWN: AtomicBool = AtomicBool::new(false);

// ---- zip writer ---------------------------------------------------------------------------

const fn crc_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}

static CRC: [u32; 256] = crc_table();

/// CRC-32 (IEEE, as zip uses it).
pub fn crc32(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in data {
        c = CRC[((c ^ u32::from(b)) & 0xFF) as usize] ^ (c >> 8);
    }
    !c
}

/// MS-DOS time and date of a local wall-clock time `YYYY-MM-DD_HH-MM-SS` split into numbers.
fn dos_time(y: u32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> (u16, u16) {
    let time = (h << 11) | (mi << 5) | (s / 2);
    let date = (y.saturating_sub(1980) << 9) | (mo << 5) | d;
    (time as u16, date as u16)
}

/// Store-only zip of `(name, bytes)` entries. Names are written as UTF-8 (flag bit 11).
pub fn write_zip(out: &mut impl Write, files: &[(String, Vec<u8>)], (time, date): (u16, u16)) -> io::Result<()> {
    let mut central = Vec::new();
    let mut offset: u32 = 0;
    for (name, data) in files {
        let crc = crc32(data);
        let len = u32::try_from(data.len()).map_err(|_| io::Error::other("file over 4 GB"))?;
        let n = name.as_bytes();
        let mut h = Vec::with_capacity(30 + n.len());
        h.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        h.extend_from_slice(&20u16.to_le_bytes()); // version needed
        h.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 names
        h.extend_from_slice(&0u16.to_le_bytes()); // stored
        h.extend_from_slice(&time.to_le_bytes());
        h.extend_from_slice(&date.to_le_bytes());
        h.extend_from_slice(&crc.to_le_bytes());
        h.extend_from_slice(&len.to_le_bytes());
        h.extend_from_slice(&len.to_le_bytes());
        h.extend_from_slice(&(n.len() as u16).to_le_bytes());
        h.extend_from_slice(&0u16.to_le_bytes());
        h.extend_from_slice(n);
        out.write_all(&h)?;
        out.write_all(data)?;

        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // made by
        central.extend_from_slice(&20u16.to_le_bytes()); // needed
        central.extend_from_slice(&0x0800u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&time.to_le_bytes());
        central.extend_from_slice(&date.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&len.to_le_bytes());
        central.extend_from_slice(&len.to_le_bytes());
        central.extend_from_slice(&(n.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // extra
        central.extend_from_slice(&0u16.to_le_bytes()); // comment
        central.extend_from_slice(&0u16.to_le_bytes()); // disk
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        central.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(n);
        offset = offset
            .checked_add((h.len() + data.len()) as u32)
            .ok_or_else(|| io::Error::other("zip over 4 GB"))?;
    }
    let count = files.len() as u16;
    let mut end = Vec::with_capacity(22);
    end.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    end.extend_from_slice(&[0u8; 4]); // disk numbers
    end.extend_from_slice(&count.to_le_bytes());
    end.extend_from_slice(&count.to_le_bytes());
    end.extend_from_slice(&(central.len() as u32).to_le_bytes());
    end.extend_from_slice(&offset.to_le_bytes());
    end.extend_from_slice(&0u16.to_le_bytes());
    out.write_all(&central)?;
    out.write_all(&end)
}

// ---- what goes in -------------------------------------------------------------------------

/// The settings file with the password value blanked (the zip may be posted in public).
pub fn redact_ini(text: &str) -> String {
    text.split_inclusive('\n')
        .map(|line| {
            let t = line.trim_start();
            let key = t.split('=').next().unwrap_or("").trim().to_ascii_lowercase();
            if !t.starts_with(';') && t.contains('=') && key == "password" {
                let eol = if line.ends_with("\r\n") { "\r\n" } else if line.ends_with('\n') { "\n" } else { "" };
                format!("password = <hidden>{eol}")
            } else {
                line.to_string()
            }
        })
        .collect()
}

/// The zips to delete so that `keep` stay (bug reports and crash bundles, oldest by time stamp).
pub fn zips_to_delete(names: &[String], keep: usize) -> Vec<String> {
    let stamp = |n: &String| n.split_once('_').map(|(_, s)| s.to_string()).unwrap_or_default();
    let mut zips: Vec<&String> =
        names.iter().filter(|n| (n.starts_with("bugreport_") || n.starts_with("crash_")) && n.ends_with(".zip")).collect();
    zips.sort_by_key(|n| stamp(n));
    let extra = zips.len().saturating_sub(keep);
    zips[..extra].iter().map(|s| (*s).clone()).collect()
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Adds a file under `name` if it exists (a missing one is just left out).
fn add(files: &mut Vec<(String, Vec<u8>)>, name: &str, path: &Path) {
    if let Ok(data) = fs::read(path) {
        files.push((name.to_string(), data));
    }
}

/// Everything except the logs: audit.txt, the friend's streamed logs, the settings file.
fn add_common(files: &mut Vec<(String, Vec<u8>)>) {
    let dir = log::data_dir();
    add(files, "audit.txt", &dir.join("audit.txt"));
    if let Ok(rd) = fs::read_dir(&dir) {
        let mut friend: Vec<String> = rd
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.starts_with("friend-log") && n.ends_with(".txt"))
            .collect();
        friend.sort();
        for n in friend {
            add(files, &n, &dir.join(&n));
        }
    }
    if let Ok(text) = fs::read_to_string(crate::settings::path()) {
        files.push(("sekiro_coop_settings.ini".to_string(), redact_ini(&text).into_bytes()));
    }
}

/// Writes the zip and prunes old ones. Returns (path, files, bytes).
fn write_bundle(kind: &str, stamp: &str, files: &[(String, Vec<u8>)]) -> io::Result<(PathBuf, usize, usize)> {
    let dir = log::runs_dir();
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{kind}_{stamp}.zip"));
    let num = |a: usize, b: usize| stamp.get(a..b).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let dos = dos_time(num(0, 4), num(5, 7), num(8, 10), num(11, 13), num(14, 16), num(17, 19));
    let mut buf = Vec::new();
    write_zip(&mut buf, files, dos)?;
    // Written under a temp name first: a half-written zip never looks like a finished one.
    let tmp = path.with_extension("zip.part");
    fs::write(&tmp, &buf)?;
    fs::rename(&tmp, &path)?;
    let names: Vec<String> = fs::read_dir(&dir)
        .map(|rd| rd.flatten().filter_map(|e| e.file_name().into_string().ok()).collect())
        .unwrap_or_default();
    for old in zips_to_delete(&names, KEEP_ZIPS) {
        let _ = fs::remove_file(dir.join(old));
    }
    Ok((path, files.len(), buf.len()))
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The previous run crashed or left no exit line (log.rs `session_header`, before `settings::init`):
/// pack its log on a thread of its own. `prev_run` = its per-run file (None before the first
/// per-run build: then log.txt + log.old.txt, which hold it).
pub fn pack_after_crash(prev: log::PrevEnd, prev_run: Option<PathBuf>) {
    let stamp = log::file_stamp(now_secs(), log::local_utc_offset_secs());
    std::thread::spawn(move || {
        let mut files = Vec::new();
        match &prev_run {
            Some(p) => add(&mut files, &file_name(p), p),
            None => {
                let dir = log::data_dir();
                add(&mut files, "log.txt", &dir.join("log.txt"));
                add(&mut files, "log.old.txt", &dir.join("log.old.txt"));
            }
        }
        add_common(&mut files);
        let why = if prev == log::PrevEnd::Crash { "crashed" } else { "had no exit line" };
        match write_bundle("crash", &stamp, &files) {
            Ok((path, n, bytes)) => log::line(&format!(
                "logzip: the previous run {why}: wrote {n} files ({} KB) to {} (its log: {})",
                bytes / 1024,
                path.display(),
                prev_run.as_deref().map_or_else(|| "log.txt + log.old.txt".to_string(), file_name)
            )),
            Err(e) => log::line(&format!("logzip: the previous run {why}: packing failed: {e}")),
        }
    });
}

/// `send_logs_key` pressed: the newest per-run logs (this run first) + the common files.
fn pack_bug_report() {
    if BUSY.swap(true, Ordering::AcqRel) {
        return;
    }
    let stamp = log::file_stamp(now_secs(), log::local_utc_offset_secs());
    log::line("logzip: packing the logs for a bug report");
    std::thread::spawn(move || {
        let mut files = Vec::new();
        let runs = log::run_logs();
        if runs.is_empty() {
            let dir = log::data_dir();
            add(&mut files, "log.txt", &dir.join("log.txt"));
            add(&mut files, "log.old.txt", &dir.join("log.old.txt"));
        } else {
            for p in runs.iter().rev().take(REPORT_RUNS) {
                add(&mut files, &file_name(p), p);
            }
        }
        add_common(&mut files);
        match write_bundle("bugreport", &stamp, &files) {
            Ok((path, n, bytes)) => {
                log::line(&format!("logzip: wrote {n} files ({} KB) to {}", bytes / 1024, path.display()));
                crate::bossrules::notice(
                    format!("Logs packed for a bug report:\n{}\\logs\\{}", crate::platform::data_folder_hint(), file_name(&path)),
                    15,
                );
            }
            Err(e) => {
                log::line(&format!("logzip: packing failed: {e}"));
                crate::bossrules::notice(format!("Packing the logs failed: {e}"), 10);
            }
        }
        BUSY.store(false, Ordering::Release);
    });
}

#[link(name = "user32")]
unsafe extern "system" {
    fn GetAsyncKeyState(vk: i32) -> i16;
}

/// Every frame (puppet.rs): the send-logs key, only while the game window has the focus.
pub fn tick() {
    let vk = SEND_LOGS_KEY.load(Ordering::Relaxed);
    if vk == 0 || vk > 255 {
        return;
    }
    // SAFETY: plain Win32 query.
    let down = unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000 != 0 && crate::spectate_input::game_focused();
    if down && !KEY_DOWN.swap(true, Ordering::Relaxed) {
        pack_bug_report();
    } else if !down {
        KEY_DOWN.store(false, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16le(b: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([b[at], b[at + 1]])
    }
    fn u32le(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
    }

    #[test]
    fn crc32_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn zip_layout_reads_back() {
        let files = vec![("a.txt".to_string(), b"hello".to_vec()), ("logs/b.log".to_string(), vec![0u8; 1000])];
        let mut z = Vec::new();
        write_zip(&mut z, &files, dos_time(2026, 11, 9, 2, 0, 31)).unwrap();
        // End of central directory.
        let e = z.len() - 22;
        assert_eq!(u32le(&z, e), 0x0605_4b50);
        assert_eq!(u16le(&z, e + 10), 2);
        let cd_size = u32le(&z, e + 12) as usize;
        let cd_off = u32le(&z, e + 16) as usize;
        assert_eq!(cd_off + cd_size, e);
        // Walk the central directory and check each local header and its data.
        let mut p = cd_off;
        for (name, data) in &files {
            assert_eq!(u32le(&z, p), 0x0201_4b50);
            assert_eq!(u16le(&z, p + 10), 0, "stored");
            assert_eq!(u32le(&z, p + 16), crc32(data));
            assert_eq!(u32le(&z, p + 20) as usize, data.len());
            let nlen = u16le(&z, p + 28) as usize;
            assert_eq!(&z[p + 46..p + 46 + nlen], name.as_bytes());
            let local = u32le(&z, p + 42) as usize;
            assert_eq!(u32le(&z, local), 0x0403_4b50);
            assert_eq!(u32le(&z, local + 14), crc32(data));
            let start = local + 30 + u16le(&z, local + 26) as usize;
            assert_eq!(&z[start..start + data.len()], &data[..]);
            p += 46 + nlen;
        }
        assert_eq!(p, e);
    }

    #[test]
    fn dos_time_fields() {
        let (t, d) = dos_time(2026, 11, 9, 2, 0, 31);
        assert_eq!(t >> 11, 2);
        assert_eq!((t >> 5) & 0x3F, 0);
        assert_eq!(t & 0x1F, 15);
        assert_eq!(d >> 9, 46);
        assert_eq!((d >> 5) & 0xF, 11);
        assert_eq!(d & 0x1F, 9);
    }

    #[test]
    fn password_is_hidden() {
        let ini = "; password = in a comment stays\r\npassword = hunter2\r\nplayer_name = Wolf\n  Password=x\n";
        let r = redact_ini(ini);
        assert_eq!(r, "; password = in a comment stays\r\npassword = <hidden>\r\nplayer_name = Wolf\npassword = <hidden>\n");
        assert!(!r.contains("hunter2"));
    }

    #[test]
    fn old_zips_are_pruned_by_time() {
        let mut names: Vec<String> = (1..=8).map(|d| format!("bugreport_2026-11-{d:02}_10-00-00.zip")).collect();
        names.extend((1..=4).map(|d| format!("crash_2026-11-{d:02}_09-00-00.zip")));
        names.push("2026-11-01_08-00-00.log".into());
        let gone = zips_to_delete(&names, 10);
        assert_eq!(gone, vec!["crash_2026-11-01_09-00-00.zip".to_string(), "bugreport_2026-11-01_10-00-00.zip".to_string()]);
    }
}
