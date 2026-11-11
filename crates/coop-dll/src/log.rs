//! Minimal append-only file log; no dependencies so Phase 0 stays tiny.

use std::fs::{self, File, OpenOptions};
use std::fmt::Write as _;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// `%LOCALAPPDATA%\sekiro-coop`: log.txt, audit.txt, the per-run logs.
pub fn data_dir() -> PathBuf {
    // Unit tests log to temp: in the real log their lines read as live spectate/warp events.
    #[cfg(test)]
    let base = std::env::temp_dir();
    #[cfg(not(test))]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("sekiro-coop")
}

fn path() -> PathBuf {
    data_dir().join("log.txt")
}

/// `logs\`: one file per game run (`YYYY-MM-DD_HH-MM-SS.log`, local start time) and the zips of logzip.rs.
pub fn runs_dir() -> PathBuf {
    data_dir().join("logs")
}

/// Per-run logs kept in `logs\` (the oldest go first).
pub const KEEP_RUNS: usize = 20;

/// This run's own file in `logs\` (the same lines as log.txt). log.txt stays as it was
/// (append-only, moved to log.old.txt at 8 MB), so every tool and instruction reading it still works.
/// `log_per_run = 0` (settings, read straight from the file at start) = no such file.
static RUN_FILE: Mutex<Option<File>> = Mutex::new(None);
static RUN_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Proleptic Gregorian date of a day count since 1970-01-01 (Howard Hinnant's civil_from_days).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// `YYYY-MM-DD_HH-MM-SS` of a unix time in a zone `offset` seconds east of UTC (file names sort by time).
pub fn file_stamp(epoch: u64, offset: i64) -> String {
    let t = epoch as i64 + offset;
    let (y, m, d) = civil(t.div_euclid(86_400));
    let s = t.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02}_{:02}-{:02}-{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// `2026-11-09_02-00-31.log`: the per-run log name (nothing else in `logs\` is pruned as a run log).
pub fn is_run_log_name(name: &str) -> bool {
    let b = name.as_bytes();
    name.len() == 23
        && name.ends_with(".log")
        && b[..19].iter().enumerate().all(|(i, c)| match i {
            4 | 7 | 13 | 16 => *c == b'-',
            10 => *c == b'_',
            _ => c.is_ascii_digit(),
        })
}

/// The run logs to delete so that `keep` stay: the oldest by name (names sort by start time).
pub fn runs_to_delete(names: &[String], keep: usize) -> Vec<String> {
    let mut runs: Vec<&String> = names.iter().filter(|n| is_run_log_name(n)).collect();
    runs.sort();
    let extra = runs.len().saturating_sub(keep);
    runs[..extra].iter().map(|s| (*s).clone()).collect()
}

/// Run logs in `logs\`, oldest first.
pub fn run_logs() -> Vec<PathBuf> {
    let dir = runs_dir();
    let Ok(rd) = fs::read_dir(&dir) else { return Vec::new() };
    let mut names: Vec<String> = rd.flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|n| is_run_log_name(n)).collect();
    names.sort();
    names.into_iter().map(|n| dir.join(n)).collect()
}

/// Opens this run's file in `logs\` and prunes the oldest beyond `KEEP_RUNS` (once, at start).
fn open_run_file(secs: u64, offset: i64) -> Option<PathBuf> {
    let dir = runs_dir();
    fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("{}.log", file_stamp(secs, offset)));
    let f = OpenOptions::new().create(true).append(true).open(&path).ok()?;
    *RUN_FILE.lock().unwrap_or_else(|e| e.into_inner()) = Some(f);
    let _ = RUN_PATH.set(path.clone());
    let names: Vec<String> = fs::read_dir(&dir)
        .map(|rd| rd.flatten().filter_map(|e| e.file_name().into_string().ok()).collect())
        .unwrap_or_default();
    for old in runs_to_delete(&names, KEEP_RUNS) {
        let _ = fs::remove_file(dir.join(old));
    }
    Some(path)
}

/// Above this the log is moved to log.old.txt (checked once per game start),
/// so an append-only log cannot fill the disk over weeks of play.
const MAX_LOG_BYTES: u64 = 8 << 20;

/// The open log (v8.6): opened once, not per line. Open + close per line ran on the game
/// thread (create_dir_all, CreateFile, the virus scanner's look at every close) for each of
/// the ~7 lines a second. Unbuffered, so a crash loses nothing; std shares read / write /
/// delete, so the uploader can copy it while the game runs.
static FILE: Mutex<Option<File>> = Mutex::new(None);

fn open() -> Option<File> {
    let path = path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG_BYTES) {
        let _ = fs::rename(&path, path.with_file_name("log.old.txt"));
    }
    OpenOptions::new().create(true).append(true).open(&path).ok()
}

/// Epoch of the last `clock:` line (0 = none yet).
static LAST_CLOCK: AtomicU64 = AtomicU64::new(0);
/// A `clock:` line this often, so epoch values can be turned into wall-clock time anywhere in a long log.
const CLOCK_EVERY_SECS: u64 = 600;

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn write_raw(secs: u64, msg: &str) {
    // The whole line is built before the lock and goes out in ONE write: `writeln!` on an
    // unbuffered File issued a WriteFile per fragment (prefix, text, newline).
    let mut buf = String::with_capacity(msg.len() + 24);
    let _ = writeln!(buf, "[{secs}] {msg}");
    let mut g = FILE.lock().unwrap_or_else(|e| e.into_inner());
    if g.is_none() {
        *g = open();
    }
    if let Some(f) = g.as_mut()
        && f.write_all(buf.as_bytes()).is_err()
    {
        // Deleted or moved under us: open it again on the next line.
        *g = None;
    }
    // Under the log.txt lock, so both files get the lines in the same order.
    if let Some(f) = RUN_FILE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        let _ = f.write_all(buf.as_bytes());
    }
}

/// One line that never waits for the log lock (v8.52): at process exit (loader lock held,
/// the other threads already gone, one of them may have died holding the lock) and in the
/// unhandled-exception filter. Busy lock: the line goes through a handle of its own.
pub fn line_nowait(msg: &str) {
    let mut buf = String::with_capacity(msg.len() + 24);
    let _ = writeln!(buf, "[{}] {msg}", now_secs());
    if let Ok(mut g) = RUN_FILE.try_lock() {
        if let Some(f) = g.as_mut() {
            let _ = f.write_all(buf.as_bytes());
        }
    } else if let Some(p) = RUN_PATH.get()
        && let Ok(mut f) = OpenOptions::new().append(true).open(p)
    {
        let _ = f.write_all(buf.as_bytes());
    }
    if let Ok(mut g) = FILE.try_lock()
        && let Some(f) = g.as_mut()
    {
        let _ = f.write_all(buf.as_bytes());
        return;
    }
    if let Ok(mut f) = OpenOptions::new().append(true).open(path()) {
        let _ = f.write_all(buf.as_bytes());
    }
}

pub fn line(msg: &str) {
    if muted(msg) {
        return;
    }
    let secs = now_secs();
    // Wall-clock reference first: before every F11 mark and every 10 minutes.
    if msg.starts_with("MARK F11") || secs.saturating_sub(LAST_CLOCK.load(Ordering::Relaxed)) >= CLOCK_EVERY_SECS {
        clock_line(secs);
    }
    write_raw(secs, msg);
}

/// `EVENT <kind> <detail>`: the compact timeline for offline analysis. Never rate limited.
/// Planned kinds (other modules adopt it later): BOSS_START, BOSS_END, DEATH, WIPE, REST, LOAD, RELEASE.
/// Already used: SESSION_START.
pub fn event(kind: &str, detail: &str) {
    let secs = now_secs();
    if detail.is_empty() {
        write_raw(secs, &format!("EVENT {kind}"));
    } else {
        write_raw(secs, &format!("EVENT {kind} {detail}"));
    }
}

/// Seconds east of UTC of the local time zone now (0 if it cannot be read).
#[cfg(windows)]
pub fn local_utc_offset_secs() -> i64 {
    #[repr(C)]
    struct TimeZoneInformation {
        bias: i32,
        standard_name: [u16; 32],
        standard_date: [u16; 8],
        standard_bias: i32,
        daylight_name: [u16; 32],
        daylight_date: [u16; 8],
        daylight_bias: i32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetTimeZoneInformation(tz: *mut TimeZoneInformation) -> u32;
    }
    // SAFETY: plain struct of integers, filled by the call.
    let mut tz: TimeZoneInformation = unsafe { std::mem::zeroed() };
    let id = unsafe { GetTimeZoneInformation(&mut tz) };
    if id == 0xFFFF_FFFF {
        return 0;
    }
    // id 2 = daylight saving in effect. Windows bias is minutes WEST of UTC.
    let extra = if id == 2 { tz.daylight_bias } else { tz.standard_bias };
    -(i64::from(tz.bias) + i64::from(extra)) * 60
}

#[cfg(not(windows))]
pub fn local_utc_offset_secs() -> i64 {
    0
}

/// `HH:MM:SS` of a unix time in a zone `offset` seconds east of UTC.
pub fn hms(epoch: u64, offset: i64) -> String {
    let day = (epoch as i64 + offset).rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", day / 3600, day / 60 % 60, day % 60)
}

/// `+hh:mm` / `-hh:mm`.
pub fn offset_label(offset: i64) -> String {
    let sign = if offset < 0 { '-' } else { '+' };
    let m = offset.abs() / 60;
    format!("{sign}{:02}:{:02}", m / 60, m % 60)
}

/// Local wall-clock time now, `HH:MM:SS`.
pub fn local_hms() -> String {
    hms(now_secs(), local_utc_offset_secs())
}

fn clock_line(secs: u64) {
    LAST_CLOCK.store(secs, Ordering::Relaxed);
    let off = local_utc_offset_secs();
    write_raw(secs, &format!("clock: epoch {secs} = local {} (UTC offset {})", hms(secs, off), offset_label(off)));
    muted_summary(secs);
}

/// How the previous run ended: read from the tail of log.txt before this run writes anything.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PrevEnd {
    /// No earlier log to read.
    NoLog,
    /// An `EVENT SESSION_END ...` line: the process ended through our code.
    Clean,
    /// `EVENT SESSION_END crash`: the unhandled-exception filter ran.
    Crash,
    /// A session start with no SESSION_END after it: killed (Task Manager) or a crash that ran no code.
    NoExitLine,
}

/// What the tail of the log was when this run started (`session_header`, once).
static PREV_END: OnceLock<PrevEnd> = OnceLock::new();

/// How the previous run ended (`NoLog` until the session header has read the log).
pub fn previous_end() -> PrevEnd {
    *PREV_END.get().unwrap_or(&PrevEnd::NoLog)
}

/// The last 64 KB of the log as text (empty when there is no log).
fn read_tail() -> String {
    read_tail_of(&path())
}

fn read_tail_of(p: &std::path::Path) -> String {
    const TAIL_BYTES: u64 = 64 << 10;
    let Ok(mut f) = File::open(p) else {
        return String::new();
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    // A cut in the middle of a UTF-8 sequence costs one replacement character in the lossy read.
    if len > TAIL_BYTES && f.seek(SeekFrom::Start(len - TAIL_BYTES)).is_err() {
        return String::new();
    }
    let mut buf = Vec::new();
    if f.take(TAIL_BYTES).read_to_end(&mut buf).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// The last non-empty line of a log tail.
fn last_line(tail: &str) -> &str {
    tail.lines().rev().map(str::trim_end).find(|l| !l.is_empty()).unwrap_or("")
}

/// The newest run log is the previous run's own file only when it ends with the same line as
/// log.txt (a run with `log_per_run = 0` wrote log.txt alone, and an older run file is not it).
fn same_run(log_tail: &str, run_tail: &str) -> bool {
    let a = last_line(log_tail);
    !a.is_empty() && a == last_line(run_tail)
}

/// Classifies the tail of the previous log: only the last session counts (an older SESSION_END
/// belongs to a run that ended days ago).
fn classify_previous(tail: &str) -> PrevEnd {
    const START: &str = "==== sekiro-coop session start ====";
    if tail.trim().is_empty() {
        return PrevEnd::NoLog;
    }
    let run = tail.rfind(START).map_or(tail, |i| &tail[i..]);
    if run.contains("EVENT SESSION_END crash") {
        PrevEnd::Crash
    } else if run.contains("EVENT SESSION_END") {
        PrevEnd::Clean
    } else {
        PrevEnd::NoExitLine
    }
}

/// First lines of a run: mod version, build id, wall clock, how the previous run ended. Called once at DLL start.
pub fn session_header() {
    let secs = now_secs();
    let built: u64 = env!("COOP_BUILD_EPOCH").parse().unwrap_or(0);
    let off = local_utc_offset_secs();
    // Before the first write: it opens the file and may move it to log.old.txt.
    let tail = read_tail();
    let prev = classify_previous(&tail);
    let _ = PREV_END.set(prev);
    // The previous run's own file, found before this run makes the newest one.
    let prev_run = run_logs().pop().filter(|p| same_run(&tail, &read_tail_of(p)));
    let per_run = crate::settings::early_flag("log_per_run", true);
    let run = if per_run { open_run_file(secs, off) } else { None };
    write_raw(secs, "==== sekiro-coop session start ====");
    write_raw(
        secs,
        &format!(
            "version: sekiro_coop v{} build {} (built epoch {built} = local {})",
            crate::ui::MOD_VERSION,
            env!("COOP_GIT_HASH"),
            hms(built, off)
        ),
    );
    clock_line(secs);
    event("SESSION_START", &format!("build={}", env!("COOP_GIT_HASH")));
    let ended = match prev {
        PrevEnd::Clean => "clean exit",
        PrevEnd::Crash => "crashed",
        PrevEnd::NoExitLine => "no exit line (closed from Task Manager or a crash that ran no code)",
        PrevEnd::NoLog => "no earlier log",
    };
    write_raw(secs, &format!("log: previous run: {ended}"));
    match (&run, per_run) {
        (Some(p), _) => write_raw(secs, &format!("log: this run also goes to {} (log_per_run, the last {KEEP_RUNS} runs are kept)", p.display())),
        (None, true) => write_raw(secs, "log: could not open a per-run file in logs\\, log.txt only"),
        (None, false) => write_raw(secs, "log: per-run files off (log_per_run = 0), log.txt only"),
    }
    if matches!(prev, PrevEnd::Crash | PrevEnd::NoExitLine) {
        crate::logzip::pack_after_crash(prev, prev_run);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_stamps_sort_by_time() {
        // 2026-11-09 02:00:31 UTC.
        assert_eq!(file_stamp(1_794_189_631, 0), "2026-11-09_02-00-31");
        assert_eq!(file_stamp(1_794_189_631, 3 * 3600), "2026-11-09_05-00-31");
        // Back across midnight and a month end.
        assert_eq!(file_stamp(1_793_491_200, -3600), "2026-10-31_23-00-00");
        assert_eq!(file_stamp(0, 0), "1970-01-01_00-00-00");
        assert_eq!(file_stamp(1_709_164_800, 0), "2024-02-29_00-00-00");
        assert!(is_run_log_name(&format!("{}.log", file_stamp(1_794_189_631, 0))));
    }

    #[test]
    fn previous_run_file_must_end_like_log_txt() {
        let log = "[1] ==== sekiro-coop session start ====\n[5] world: x\n[9] coop: last line\r\n\n";
        assert!(same_run(log, "[5] world: x\n[9] coop: last line\n"));
        assert!(!same_run(log, "[3] coop: an older run\n"));
        assert!(!same_run("", ""));
    }

    #[test]
    fn run_log_names_and_pruning() {
        assert!(is_run_log_name("2026-11-09_02-00-31.log"));
        assert!(!is_run_log_name("2026-11-09_02-00-31.zip"));
        assert!(!is_run_log_name("crash_2026-11-09_02-00-31.zip"));
        assert!(!is_run_log_name("log.txt"));
        assert!(!is_run_log_name("2026-11-09 02-00-31.log"));
        let mut names: Vec<String> = (0..25).rev().map(|i| format!("2026-11-{:02}_10-00-00.log", i + 1)).collect();
        names.push("crash_2026-11-01_09-00-00.zip".into());
        names.push("notes.txt".into());
        let gone = runs_to_delete(&names, 20);
        assert_eq!(gone, (1..=5).map(|d| format!("2026-11-{d:02}_10-00-00.log")).collect::<Vec<_>>());
        assert!(runs_to_delete(&names[..3], 20).is_empty());
    }

    #[test]
    fn hms_formats_with_offset() {
        let midnight = 1_793_059_200 - 1_793_059_200 % 86_400;
        assert_eq!(hms(midnight + 86_399 - 20, 0), "23:59:39");
        assert_eq!(hms(midnight, 7200), "02:00:00");
        // Crosses midnight backwards.
        assert_eq!(hms(midnight + 60, -3600), "23:01:00");
        assert_eq!(hms(0, 0), "00:00:00");
    }

    #[test]
    fn offset_labels() {
        assert_eq!(offset_label(0), "+00:00");
        assert_eq!(offset_label(7200), "+02:00");
        assert_eq!(offset_label(-12_600), "-03:30");
        assert_eq!(offset_label(19_800), "+05:30");
    }

    #[test]
    fn local_offset_is_sane() {
        let o = local_utc_offset_secs();
        assert!((-14 * 3600..=14 * 3600).contains(&o));
    }

    #[test]
    fn previous_run_without_a_log() {
        assert_eq!(classify_previous(""), PrevEnd::NoLog);
        assert_eq!(classify_previous("  \r\n\t "), PrevEnd::NoLog);
    }

    #[test]
    fn previous_run_that_closed_normally() {
        let tail = "[1] ==== sekiro-coop session start ====\n[1] version: sekiro_coop v1.0.7\n[9] EVENT SESSION_END exit: the game process is exiting normally (ExitProcess)\n";
        assert_eq!(classify_previous(tail), PrevEnd::Clean);
    }

    #[test]
    fn previous_run_that_crashed() {
        let tail = "[1] ==== sekiro-coop session start ====\n[9] EVENT SESSION_END crash: access violation at 0x7ff6\n";
        assert_eq!(classify_previous(tail), PrevEnd::Crash);
    }

    #[test]
    fn previous_run_that_was_killed() {
        assert_eq!(classify_previous("[1] ==== sekiro-coop session start ====\n[9] coop: dll attached\n"), PrevEnd::NoExitLine);
    }

    #[test]
    fn only_the_last_session_counts() {
        let tail = "[1] ==== sekiro-coop session start ====\n[9] EVENT SESSION_END exit: the game process is exiting normally (ExitProcess)\n[1] ==== sekiro-coop session start ====\n[9] coop: dll attached\n";
        assert_eq!(classify_previous(tail), PrevEnd::NoExitLine);
    }

    #[test]
    fn tail_without_a_session_start() {
        assert_eq!(classify_previous("[9] EVENT SESSION_END exit: the game process is exiting normally (ExitProcess)\n"), PrevEnd::Clean);
    }
}

/// Log categories switched off (`log_off` in the settings, v8.53): the first word of a line up to
/// `:`, `[`, `(` or a space, lower case ("gpuguard", "native", "t" for the T position lines).
/// EVENT lines and the `clock:` / session header lines never pass through here.
static MUTED: Mutex<Vec<String>> = Mutex::new(Vec::new());
static ANY_MUTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Lines dropped per category since the last `clock:` line.
static DROPPED: Mutex<Vec<(String, u64)>> = Mutex::new(Vec::new());

/// The category of a line: its first word, lower case.
pub fn category(msg: &str) -> String {
    let end = msg.find([':', '[', '(', ' ']).unwrap_or(msg.len());
    msg[..end].to_ascii_lowercase()
}

/// Parses a `log_off` value ("observe, hkb, native: remote moved") into entries: a bare word is a
/// category, an entry with a `:` or a space is a line prefix (one line type of a category).
pub fn parse_categories(value: &str) -> Vec<String> {
    value.split([',', ';']).map(|s| s.trim().to_ascii_lowercase()).filter(|s| !s.is_empty()).collect()
}

/// The muted entry a line falls under, if any (see `parse_categories`).
fn muted_entry(entries: &[String], msg: &str) -> Option<String> {
    let cat = category(msg);
    entries.iter().find(|e| **e == cat || (e.contains([':', ' ']) && msg.len() >= e.len() && msg.as_bytes()[..e.len()].eq_ignore_ascii_case(e.as_bytes()))).cloned()
}

/// Sets the muted categories (settings.rs, every reload). A change is logged once.
pub fn set_muted(cats: Vec<String>) {
    let mut g = MUTED.lock().unwrap_or_else(|e| e.into_inner());
    if *g == cats {
        return;
    }
    ANY_MUTED.store(!cats.is_empty(), Ordering::Relaxed);
    *g = cats;
    let list = if g.is_empty() { "none".to_string() } else { g.join(", ") };
    drop(g);
    write_raw(now_secs(), &format!("log: categories off: {list} (settings log_off; counts in the clock lines)"));
}

fn muted(msg: &str) -> bool {
    if !ANY_MUTED.load(Ordering::Relaxed) || msg.starts_with("MARK") || msg.starts_with("CRASH") {
        return false;
    }
    let Some(cat) = muted_entry(&MUTED.lock().unwrap_or_else(|e| e.into_inner()), msg) else {
        return false;
    };
    let mut d = DROPPED.lock().unwrap_or_else(|e| e.into_inner());
    match d.iter_mut().find(|(c, _)| *c == cat) {
        Some((_, n)) => *n += 1,
        None => d.push((cat, 1)),
    }
    true
}

/// After each `clock:` line: how many lines each muted category dropped since the last one.
fn muted_summary(secs: u64) {
    let d = std::mem::take(&mut *DROPPED.lock().unwrap_or_else(|e| e.into_inner()));
    if d.is_empty() {
        return;
    }
    let parts: Vec<String> = d.iter().map(|(c, n)| format!("{c} {n}")).collect();
    write_raw(secs, &format!("log: muted lines since the last clock line: {}", parts.join(", ")));
}

#[cfg(test)]
mod category_tests {
    use super::*;

    #[test]
    fn categories_are_the_first_word() {
        assert_eq!(category("gpuguard: 0 s: 804 material uploads"), "gpuguard");
        assert_eq!(category("native[after 3 s]: Net chr"), "native");
        assert_eq!(category("T client ms=1 P=1,2,3"), "t");
        assert_eq!(category("netai send 4"), "netai");
        assert_eq!(category("ownfill (engagement): 3"), "ownfill");
        assert_eq!(parse_categories(" Observe, hkb ;t, native: remote moved"), vec!["observe", "hkb", "t", "native: remote moved"]);
        let e = parse_categories("hkb, native: remote moved");
        assert_eq!(muted_entry(&e, "native: remote moved 1.6 m off our pin").as_deref(), Some("native: remote moved"));
        assert_eq!(muted_entry(&e, "native: pin (10 s): 0 snaps"), None, "other native lines stay");
        assert_eq!(muted_entry(&e, "hkb: sent 1469 packets").as_deref(), Some("hkb"));
        assert_eq!(muted_entry(&e, "n"), None);
        assert!(parse_categories("").is_empty());
    }
}
