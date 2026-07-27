// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! The Windows trigger: resident, event-driven, 0% CPU while idle.
//!
//! A hidden window and a message loop, and that is the whole program. It costs
//! a couple of megabytes of working set and nothing else, because the process
//! spends its entire life blocked in `GetMessage` — a real kernel wait, not a
//! sleep in a loop. **There is no polling fallback and there must never be
//! one**: that idle cost is the entire justification for staying resident
//! rather than being started per event (see `../../structure.md`,
//! "Why not one-shot on Windows too").
//!
//! Three details are worth knowing before touching this file:
//!
//! * The window is **top-level, not message-only**. Broadcast
//!   `WM_DEVICECHANGE` volume notifications are delivered to top-level windows
//!   only. An `HWND_MESSAGE` window compiles, runs, and silently receives
//!   nothing forever — the most expensive way to get this wrong.
//! * `dbcv_unitmask` is a **bitmask** of drive letters, not one letter. A
//!   multi-partition device arrives as a single event carrying several.
//! * A cartridge plugged in *before* login never produces an arrival event at
//!   all, hence the startup sweep.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::ptr;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
use windows_sys::Win32::System::Diagnostics::Debug::{SEM_FAILCRITICALERRORS, SetErrorMode};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::System::WindowsProgramming::{DRIVE_FIXED, DRIVE_REMOVABLE};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DBT_DEVICEARRIVAL, DBT_DEVTYP_VOLUME, DBTF_NET,
    DEV_BROADCAST_HDR, DEV_BROADCAST_VOLUME, DefWindowProcW, DispatchMessageW, GetMessageW, MSG,
    PostQuitMessage, RegisterClassW, TranslateMessage, WM_DESTROY, WM_DEVICECHANGE, WNDCLASSW,
    WS_OVERLAPPED,
};

use crate::log::Log;
use crate::{settings, volume};

/// Its own name, distinct from the launcher's `Local\GaCaSy.CartridgeLauncher`.
/// `Local\` scopes it to the login session, which is the right scope for
/// something started per user by a `Run` entry.
const INSTANCE_MUTEX: &str = r"Local\GaCaSy.CartridgeListener";

const WINDOW_CLASS: &str = "GaCaSy.ListenerWindow";

/// Everything the window procedure needs.
///
/// Held in a thread-local rather than threaded through `GWLP_USERDATA`: the
/// window, the message loop and every `wnd_proc` call all live on the one
/// thread `run` is called from, so a thread-local is the same guarantee with
/// none of the pointer casting.
struct State {
    log: Log,
    /// When each drive letter was last acted on, for the debounce below.
    recent: HashMap<char, Instant>,
}

impl State {
    /// True when this letter was handled recently enough that this arrival is
    /// a repeat rather than a new connection.
    ///
    /// A flaky USB link fires several `DBT_DEVICEARRIVAL`s for one physical
    /// plug-in, and without this each one starts another launcher. Keyed on
    /// the drive letter, so swapping a *different* cartridge into the same
    /// letter inside the window would also be skipped — at a few seconds that
    /// is not a real sequence, and the alternative (keying on something about
    /// the volume) means reading and verifying it before deciding to skip it,
    /// which is most of the work the debounce exists to avoid.
    fn debounced(&mut self, letter: char) -> bool {
        let window = Duration::from_secs(settings::DEBOUNCE_SECONDS);
        let now = Instant::now();
        if !window.is_zero()
            && let Some(previous) = self.recent.get(&letter)
            && now.duration_since(*previous) < window
        {
            return true;
        }
        self.recent.insert(letter, now);
        false
    }
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Runs until logout. Returns early (without launching anything) if another
/// instance already holds the mutex.
pub fn run(log: Log) {
    let Some(_instance) = acquire_single_instance() else {
        // The `Run` entry can fire twice across a fast logoff/logon, and two
        // listeners racing on one arrival means two launchers on screen.
        log.line("another listener is already running; exiting");
        return;
    };

    // Sweeping drive letters touches removable drives, and an empty card
    // reader would otherwise pop the modal "There is no disk in the drive"
    // box — from a process with no visible window, which is unclosable-looking
    // and inexplicable. Failing the call silently is exactly what we want.
    unsafe { SetErrorMode(SEM_FAILCRITICALERRORS) };

    log.line("listener started");
    STATE.with(|state| {
        *state.borrow_mut() = Some(State {
            log,
            recent: HashMap::new(),
        })
    });

    let Some(hwnd) = create_hidden_window() else {
        with_state(|state| {
            state
                .log
                .line("FAILED to create the listener window; exiting")
        });
        return;
    };

    // After the window exists, so an arrival that happens mid-sweep is queued
    // rather than missed. The debounce then keeps the queued event from
    // re-launching what the sweep already picked up.
    startup_sweep();

    message_loop();
    let _ = hwnd;
    with_state(|state| state.log.line("listener stopped"));
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|state| state.borrow_mut().as_mut().map(f))
}

// ── Single instance ──────────────────────────────────────────────────────

/// Holds the process-wide mutex; dropping it (or exiting) frees the name.
struct InstanceGuard(HANDLE);

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CloseHandle(self.0) };
        }
    }
}

/// `Some` if this is the first instance, `None` if one is already running.
/// Same named-mutex pattern as `../../../launcher/src/main.rs`.
fn acquire_single_instance() -> Option<InstanceGuard> {
    let name = wide(INSTANCE_MUTEX);
    unsafe {
        let handle = CreateMutexW(ptr::null(), 0, name.as_ptr());
        if handle.is_null() {
            // Couldn't create the mutex at all — don't let the guard become a
            // reason the listener refuses to run.
            return Some(InstanceGuard(handle));
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(handle);
            return None;
        }
        Some(InstanceGuard(handle))
    }
}

// ── Window and message loop ──────────────────────────────────────────────

/// Registers the class and creates the (never shown) top-level window.
///
/// It is never passed to `ShowWindow`, so it stays invisible with no taskbar
/// button — but it *is* a real top-level window, which is what makes broadcast
/// `WM_DEVICECHANGE` reach it. Using `HWND_MESSAGE` here would be the classic
/// silent failure; see this module's header.
fn create_hidden_window() -> Option<HWND> {
    unsafe {
        let instance = GetModuleHandleW(ptr::null());
        let class_name = wide(WINDOW_CLASS);

        let mut class: WNDCLASSW = std::mem::zeroed();
        class.lpfnWndProc = Some(wnd_proc);
        class.hInstance = instance;
        class.lpszClassName = class_name.as_ptr();
        if RegisterClassW(&class) == 0 {
            return None;
        }

        let title = wide("GaCaSy Listener");
        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPED,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            0,
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null(),
        );
        (!hwnd.is_null()).then_some(hwnd)
    }
}

/// Blocks in `GetMessage` until the window is destroyed or the session ends.
/// This call, and not a timer, is why the idle CPU cost is zero.
fn message_loop() {
    let mut message: MSG = unsafe { std::mem::zeroed() };
    loop {
        // 0 = WM_QUIT, -1 = error. Either way there is nothing left to pump.
        let result = unsafe { GetMessageW(&mut message, ptr::null_mut(), 0, 0) };
        if result <= 0 {
            return;
        }
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_DEVICECHANGE if wparam as u32 == DBT_DEVICEARRIVAL => {
            unsafe { on_device_arrival(lparam) };
            // TRUE: the message was handled. Device-change broadcasts are
            // documented to be answered this way.
            1
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

/// Decodes one `DBT_DEVICEARRIVAL` and runs the shared core over every drive
/// letter it names.
///
/// # Safety
///
/// `lparam` must be the pointer Windows passed with the message: either null,
/// or a valid `DEV_BROADCAST_HDR` whose `dbch_devicetype` says what the rest of
/// the allocation actually is.
unsafe fn on_device_arrival(lparam: LPARAM) {
    if lparam == 0 {
        return;
    }
    // Not every arrival is a volume — ports, interfaces and handles use this
    // message too, and their payloads are differently shaped. The header's
    // device type is the only thing safe to read before deciding.
    let header = lparam as *const DEV_BROADCAST_HDR;
    if unsafe { (*header).dbch_devicetype } != DBT_DEVTYP_VOLUME {
        return;
    }

    let volume = lparam as *const DEV_BROADCAST_VOLUME;
    let (unitmask, flags) = unsafe { ((*volume).dbcv_unitmask, (*volume).dbcv_flags) };

    // A mapped network share arrives as a volume like any other. It cannot be
    // a cartridge, and probing one can block on an unreachable server, so it
    // is dropped before any file access.
    if flags & DBTF_NET != 0 {
        with_state(|state| state.log.line("ignored a network drive arrival"));
        return;
    }

    // The bitmask, not a single letter: one event can carry several.
    for letter in letters_from_mask(unitmask) {
        handle_letter(letter, "arrival");
    }
}

/// Runs the shared core over one drive letter, subject to the drive-type
/// filter and the debounce.
fn handle_letter(letter: char, reason: &str) {
    with_state(|state| {
        let drive_type = drive_type(letter);
        if !is_candidate_drive(drive_type) {
            state.log.line(&format!(
                "{letter}: ignored on {reason}: drive type {drive_type} is not local storage"
            ));
            return;
        }
        if state.debounced(letter) {
            state.log.line(&format!(
                "{letter}: ignored on {reason}: handled moments ago"
            ));
            return;
        }
        let root = drive_root(letter);
        volume::handle_volume(&root, &state.log);
    });
}

// ── Volume enumeration ───────────────────────────────────────────────────

/// Runs the core over every drive already mounted.
///
/// Without this the listener only ever works if you plug the cartridge in
/// *after* logging in — a cartridge that was already connected at boot
/// produces no arrival event, and the failure reads as flakiness rather than
/// as a missing feature.
fn startup_sweep() {
    for letter in mounted_drive_letters() {
        handle_letter(letter, "startup sweep");
    }
}

/// Every drive letter currently mounted, A–Z.
fn mounted_drive_letters() -> Vec<char> {
    letters_from_mask(unsafe { GetLogicalDrives() })
}

/// Expands a 26-bit drive-letter bitmask (bit 0 = `A`) into letters.
fn letters_from_mask(mask: u32) -> Vec<char> {
    (0..26)
        .filter(|bit| mask & (1 << bit) != 0)
        .map(|bit| (b'A' + bit as u8) as char)
        .collect()
}

fn drive_root(letter: char) -> PathBuf {
    PathBuf::from(format!("{letter}:\\"))
}

fn drive_type(letter: char) -> u32 {
    let root = wide(&format!("{letter}:\\"));
    unsafe { GetDriveTypeW(root.as_ptr()) }
}

/// Whether a drive is the kind of thing a cartridge can be.
///
/// "Any mounted storage volume" is the rule — NVMe, SSD, HDD and USB stick
/// alike, never a specific USB id — but that is *local* storage. Network
/// shares, optical drives and RAM disks are excluded here rather than left to
/// the missing `.cartridge` to reject, because reaching for a file on a stale
/// network mount can block for a long time and this runs on the message thread.
fn is_candidate_drive(drive_type: u32) -> bool {
    matches!(drive_type, DRIVE_FIXED | DRIVE_REMOVABLE)
}

/// A NUL-terminated UTF-16 buffer for the Win32 `…W` entry points.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_a_unit_mask_into_every_letter_it_names() {
        assert_eq!(letters_from_mask(0), Vec::<char>::new());
        assert_eq!(letters_from_mask(1), vec!['A']);
        // The case the bitmask exists for: one event, several partitions.
        assert_eq!(letters_from_mask(0b1_0001_0000), vec!['E', 'I']);
        assert_eq!(letters_from_mask(1 << 25), vec!['Z']);
        // Bits above Z are not letters and must not be invented.
        assert_eq!(letters_from_mask(u32::MAX).len(), 26);
    }

    #[test]
    fn only_local_storage_is_a_candidate() {
        assert!(is_candidate_drive(DRIVE_FIXED));
        assert!(is_candidate_drive(DRIVE_REMOVABLE));
        for other in [
            windows_sys::Win32::System::WindowsProgramming::DRIVE_REMOTE,
            windows_sys::Win32::System::WindowsProgramming::DRIVE_CDROM,
            windows_sys::Win32::System::WindowsProgramming::DRIVE_RAMDISK,
            windows_sys::Win32::System::WindowsProgramming::DRIVE_NO_ROOT_DIR,
            windows_sys::Win32::System::WindowsProgramming::DRIVE_UNKNOWN,
        ] {
            assert!(!is_candidate_drive(other));
        }
    }

    #[test]
    fn drive_roots_are_absolute_letter_paths() {
        assert_eq!(drive_root('E'), PathBuf::from(r"E:\"));
    }

    #[test]
    fn wide_strings_are_nul_terminated() {
        assert_eq!(wide("A:"), vec![65, 58, 0]);
    }
}
