//! Player packet rate (FORK.md "Player packet rate (experimental)", `[ADVANCED]
//! player_packet_hz`).
//!
//! The game sends our own player frame (game packet type 1: position, rotation, HP, sync
//! states, ~76 B) from PadManipulator slot 11 `0x7EEB50` [VAR, Ghidra on the 2026-11-11 dump]:
//! - `pad+0x360` is a countdown, decremented once per call (once per frame) and sent when it
//!   drops below 1 or the force byte `chr+0x2201` is set;
//! - after each send it is re-armed with `0x96F8A0(5)` (`mov ecx, 5` at `0x7EEF47`, `call` at
//!   `0x7EEF4C`). `0x96F8A0` looks up the debug config key `ChrSync.FrameSkipCount` (UTF-16 at
//!   `0x2A110C0`) with that default and returns it; in retail it returns 5 (live: 12.0/s);
//! - that `call` is the only caller of `0x96F8A0` (call/jmp byte scan of the image), and no
//!   other packet type goes through it.
//!
//! So the game's rate is 5 frames: 12/s at 60 fps, 24/s at 120 fps. With `player_packet_hz`
//! above 12, `0x96F8A0` is redirected to `frame_skip_count`, which returns the frame count that
//! gives that rate at the measured frame time (never more frames than the game's own count).
//! Only this machine's sending changes. The receiver's NetworkManipulator measures the time
//! between packets itself (`0x7EC9E0` divides by `+0x3CC`, the time since the previous packet,
//! reset per packet in `0x7EC250`); the mod's own play-out and position pin adapt to the
//! measured cadence (native.rs `next_packet`, `pin_delay_ms`).

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};
use std::time::{Duration, Instant};

use crate::{hook, log, tuning};

/// The game's own rate at 60 fps: `player_packet_hz` at or below this installs nothing.
pub const VANILLA_HZ: u32 = 12;
pub const DEFAULT_HZ: u32 = 30;
pub const MAX_HZ: u32 = 60;

/// `(u32 default) -> u32`: `ChrSync.FrameSkipCount` lookup, only caller `0x7EEF4C`.
const SKIP_COUNT_RVA: usize = 0x96F8A0;
/// `push rdi; sub rsp, 0x70; mov qword [rsp+0x28], -2`.
const SKIP_COUNT_PROLOGUE: [u8; 15] = [0x40, 0x57, 0x48, 0x83, 0xEC, 0x70, 0x48, 0xC7, 0x44, 0x24, 0x28, 0xFE, 0xFF, 0xFF, 0xFF];
/// `mov ecx, 5; call 0x96F8A0` in `0x7EEB50`.
const CALL_SITE_RVA: usize = 0x7EEF47;
const CALL_SITE: [u8; 10] = [0xB9, 0x05, 0x00, 0x00, 0x00, 0xE8, 0x4F, 0x09, 0x18, 0x00];
/// The game's frame count (the immediate above).
const GAME_FRAMES: u32 = 5;

/// Smoothed frame time in microseconds (FrameBegin to FrameBegin).
static FRAME_US: AtomicU32 = AtomicU32::new(16_667);
static LAST_FRAME: Mutex<Option<Instant>> = Mutex::new(None);
static INSTALLED: AtomicBool = AtomicBool::new(false);
/// What the last log line said: `hz << 8 | frames` (0 = nothing logged yet).
static LOGGED: AtomicU32 = AtomicU32::new(0);
static LOGGED_AT: Mutex<Option<Instant>> = Mutex::new(None);

/// Frames between two type-1 sends for `hz` at `frame_ms` per frame. `game` = the frame count
/// the game asked for (5): returned as is at or below 12 Hz, and never exceeded (a setting
/// below the game's own rate at a high frame rate does not slow the sending down).
pub fn skip_frames(hz: u32, frame_ms: f32, game: u32) -> u32 {
    if hz <= VANILLA_HZ || game == 0 {
        return game;
    }
    let frame_ms = if frame_ms.is_finite() { frame_ms.clamp(4.0, 100.0) } else { 1000.0 / 60.0 };
    let n = (1000.0 / hz.min(MAX_HZ) as f32 / frame_ms).round() as u32;
    n.clamp(1, game)
}

/// The smoothed frame time in ms.
pub fn frame_ms() -> f32 {
    FRAME_US.load(Relaxed) as f32 / 1000.0
}

/// Our replacement of `0x96F8A0`, called by the game (chr update) after each type-1 send.
extern "C" fn frame_skip_count(default: u32) -> u32 {
    skip_frames(tuning::player_packet_hz(), frame_ms(), default)
}

/// Start-up, before any player exists (lib.rs, with the other hooks).
pub fn install() {
    let hz = tuning::player_packet_hz();
    if hz <= VANILLA_HZ {
        log::line("native: player packet rate 12 Hz (the game's own, player_packet_hz = 12): nothing patched");
        return;
    }
    let site = crate::game::module_base() + CALL_SITE_RVA;
    let found: Vec<u8> = (0..CALL_SITE.len()).filter_map(|i| crate::game::read::<u8>(site + i)).collect();
    if found != CALL_SITE {
        log::line(&format!(
            "native: player packet rate NOT changed: the send site {site:#x} has bytes {found:02x?}, expected {CALL_SITE:02x?}"
        ));
        return;
    }
    if !hook::redirect_function(SKIP_COUNT_RVA, &SKIP_COUNT_PROLOGUE, frame_skip_count as *const () as usize, "player packet rate (0x96F8A0)") {
        log::line("native: player packet rate NOT changed (the frame-skip lookup did not match)");
        return;
    }
    INSTALLED.store(true, Relaxed);
    let n = skip_frames(hz, frame_ms(), GAME_FRAMES);
    LOGGED.store(hz << 8 | n, Relaxed);
    log::line(&format!(
        "native: player packet rate 12 -> {hz} Hz (interval {:.0} ms; every {n} frames instead of {GAME_FRAMES}, recomputed from the frame time)",
        1000.0 / hz as f32
    ));
}

/// Once per FrameBegin (native.rs `begin_frame`): the frame time, and a line when the frame
/// count changes (a live settings change, or the frame rate moved), at most every 10 s.
pub fn on_frame(now: Instant) {
    if let Ok(mut g) = LAST_FRAME.lock()
        && let Some(prev) = g.replace(now)
    {
        // Loading screens and hitches are clamped so one long frame does not swing it.
        let us = (now.duration_since(prev).as_secs_f32() * 1e6).clamp(4_000.0, 100_000.0);
        let old = FRAME_US.load(Relaxed) as f32;
        FRAME_US.store((old * 0.95 + us * 0.05) as u32, Relaxed);
    }
    let hz = tuning::player_packet_hz();
    let n = skip_frames(hz, frame_ms(), GAME_FRAMES);
    let key = hz << 8 | n;
    if LOGGED.load(Relaxed) == key {
        return;
    }
    let Ok(mut at) = LOGGED_AT.lock() else { return };
    if at.is_some_and(|t| now.duration_since(t) < Duration::from_secs(10)) {
        return;
    }
    *at = Some(now);
    LOGGED.store(key, Relaxed);
    if INSTALLED.load(Relaxed) {
        log::line(&format!(
            "native: player packet rate setting {hz} Hz: every {n} frames at {:.0} fps (~{:.0}/s)",
            1000.0 / frame_ms(),
            1000.0 / (frame_ms() * n as f32)
        ));
    } else if hz > VANILLA_HZ {
        log::line(&format!("native: player_packet_hz = {hz} applies after a game restart (started with 12)"));
    }
}

/// For the native stats line.
pub fn describe() -> String {
    let n = if INSTALLED.load(Relaxed) { skip_frames(tuning::player_packet_hz(), frame_ms(), GAME_FRAMES) } else { GAME_FRAMES };
    format!("our type 1 every {n} frames at {:.0} fps", 1000.0 / frame_ms())
}

#[cfg(test)]
mod tests {
    use super::*;

    const F60: f32 = 1000.0 / 60.0;
    const F120: f32 = 1000.0 / 120.0;

    #[test]
    fn vanilla_and_below_keep_the_games_count() {
        assert_eq!(skip_frames(12, F60, 5), 5);
        assert_eq!(skip_frames(0, F60, 5), 5);
        assert_eq!(skip_frames(12, F120, 5), 5);
    }

    #[test]
    fn rate_to_frames_at_60_and_120_fps() {
        assert_eq!(skip_frames(30, F60, 5), 2);
        assert_eq!(skip_frames(60, F60, 5), 1);
        assert_eq!(skip_frames(20, F60, 5), 3);
        assert_eq!(skip_frames(15, F60, 5), 4);
        assert_eq!(skip_frames(30, F120, 5), 4);
        assert_eq!(skip_frames(60, F120, 5), 2);
        // A frame rate a little under 60 still gives 2 frames for 30 Hz.
        assert_eq!(skip_frames(30, 16.9, 5), 2);
    }

    #[test]
    fn never_slower_than_the_game_and_never_zero() {
        // 13 Hz at 120 fps would be 9 frames: the game's 5 (24/s) wins.
        assert_eq!(skip_frames(13, F120, 5), 5);
        // 60 Hz at 30 fps: at least one frame.
        assert_eq!(skip_frames(60, 33.3, 5), 1);
        // Above the cap counts as 60 Hz; nonsense frame times stay finite.
        assert_eq!(skip_frames(500, F60, 5), 1);
        assert_eq!(skip_frames(30, f32::NAN, 5), 2);
        assert_eq!(skip_frames(30, 0.0, 5), 5);
        assert_eq!(skip_frames(30, F60, 0), 0);
    }
}
