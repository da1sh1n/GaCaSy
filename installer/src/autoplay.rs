// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Reads, rewrites and restores the per-user AutoPlay choice for removable
//! drives, backing up the previous value under our own key.

// ########## AUTOPLAY SUPPRESSION ##########

/// The AutoPlay event for "a drive with ordinary files on it just arrived".
/// Named separately from the paths below because it is the thing being talked
/// about; the paths are just where Windows keeps the answer.
#[cfg(windows)]
const CHOSEN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\AutoplayHandlers\UserChosenExecuteHandlers\StorageOnArrival";

/// The parallel key the Settings app reads to show the current selection.
/// Writing only [`CHOSEN_KEY`] works, but leaves Settings displaying the old
/// choice — which reads as the change not having taken.
#[cfg(windows)]
const DEFAULT_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\AutoplayHandlers\EventHandlersDefaultSelection\StorageOnArrival";

/// The handler that means "do nothing". A real registered handler
/// (`HKLM\…\AutoplayHandlers\Handlers\MSTakeNoAction`, ProgID
/// `Shell.AutoplaySpecial`), not an invented value — deleting the choice
/// instead would fall back to the "ask me every time" popup, which is still a
/// thing appearing over the launcher.
#[cfg(windows)]
const TAKE_NO_ACTION: &str = "MSTakeNoAction";

/// Ours, and the only key outside AutoPlay's own that this program writes. What
/// was there before we changed it is parked here so uninstalling can put it
/// back — a setting silently changed and never restored is the kind of thing
/// people find years later and cannot explain.
#[cfg(windows)]
const BACKUP_KEY: &str = r"Software\Romzeta\AutoPlay";

#[cfg(windows)]
const BACKUP_CHOSEN: &str = "PreviousChosen";
#[cfg(windows)]
const BACKUP_DEFAULT: &str = "PreviousDefault";

/// Recorded when the value we are replacing was not set at all, so that
/// restoring knows to delete rather than to write something back. A literal
/// handler name could never collide with this, since handler names are registry
/// key names.
#[cfg(windows)]
const NONE_SENTINEL: &str = "<none>";

#[cfg(windows)]
mod platform {
    use super::*;
    use crate::reg::{self, HKEY_CURRENT_USER as HKCU};

    /// The handler AutoPlay will run for an arriving drive, if any is chosen.
    pub fn currentChoice() -> Option<String> {
        let key = reg::open(HKCU, CHOSEN_KEY, reg::READ)?;
        reg::querySz(&key, None).filter(|choice| !choice.is_empty())
    }

    pub fn suppressed() -> bool {
        currentChoice().is_some_and(|choice| choice.eq_ignore_ascii_case(TAKE_NO_ACTION))
    }

    pub fn suppress() -> Result<Vec<String>, String> {
        let previous_chosen = read(CHOSEN_KEY);
        let previous_default = read(DEFAULT_KEY);

        // Only ever taken once. Repair/update runs the install again, and a
        // second backup would record the MSTakeNoAction *we* wrote as the user's
        // own preference — after which uninstalling would "restore" the
        // suppression and never give the original setting back.
        if !backedUp() {
            let backup = reg::create(HKCU, BACKUP_KEY)?;
            reg::setSz(
                &backup,
                Some(BACKUP_CHOSEN),
                previous_chosen.as_deref().unwrap_or(NONE_SENTINEL),
            )?;
            reg::setSz(
                &backup,
                Some(BACKUP_DEFAULT),
                previous_default.as_deref().unwrap_or(NONE_SENTINEL),
            )?;
        }

        write(CHOSEN_KEY, TAKE_NO_ACTION)?;
        write(DEFAULT_KEY, TAKE_NO_ACTION)?;

        Ok(vec![
            "Set Windows AutoPlay for removable drives to \"Take no action\" — no folder \
             will open over the launcher"
                .into(),
        ])
    }

    pub fn restore() -> Result<Vec<String>, String> {
        let Some(backup) = reg::open(HKCU, BACKUP_KEY, reg::READ) else {
            return Ok(Vec::new()); // never suppressed, so nothing to undo
        };
        let chosen = reg::querySz(&backup, Some(BACKUP_CHOSEN));
        let default = reg::querySz(&backup, Some(BACKUP_DEFAULT));
        drop(backup); // the key cannot be deleted while it is open

        putBack(CHOSEN_KEY, chosen.as_deref())?;
        putBack(DEFAULT_KEY, default.as_deref())?;
        reg::deleteKey(HKCU, BACKUP_KEY);

        Ok(vec![
            "Put your Windows AutoPlay setting for removable drives back as it was".into(),
        ])
    }

    fn backedUp() -> bool {
        reg::open(HKCU, BACKUP_KEY, reg::READ)
            .and_then(|key| reg::querySz(&key, Some(BACKUP_CHOSEN)))
            .is_some()
    }

    fn read(path: &str) -> Option<String> {
        let key = reg::open(HKCU, path, reg::READ)?;
        reg::querySz(&key, None).filter(|value| !value.is_empty())
    }

    fn write(path: &str, value: &str) -> Result<(), String> {
        // Created rather than opened: EventHandlersDefaultSelection\… is absent
        // on a profile where AutoPlay has never been touched.
        let key = reg::create(HKCU, path)?;
        reg::setSz(&key, None, value)
    }

    /// Restores one value, where "there was nothing here" is itself a state
    /// worth restoring — writing an empty string instead would leave AutoPlay
    /// with a chosen handler of `""`.
    fn putBack(path: &str, previous: Option<&str>) -> Result<(), String> {
        match previous {
            None | Some(NONE_SENTINEL) => {
                if let Some(key) = reg::open(HKCU, path, reg::WRITE) {
                    reg::deleteValue(&key, None);
                }
                Ok(())
            }
            Some(value) => write(path, value),
        }
    }
}

/// Linux has no AutoPlay and no Explorer window to suppress; what a desktop
/// environment does with a mounted volume is its own business and not something
/// an installer should be rewriting. See `../../listener/structure.md`.
#[cfg(not(windows))]
mod platform {
    pub fn currentChoice() -> Option<String> {
        None
    }
    pub fn suppressed() -> bool {
        false
    }
    pub fn suppress() -> Result<Vec<String>, String> {
        Ok(Vec::new())
    }
    pub fn restore() -> Result<Vec<String>, String> {
        Ok(Vec::new())
    }
}

pub use platform::{currentChoice, restore, suppress, suppressed};

/// Whether AutoPlay is currently set to open a folder — the specific state the
/// checkbox exists to fix, as opposed to "ask me every time" or any other
/// handler, which are wrong for a cartridge too but less obviously so.
pub fn opensAFolder() -> bool {
    currentChoice().is_some_and(|choice| choice.eq_ignore_ascii_case("MSOpenFolder"))
}
