// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! The Windows trigger: a hidden top-level window and a message loop blocked in
//! `GetMessage`, waiting for `WM_DEVICECHANGE`. Also owns the tray icon, its
//! menu, the single-instance mutex, and the startup sweep of mounted drives.
//!
//! The window is top-level, not `HWND_MESSAGE`: broadcast device notifications
//! reach nothing else. `dbcv_unitmask` is a bitmask, so one event can carry
//! several drive letters.

// ########## THE WINDOWS TRIGGER ##########

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::ptr;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, LPARAM, LRESULT, POINT, WPARAM,
};
use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
use windows_sys::Win32::System::Diagnostics::Debug::{SEM_FAILCRITICALERRORS, SetErrorMode};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::System::WindowsProgramming::{DRIVE_FIXED, DRIVE_REMOVABLE};
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
    ShellExecuteW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CW_USEDEFAULT, CreatePopupMenu, CreateWindowExW, DBT_DEVICEARRIVAL,
    DBT_DEVTYP_VOLUME, DBTF_NET, DEV_BROADCAST_HDR, DEV_BROADCAST_VOLUME, DefWindowProcW,
    DestroyMenu, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW, LoadIconW, MF_STRING,
    MSG, PostQuitMessage, RegisterClassW, SW_SHOWNORMAL, SetForegroundWindow, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WM_APP, WM_CONTEXTMENU, WM_DESTROY,
    WM_DEVICECHANGE, WM_RBUTTONUP, WNDCLASSW, WS_OVERLAPPED,
};

use common::utf16::wide;

use crate::log::Log;
use crate::{settings, volume};

/// The listener's mutex name, distinct from the launcher's
/// `Local\Romzeta.CartridgeLauncher`. `Local\` scopes it to the login session,
/// matching a `Run` entry's per-user lifetime.
const INSTANCE_MUTEX: &str = r"Local\Romzeta.CartridgeListener";

const WINDOW_CLASS: &str = "Romzeta.ListenerWindow";

/// `uID` `Shell_NotifyIconW` identifies this icon by, alongside `hWnd`. One
/// tray icon per process, so any constant does — it only has to be stable
/// between the `NIM_ADD` in `addTrayIcon` and the `NIM_DELETE` on shutdown.
const TRAY_ICON_UID: u32 = 1;

/// The icon resource `build.rs` compiles in via `winres`, which assigns id
/// `1` to the first (and only) `set_icon` call.
///
/// This is Win32's `MAKEINTRESOURCE`: an integer id passed where an `LPCWSTR`
/// is expected, which the loader tells apart from a real string by its value
/// being below 65536. `without_provenance` is the honest spelling of "this
/// address is a token, not memory" — it must stay exactly 1, so `ptr::dangling`
/// is not a substitute (that yields `align_of::<u16>()`, which is 2).
const TRAY_ICON_RESOURCE: *const u16 = std::ptr::without_provenance(1);

/// Custom message `Shell_NotifyIconW` delivers mouse activity on the tray
/// icon through. `WM_APP` is the documented start of the range an
/// application is free to define its own messages in.
const WM_TRAYICON: u32 = WM_APP + 1;

const ID_MENU_OPEN_LOG: u32 = 1;
const ID_MENU_EXIT: u32 = 2;

/// Everything the window procedure needs.
///
/// Held in a thread-local rather than threaded through `GWLP_USERDATA`: the
/// window, the message loop and every `wndProc` call all live on the one
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

/// Takes ownership of `log` and runs until logout: sweeps the drives already
/// mounted, then pumps messages. Returns early, launching nothing, if another
/// instance already holds the mutex.
pub fn run(log: Log) {
    let Some(_instance) = acquireSingleInstance() else {
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

    let Some(hwnd) = createHiddenWindow() else {
        withState(|state| {
            state
                .log
                .line("FAILED to create the listener window; exiting")
        });
        return;
    };

    // A tray icon is a courtesy, not a requirement — a cartridge still gets
    // noticed and launched without one — so failing to add it is logged and
    // otherwise ignored rather than treated as a reason to exit.
    if !addTrayIcon(hwnd) {
        withState(|state| {
            state
                .log
                .line("failed to add the tray icon; continuing without one")
        });
    }

    // After the window exists, so an arrival that happens mid-sweep is queued
    // rather than missed. The debounce then keeps the queued event from
    // re-launching what the sweep already picked up.
    startupSweep();

    messageLoop();
    let _ = hwnd;
    withState(|state| state.log.line("listener stopped"));
}

/// Runs `f` against the thread-local state, handing back whatever it returns.
/// `None` when `run` has not populated the state yet, which is the case for
/// every callback that could somehow fire before startup finished.
fn withState<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    // `borrow_mut` panics on a nested call, so nothing inside `f` may reach
    // back into `withState` — every caller here is a leaf.
    STATE.with(|state| state.borrow_mut().as_mut().map(f))
}

// ========== Single Instance ==========

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
fn acquireSingleInstance() -> Option<InstanceGuard> {
    let name = wide(INSTANCE_MUTEX);
    unsafe {
        // A named mutex, not a lock file: the kernel frees the name when the
        // process dies, however it dies, so a crash cannot leave a stale claim.
        let handle = CreateMutexW(ptr::null(), 0, name.as_ptr());
        if handle.is_null() {
            // Couldn't create the mutex at all — don't let the guard become a
            // reason the listener refuses to run.
            return Some(InstanceGuard(handle));
        }
        // CreateMutexW succeeds either way; the error code is the only thing
        // that distinguishes "created it" from "opened someone else's".
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(handle);
            return None;
        }
        Some(InstanceGuard(handle))
    }
}

// ========== Window And Message Loop ==========

/// Registers the class and creates the (never shown) top-level window.
///
/// It is never passed to `ShowWindow`, so it stays invisible with no taskbar
/// button — but it *is* a real top-level window, which is what makes broadcast
/// `WM_DEVICECHANGE` reach it. Using `HWND_MESSAGE` here would be the classic
/// silent failure; see this module's header.
fn createHiddenWindow() -> Option<HWND> {
    unsafe {
        let instance = GetModuleHandleW(ptr::null());
        let class_name = wide(WINDOW_CLASS);

        // `zeroed` then set what matters: WNDCLASSW has a dozen fields Windows
        // reads as "default" when they are null, and naming them all is noise.
        let mut class: WNDCLASSW = std::mem::zeroed();
        class.lpfnWndProc = Some(wndProc);
        class.hInstance = instance;
        class.lpszClassName = class_name.as_ptr();
        if RegisterClassW(&class) == 0 {
            return None;
        }

        let title = wide("Romzeta Listener");
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
fn messageLoop() {
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

/// The window procedure Windows calls for every message this window receives.
/// Handles device arrivals, tray clicks and destruction, and hands everything
/// else to the default handler. The return value's meaning is per-message.
///
/// # Safety
///
/// Called only by Windows, with the arguments it documents for each message.
/// `extern "system"` because Windows calls it, not Rust.
unsafe extern "system" fn wndProc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_DEVICECHANGE if wparam as u32 == DBT_DEVICEARRIVAL => {
            unsafe { onDeviceArrival(lparam) };
            // TRUE: the message was handled. Device-change broadcasts are
            // documented to be answered this way.
            1
        }
        WM_TRAYICON => {
            // With no `NIM_SETVERSION` call, the shell reports mouse activity
            // the old way: `lparam` is the mouse message itself, the same
            // value a click on a real window would have arrived as.
            if matches!(lparam as u32, WM_RBUTTONUP | WM_CONTEXTMENU) {
                unsafe { showTrayMenu(hwnd) };
            }
            0
        }
        WM_DESTROY => {
            removeTrayIcon(hwnd);
            unsafe { PostQuitMessage(0) };
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

// ========== Tray Icon ==========

/// Adds the tray icon. `false` if the shell refused it, which is not fatal —
/// see the call site in `run`.
fn addTrayIcon(hwnd: HWND) -> bool {
    unsafe {
        let instance = GetModuleHandleW(ptr::null());
        let icon = LoadIconW(instance, TRAY_ICON_RESOURCE);

        let mut data: NOTIFYICONDATAW = std::mem::zeroed();
        data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = hwnd;
        data.uID = TRAY_ICON_UID;
        data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        data.uCallbackMessage = WM_TRAYICON;
        data.hIcon = icon;
        setTip(&mut data.szTip, "Romzeta Listener");

        Shell_NotifyIconW(NIM_ADD, &data) != 0
    }
}

/// Removes the tray icon on the way out, so it doesn't linger as a stale
/// entry in the hidden-icons flyout after the process has already exited.
fn removeTrayIcon(hwnd: HWND) {
    unsafe {
        let mut data: NOTIFYICONDATAW = std::mem::zeroed();
        data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = hwnd;
        data.uID = TRAY_ICON_UID;
        Shell_NotifyIconW(NIM_DELETE, &data);
    }
}

/// Copies `text` (truncated to fit) into a `NOTIFYICONDATAW` fixed-size
/// UTF-16 field, NUL-terminated.
fn setTip(field: &mut [u16], text: &str) {
    let wide = wide(text);
    let n = wide.len().min(field.len());
    let truncated = n == field.len();
    field[..n].copy_from_slice(&wide[..n]);
    if truncated {
        // `wide()` already NUL-terminates; this only matters when truncation
        // above cut the NUL off the end.
        field[n - 1] = 0;
    }
}

/// Builds and shows the right-click menu at the cursor, then acts on
/// whichever item (if any) was picked.
unsafe fn showTrayMenu(hwnd: HWND) {
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        let open_log = wide("Open log");
        let exit = wide("Exit");
        AppendMenuW(
            menu,
            MF_STRING,
            ID_MENU_OPEN_LOG as usize,
            open_log.as_ptr(),
        );
        AppendMenuW(menu, MF_STRING, ID_MENU_EXIT as usize, exit.as_ptr());

        let mut point: POINT = std::mem::zeroed();
        GetCursorPos(&mut point);

        // A window that isn't the foreground window never gets the message
        // that tells a popup menu to dismiss itself when the user clicks
        // elsewhere — the classic Win32 "menu that won't go away" bug. This
        // hidden window is never activated any other way, so it has to be
        // forced here.
        SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            0,
            hwnd,
            ptr::null(),
        );
        DestroyMenu(menu);

        match cmd as u32 {
            ID_MENU_OPEN_LOG => openLogFile(),
            ID_MENU_EXIT => {
                DestroyWindow(hwnd);
            }
            _ => {}
        }
    }
}

/// Opens the log with whatever the user has associated with `.log` files —
/// same as double-clicking it in Explorer. The listener has no window of its
/// own to display the log in, and doesn't need one just for this.
fn openLogFile() {
    withState(|state| {
        let Some(path) = state.log.path() else {
            return;
        };
        let verb = wide("open");
        let file = wide(&path.to_string_lossy());
        unsafe {
            ShellExecuteW(
                ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                ptr::null(),
                ptr::null(),
                SW_SHOWNORMAL,
            );
        }
    });
}

/// Decodes one `DBT_DEVICEARRIVAL` and runs the shared core over every drive
/// letter it names.
///
/// # Safety
///
/// `lparam` must be the pointer Windows passed with the message: either null,
/// or a valid `DEV_BROADCAST_HDR` whose `dbch_devicetype` says what the rest of
/// the allocation actually is.
unsafe fn onDeviceArrival(lparam: LPARAM) {
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
        withState(|state| state.log.line("ignored a network drive arrival"));
        return;
    }

    // The bitmask, not a single letter: one event can carry several.
    for letter in lettersFromMask(unitmask) {
        handleLetter(letter, "arrival");
    }
}

/// Runs the shared core over one drive letter, subject to the drive-type filter
/// and the debounce. `reason` names what brought us here, for the log line.
fn handleLetter(letter: char, reason: &str) {
    withState(|state| {
        let drive_type = driveType(letter);
        if !isCandidateDrive(drive_type) {
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
        let root = driveRoot(letter);
        volume::handleVolume(&root, &state.log);
    });
}

// ========== Volume Enumeration ==========

/// Runs the core over every drive already mounted.
///
/// Without this the listener only ever works if you plug the cartridge in
/// *after* logging in — a cartridge that was already connected at boot
/// produces no arrival event, and the failure reads as flakiness rather than
/// as a missing feature.
fn startupSweep() {
    for letter in mountedDriveLetters() {
        handleLetter(letter, "startup sweep");
    }
}

/// Every drive letter currently mounted, A–Z.
fn mountedDriveLetters() -> Vec<char> {
    // Same bitmask shape as an arrival event, so one expander serves both.
    lettersFromMask(unsafe { GetLogicalDrives() })
}

/// Expands a 26-bit drive-letter bitmask (bit 0 = `A`) into the letters it sets.
fn lettersFromMask(mask: u32) -> Vec<char> {
    (0..26)
        .filter(|bit| mask & (1 << bit) != 0)
        // Arithmetic on the ASCII byte, then back to a char — every drive
        // letter is A–Z, so there is no multi-byte case to worry about.
        .map(|bit| (b'A' + bit as u8) as char)
        .collect()
}

/// A drive letter as a root path, e.g. `E:\`.
fn driveRoot(letter: char) -> PathBuf {
    PathBuf::from(format!("{letter}:\\"))
}

/// What kind of drive is mounted at `letter` — one of the Win32 `DRIVE_*`
/// values. `DRIVE_NO_ROOT_DIR` for a letter nothing is mounted at.
fn driveType(letter: char) -> u32 {
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
fn isCandidateDrive(drive_type: u32) -> bool {
    matches!(drive_type, DRIVE_FIXED | DRIVE_REMOVABLE)
}
