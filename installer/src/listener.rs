// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Job 2 — putting the listener on the PC.
//!
//! Two steps:
//!
//! 1. Copy the embedded `listener.exe` into its folder.
//! 2. Make it run. On Windows the listener is a *resident* process, so this is
//!    an `HKCU\…\Run` entry. On Linux there will be nothing to autostart at all
//!    — udev starts it per connection — so that step becomes a rules file. See
//!    `../../listener/structure.md`, "Execution models".
//!
//! There used to be a third: writing a `config.toml` and merging the cartridge's
//! key into its `keys` list. Both the file and the concept are gone. The
//! listener decides what to trust from public keys compiled into it, so there is
//! nothing to pair, nothing to copy between two machines, and nothing on disk
//! whose contents could grant a cartridge the right to run. Installing is now
//! one file and one registry value.
//!
//! ## Where it lives: `%LOCALAPPDATA%\GaCaSy`, and only there
//!
//! One location, no choice, no elevation. The listener keeps its **exe and log
//! in one folder**, and this is the folder — the same one
//! `settings::fallback_log_path` in the listener already names, so the log is
//! simply *there* rather than somewhere a second document has to explain.
//!
//! `../structure.md` originally specced `C:\Program Files\GaCaSy\` and named the
//! problem with it in the same breath: the user the listener runs as cannot
//! write there, so its log has to go somewhere else. An all-users install was
//! never buying much either — autostart is `HKCU\…\Run`, per user, wherever the
//! binary sits, so every account that wants the listener registers its own
//! regardless. What Program Files did buy was a UAC prompt, an elevated
//! relaunch path, and a listener whose three files live in two places.
//!
//! So it is gone. What follows from that, and is the point of the change:
//!
//! * The listener's files are always together, always writable, and always in
//!   the place its own documentation points at.
//! * **Nothing in this installer asks for administrator** — not job 1, which
//!   writes to a drive the user can already write to, and now not job 2 either.
//! * There is one path to say out loud, one path to look in when something goes
//!   wrong, and one path to uninstall.
//!
//! [`legacy_dirs`] still knows the two folders earlier builds used, so an
//! install left in one of them can be found and cleared out instead of quietly
//! shadowing the real one.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::autoplay;
use crate::payload;

pub const EXE_NAME: &str = if cfg!(windows) { "listener.exe" } else { "listener" };

/// The config file earlier builds wrote. Nothing reads it any more; it is named
/// here only so an upgrade can clear it away rather than leave a file behind
/// that looks like it still configures something.
const STALE_CONFIG_FILE: &str = "config.toml";

/// The folder name, under `%LOCALAPPDATA%`.
const FOLDER: &str = "GaCaSy";

/// Name of the `Run` value. Also what the user sees in Task Manager's Startup
/// tab, so it is a product name and not an exe name.
pub const AUTOSTART_NAME: &str = "GaCaSy Listener";

/// `%LOCALAPPDATA%\GaCaSy` — the listener's home, and the only place this
/// installer writes it.
///
/// `None` only when the environment doesn't say where `%LOCALAPPDATA%` is,
/// which in practice means a stripped-down service account.
pub fn install_dir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join(FOLDER))
}

/// Folders earlier builds of this installer used, newest first.
///
/// Nothing is ever written to these. They exist so that a PC set up by an
/// earlier build can be recognised, its files removed, and — most importantly —
/// its `Run` entry retired, since a login entry pointing at an exe that is no
/// longer the one being maintained is the kind of fault that only shows up as
/// "it stopped noticing my cartridge".
fn legacy_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(local).join("Programs").join(FOLDER));
    }
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        dirs.push(PathBuf::from(program_files).join(FOLDER));
    }
    dirs
}

/// A listener found on this PC.
pub struct Installed {
    pub dir: PathBuf,
    /// True when the `Run` entry points at *this* install's exe. False means the
    /// binary is there but nothing starts it at login — a repair case.
    pub autostart: bool,
    /// Sitting in a folder an earlier build used. Installing moves it here.
    pub legacy: bool,
}

/// Every listener on this PC: the one in [`install_dir`], plus anything left in
/// a folder an earlier build used.
pub fn find() -> Vec<Installed> {
    let home = install_dir();
    home.clone()
        .into_iter()
        .map(|dir| (dir, false))
        .chain(legacy_dirs().into_iter().map(|dir| (dir, true)))
        // A legacy list that happens to name the current folder (if the two ever
        // coincide on some future layout) must not produce two rows for it.
        .filter(|(dir, legacy)| !legacy || Some(dir) != home.as_ref())
        .filter(|(dir, _)| dir.join(EXE_NAME).is_file())
        .map(|(dir, legacy)| Installed {
            autostart: platform::autostart_target()
                .is_some_and(|target| same_path(&target, &dir.join(EXE_NAME))),
            legacy,
            dir,
        })
        .collect()
}

/// Installs or repairs the listener.
///
/// There is no pairing argument any more, and no pairing step: the listener
/// carries the keys it trusts inside itself, so installing it *is* the whole
/// setup. A cartridge made by this same installer works the moment it is
/// plugged in, on this PC or any other with this listener on it.
///
/// `suppress_autoplay` is the one thing here that reaches outside GaCaSy's own
/// settings, which is why it is a parameter and not a decision made in this
/// function: see [`crate::autoplay`].
///
/// Returns the lines to show the user — what was written where — because "it
/// worked" is not enough for something that leaves no window behind.
pub fn install(start_now: bool, suppress_autoplay: bool) -> Result<Vec<String>, String> {
    if let Some(defect) = payload::defect() {
        return Err(defect);
    }
    let dir = install_dir().ok_or("This account has no %LOCALAPPDATA% to install into.")?;
    let exe = dir.join(EXE_NAME);
    let mut done = Vec::new();

    // Anything an earlier build left elsewhere is cleared out *before* the
    // write, so its login entry stops pointing at a copy nothing maintains any
    // more. Nothing needs carrying over from it now that trust is not on disk.
    done.extend(take_over_legacy_installs());

    fs::create_dir_all(&dir).map_err(|e| format!("{} could not be created: {e}", dir.display()))?;

    // A running listener holds its own exe open, so an upgrade or repair has to
    // stop it first — otherwise the copy fails with a sharing violation that
    // looks like a permissions problem and isn't.
    let stopped = platform::stop_running(&exe);
    if stopped > 0 {
        done.push(format!(
            "Stopped {stopped} running listener{}",
            if stopped == 1 { "" } else { "s" }
        ));
    }

    fs::write(&exe, payload::listener()?)
        .map_err(|e| format!("{} could not be written: {e}", exe.display()))?;
    done.push(format!("Installed {}", exe.display()));

    // An upgrade from a build that had one. Leaving it would strand a file that
    // looks like it still configures something and has not for a while.
    let stale = dir.join(STALE_CONFIG_FILE);
    if stale.is_file() && fs::remove_file(&stale).is_ok() {
        done.push(format!(
            "Removed {} — the listener has no configuration file now",
            stale.display()
        ));
    }

    // Per-user, like everything else here: the listener runs as whoever is
    // logged in, out of that user's own AppData.
    platform::set_autostart(&exe)?;
    done.push(format!("Set it to start at login ({AUTOSTART_NAME})"));

    // Reported, never fatal. The listener is installed and working at this
    // point; AutoPlay is about what else appears on screen beside the launcher,
    // so failing to change it is a worse-looking cartridge insert and not a
    // failed install.
    if suppress_autoplay {
        match autoplay::suppress() {
            Ok(lines) => done.extend(lines),
            Err(e) => done.push(format!(
                "The Windows AutoPlay setting could not be changed ({e}); a folder may still \
                 open when a cartridge is plugged in"
            )),
        }
    }

    if start_now {
        match Command::new(&exe).current_dir(&dir).spawn() {
            // Without this the user has to log out and back in before plugging
            // a cartridge in does anything, which reads as the install having
            // failed.
            Ok(_) => done.push("Started it — plug a cartridge in to test".into()),
            Err(e) => done.push(format!("Could not start it now ({e}); it will start at login")),
        }
    }
    Ok(done)
}

/// Removes the listener: the autostart entry, the running process, the folder.
///
/// Undoing step 3 matters more than deleting the files — a `Run` entry pointing
/// at an exe that is gone is a failed-to-start error at every login.
///
/// The AutoPlay setting is put back only once the **last** listener on this PC
/// is gone. Removing a stray copy an earlier build left behind, while the real
/// install stays, must not hand the user back a PC that opens a folder every
/// time they plug a cartridge in.
pub fn uninstall(dir: &Path) -> Result<Vec<String>, String> {
    let mut done = remove_install(dir)?;
    if find().is_empty() {
        match autoplay::restore() {
            Ok(lines) => done.extend(lines),
            Err(e) => done.push(format!("The AutoPlay setting could not be put back ({e})")),
        }
    }
    if done.is_empty() {
        done.push("Nothing was installed there".into());
    }
    Ok(done)
}

/// Stops, un-registers and deletes the listener at `dir`. Returns a line per
/// thing actually undone, so an install that was already half-gone doesn't
/// report work it didn't do.
///
/// The `Run` entry is only cleared when it points at *this* folder's exe — a PC
/// with a stray legacy copy must not have its working install's login entry
/// removed as a side effect of cleaning that copy up.
fn remove_install(dir: &Path) -> Result<Vec<String>, String> {
    let exe = dir.join(EXE_NAME);
    let mut done = Vec::new();

    if platform::autostart_target().is_some_and(|target| same_path(&target, &exe)) {
        platform::clear_autostart()?;
        done.push("Removed the login entry".into());
    }
    let stopped = platform::stop_running(&exe);
    if stopped > 0 {
        done.push(format!("Stopped {stopped} running listener(s)"));
    }
    if dir.is_dir() {
        fs::remove_dir_all(dir)
            .map_err(|e| format!("{} could not be removed: {e}", dir.display()))?;
        done.push(format!("Removed {}", dir.display()));
    }
    Ok(done)
}

/// Stops, un-registers and deletes any install left in a [`legacy_dirs`] folder.
///
/// This used to also carry the old install's trusted keys forward, which was the
/// important half: removing the folder without them would silently un-pair every
/// cartridge the PC knew. There is nothing to carry now — the replacement
/// listener already trusts everything the old one did, because that list is
/// compiled into both — so this is pure cleanup.
///
/// A folder that refuses to be deleted — a Program Files copy on a PC where this
/// installer isn't elevated — is reported and stepped over, not treated as a
/// failure of the install: the new listener works either way, and the leftover
/// no longer has the login entry.
fn take_over_legacy_installs() -> Vec<String> {
    let mut done = Vec::new();
    for dir in legacy_dirs() {
        if !dir.join(EXE_NAME).is_file() {
            continue;
        }
        done.push(format!("Found an older install in {}", dir.display()));
        match remove_install(&dir) {
            Ok(lines) => done.extend(lines),
            Err(e) => done.push(format!("Could not fully remove it: {e}")),
        }
    }
    done
}

/// Path comparison for the registry value, which may be quoted and may differ in
/// case from what we wrote.
fn same_path(recorded: &str, exe: &Path) -> bool {
    let recorded = recorded.trim().trim_matches('"');
    Path::new(recorded)
        .to_string_lossy()
        .eq_ignore_ascii_case(&exe.to_string_lossy())
}

#[cfg(windows)]
mod platform {
    use std::path::Path;

    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
        QueryFullProcessImageNameW, TerminateProcess,
    };

    use crate::reg::{self, HKEY_CURRENT_USER as HKCU};

    /// Per-user autostart. The listener is resident on Windows — it has to be
    /// running to hear `WM_DEVICECHANGE` — so something must start it at login.
    /// `HKCU\…\Run` is the lightest thing that does, and it needs no admin.
    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

    pub fn set_autostart(exe: &Path) -> Result<(), String> {
        let key = reg::open(HKCU, RUN_KEY, reg::WRITE)
            .ok_or("The Windows Run key could not be opened.")?;
        // Quoted: `C:\Users\First Last\AppData\…` has a space in it whenever the
        // account name does, and an unquoted one is the classic "starts
        // C:\Users\First.exe" bug.
        reg::set_sz(
            &key,
            Some(super::AUTOSTART_NAME),
            &format!("\"{}\"", exe.display()),
        )
        .map_err(|e| format!("The login entry could not be written. {e}"))
    }

    pub fn clear_autostart() -> Result<(), String> {
        let Some(key) = reg::open(HKCU, RUN_KEY, reg::WRITE) else {
            return Ok(()); // nothing to remove from
        };
        reg::delete_value(&key, Some(super::AUTOSTART_NAME));
        Ok(())
    }

    /// What the `Run` entry currently points at, if anything.
    pub fn autostart_target() -> Option<String> {
        let key = reg::open(HKCU, RUN_KEY, reg::READ)?;
        reg::query_sz(&key, Some(super::AUTOSTART_NAME))
    }

    /// Terminates every process running exactly `exe`, returning how many.
    ///
    /// Matched on the full image path, not the file name: killing anything
    /// called `listener.exe` would be a rude and easily-wrong thing to do on
    /// somebody else's PC. There is no gentler signal available — the listener
    /// has no window to close and no IPC to ask through — but it holds no state
    /// beyond its log, so it has nothing to lose by being stopped this way.
    pub fn stop_running(exe: &Path) -> usize {
        let mut stopped = 0;
        let wanted = exe.to_string_lossy().to_ascii_lowercase();
        let name = exe
            .file_name()
            .map(|n| n.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();

        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return 0;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;

            let mut more = Process32FirstW(snapshot, &mut entry);
            while more != 0 {
                if from_wide(&entry.szExeFile).to_ascii_lowercase() == name
                    && let Some(path) = image_path(entry.th32ProcessID)
                    && path.to_ascii_lowercase() == wanted
                {
                    let handle = OpenProcess(PROCESS_TERMINATE, 0, entry.th32ProcessID);
                    if !handle.is_null() {
                        if TerminateProcess(handle, 0) != 0 {
                            stopped += 1;
                        }
                        CloseHandle(handle);
                    }
                }
                more = Process32NextW(snapshot, &mut entry);
            }
            CloseHandle(snapshot);
        }
        stopped
    }

    fn image_path(pid: u32) -> Option<String> {
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return None;
            }
            let mut buffer = [0u16; 32768];
            let mut size = buffer.len() as u32;
            let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size);
            CloseHandle(handle);
            (ok != 0).then(|| String::from_utf16_lossy(&buffer[..size as usize]))
        }
    }

    fn from_wide(buffer: &[u16]) -> String {
        let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..end])
    }
}

#[cfg(not(windows))]
mod platform {
    use std::path::Path;

    /// Linux activation is a udev rule, not an autostart entry — the listener
    /// there is one-shot and has nothing to keep running. See `../structure.md`,
    /// "Future".
    pub fn set_autostart(_exe: &Path) -> Result<(), String> {
        Err("Installing the listener is Windows-only in v1.".into())
    }
    pub fn clear_autostart() -> Result<(), String> {
        Ok(())
    }
    pub fn autostart_target() -> Option<String> {
        None
    }
    pub fn stop_running(_exe: &Path) -> usize {
        0
    }
}
