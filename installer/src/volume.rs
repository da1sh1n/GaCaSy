// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Which drives may become a cartridge: external storage only. The drive
//! Windows is on and every internal disk are refused.
//!
//! Externality comes from the hardware, not `DRIVE_REMOVABLE` — a USB SSD in an
//! enclosure reports `DRIVE_FIXED` like an internal disk. So
//! `IOCTL_STORAGE_QUERY_PROPERTY` supplies the bus type, and USB, FireWire, SD
//! and MMC count as external. The query wants a handle with zero access rights,
//! so none of this needs administrator. A Thunderbolt/PCIe enclosure reports
//! `BusTypeNvme` and is refused; Windows-To-Go passes the bus test, so the
//! system-drive check runs first.
//!
//! A cartridge's name is its volume label. `setLabel` is the only write here;
//! nothing formats, partitions or erases.

// ########## ELIGIBLE VOLUMES ##########

use std::path::{Path, PathBuf};

use trust::Anchor;

// `ANCHORS: &[Anchor]`, written by build.rs from keys/*.pub — the same constant,
// generated the same way, as the listener's. Compiled in rather than read from
// disk: an anchor in a writable file beside the exe would let anything that
// could edit it decide what this program trusts.
include!(concat!(env!("OUT_DIR"), "/trust_anchors.rs"));

/// The version of `<root>/launcher.exe`, if it is a launcher this build's keys
/// vouch for. `None` for no launcher, an unsigned one, a stranger's, or a
/// genuine Romzeta binary that is not a launcher — all four mean "not a
/// cartridge this installer made", which is the only distinction the next
/// screen needs.
///
/// **Nothing is executed**: the version comes out of the signed comment, so no
/// binary off a stranger's USB stick is ever run to find out what it is.
pub fn attestedLauncher(root: &Path) -> Option<String> {
    let path = root.join(crate::cartridge::LAUNCHER_NAME);
    if !path.is_file() {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    trust::attest(&bytes, ANCHORS, trust::LAUNCHER_ROLE)
        .ok()
        .map(|attested| attested.version)
}

/// Whether a drive may be written to, and if not, why not.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Eligibility {
    /// External storage. The only kind a cartridge is made on.
    Allowed,
    /// Windows itself lives here. Refused before anything else is asked.
    SystemDrive,
    /// Internal storage — SATA, NVMe, RAID and so on.
    Internal,
}

impl Eligibility {
    pub fn allowed(self) -> bool {
        self == Eligibility::Allowed
    }

    /// The sentence shown beside a drive that can't be picked.
    pub fn reason(self) -> &'static str {
        match self {
            Eligibility::Allowed => "",
            Eligibility::SystemDrive => "Windows is installed here — never usable as a cartridge.",
            Eligibility::Internal => {
                "Internal drive. A cartridge has to be something you can unplug."
            }
        }
    }
}

pub struct Volume {
    /// `E:\` — the path everything on the cartridge is relative to.
    pub root: PathBuf,
    /// The volume label, or an empty string when it has none. This is a
    /// cartridge's name — the one the installer can change, and the only piece
    /// of a cartridge that lives outside its file layout.
    pub label: String,
    /// `NTFS`, `exFAT`, `FAT32` — read for [`Volume::maxLabelLen`], since what
    /// a label may contain is a property of the filesystem and nothing else.
    pub fs: String,
    /// How the disk underneath is attached — "USB", "NVMe", "SATA". Shown so a
    /// refusal names its own reason instead of being a mystery.
    pub bus: &'static str,
    pub eligibility: Eligibility,
    pub free_bytes: u64,
    pub total_bytes: u64,
    /// Already carries a launcher **we verified** — routes to edit mode rather
    /// than create. Checked by the same call the listener makes, not by the
    /// file's presence: the edit path goes on to read what it found, and the
    /// picker vouches for the drive on screen. An unsigned or foreign
    /// `launcher.exe` means create mode, which is the honest answer.
    pub is_cartridge: bool,

    /// The launcher version its signature states, when it is one of ours. Read
    /// from the signed comment, never by running the file — see
    /// `../version.rs`.
    pub launcher_version: Option<String>,
}

impl Volume {
    pub fn allowed(&self) -> bool {
        self.eligibility.allowed()
    }

    /// `E:\ — ROMZETA (57.2 GB free of 119 GB)`, the one line the picker shows.
    pub fn summary(&self) -> String {
        let mut text = self.root.display().to_string();
        if !self.label.is_empty() {
            text.push_str(" — ");
            text.push_str(&self.label);
        }
        if self.total_bytes > 0 {
            text.push_str(&format!(
                " ({} free of {})",
                humanBytes(self.free_bytes),
                humanBytes(self.total_bytes)
            ));
        }
        text
    }

    /// How long a name this drive will accept — see [`maxLabelLen`].
    pub fn maxLabelLen(&self) -> usize {
        maxLabelLen(&self.fs)
    }
}

/// The longest label the filesystem allows, in characters.
///
/// NTFS and ReFS take 32; the FAT family — FAT, FAT32 and exFAT alike — takes
/// 11. An unrecognised filesystem is given the larger limit rather than the
/// smaller one: this check exists to catch a too-long name before the user sits
/// through a copy, and Windows itself is the authority at the moment of writing.
/// Guessing low would refuse names that would have worked.
pub fn maxLabelLen(fs: &str) -> usize {
    if isFat(fs) { 11 } else { 32 }
}

fn isExfat(fs: &str) -> bool {
    fs.eq_ignore_ascii_case("exFAT")
}

fn isFat(fs: &str) -> bool {
    isExfat(fs) || fs.to_ascii_uppercase().starts_with("FAT")
}

/// Characters this filesystem will not put in a label.
///
/// Three different answers, and collapsing them would cost real names. NTFS and
/// ReFS take essentially anything. **FAT and FAT32** are the strict ones, down
/// to refusing a full stop. **exFAT** shares their 11-character limit but not
/// their character rules — it takes whatever a filename can hold — so lumping it
/// in with FAT32 would refuse `V1.0` on a drive that would have accepted it.
fn forbidden(fs: &str) -> &'static [char] {
    const FAT: [char; 16] = [
        '*', '?', '/', '\\', '|', '.', ',', ';', ':', '+', '=', '[', ']', '<', '>', '"',
    ];
    const EXFAT: [char; 9] = ['*', '?', '/', '\\', '|', ':', '<', '>', '"'];
    if isExfat(fs) {
        &EXFAT
    } else if isFat(fs) {
        &FAT
    } else {
        &[]
    }
}

/// Whether `name` can be this filesystem's volume label. An empty name is
/// valid and means "no label".
///
/// Checked here as well as at the write, because the write happens at the end
/// of a job that may have spent minutes copying — a name that was never going
/// to land should stop the Review button, not the last step.
pub fn validateLabel(name: &str, fs: &str) -> Result<(), String> {
    let limit = maxLabelLen(fs);
    let length = name.chars().count();
    if length > limit {
        let filesystem = if fs.is_empty() { "this drive" } else { fs };
        return Err(format!(
            "The name is {length} characters. {filesystem} allows at most {limit}."
        ));
    }
    if let Some(bad) = name.chars().find(|c| c.is_control()) {
        return Err(format!("The name cannot contain {bad:?}."));
    }
    if let Some(bad) = name.chars().find(|c| forbidden(fs).contains(c)) {
        return Err(format!("A {fs} drive's name cannot contain {bad:?}."));
    }
    Ok(())
}

/// Renames a drive — the one piece of a cartridge that is not a file on it.
///
/// An empty `name` clears the label. Nothing else about the volume is touched:
/// this sets a string, and is not a format.
pub fn setLabel(root: &Path, name: &str) -> Result<(), String> {
    platform::setLabel(root, name)
}

/// The drives a cartridge can be made on, in drive-letter order.
///
/// Refused drives are **dropped**, not returned greyed out. The picker used to
/// list them with the reason beside them, on the grounds that a filter which
/// silently shortens a list reads as a bug — but on a normal PC that fills the
/// screen with `C:` and every internal disk, rows that exist only to say no. The
/// explanation moved into one line of prose on the picker itself, which answers
/// "why isn't my D: drive here?" once instead of once per drive.
///
/// [`all`] is the unfiltered view, for the tests that check a drive is refused
/// for the right reason rather than merely absent.
pub fn list() -> Vec<Volume> {
    let mut volumes: Vec<Volume> = all().into_iter().filter(Volume::allowed).collect();
    volumes.sort_by(|a, b| a.root.cmp(&b.root));
    volumes
}

/// Every mounted drive, refused ones included, each carrying the
/// [`Eligibility`] that decided it.
pub(crate) fn all() -> Vec<Volume> {
    platform::list()
}

/// Rounded to three significant-ish digits, in the units a drive is sold in
/// (powers of 1000), so the number matches the one on the box and in Explorer's
/// properties dialog.
pub fn humanBytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit + 1 < UNITS.len() {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value < 10.0 {
        format!("{value:.2} {}", UNITS[unit])
    } else if value < 100.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

/// True when `root` is the drive Windows is installed on.
///
/// Compared as a drive letter, because that is all a drive root is, and asked of
/// `%SystemRoot%` rather than hardcoded to `C:` — Windows on another letter is
/// rare but real, and a hardcoded `C:` would be wrong in the dangerous
/// direction there.
///
/// Three variables are consulted and **any** match vetoes. They can each be
/// missing or tampered with, and requiring only one to match means no single
/// absent variable can quietly switch the veto off.
pub fn isSystemDrive(root: &Path) -> bool {
    let Some(letter) = driveLetter(root) else {
        return false;
    };
    ["SystemRoot", "windir", "SystemDrive"]
        .iter()
        .filter_map(std::env::var_os)
        .any(|value| driveLetter(Path::new(&value)) == Some(letter))
}

/// The uppercase drive letter of a path — `e:\games` → `Some('E')`.
pub(crate) fn driveLetter(path: &Path) -> Option<char> {
    let text = path.to_string_lossy();
    let mut chars = text.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    (letter.is_ascii_alphabetic() && chars.next() == Some(':')).then_some(letter)
}

#[cfg(windows)]
mod platform {
    use super::{Eligibility, Volume, isSystemDrive};
    use std::path::PathBuf;
    use std::ptr;

    use common::utf16::{fromWide, wide};

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, ERROR_INVALID_NAME, ERROR_LABEL_TOO_LONG,
        ERROR_WRITE_PROTECT, GetLastError, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        BusType1394, BusTypeAta, BusTypeAtapi, BusTypeFibre, BusTypeFileBackedVirtual, BusTypeMmc,
        BusTypeNvme, BusTypeRAID, BusTypeSCM, BusTypeSas, BusTypeSata, BusTypeScsi, BusTypeSd,
        BusTypeSpaces, BusTypeUfs, BusTypeUsb, BusTypeVirtual, CreateFileW, FILE_SHARE_READ,
        FILE_SHARE_WRITE, GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives,
        GetVolumeInformationW, OPEN_EXISTING, STORAGE_BUS_TYPE, SetVolumeLabelW,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::Ioctl::{
        IOCTL_STORAGE_QUERY_PROPERTY, PropertyStandardQuery, STORAGE_DEVICE_DESCRIPTOR,
        STORAGE_PROPERTY_QUERY, StorageDeviceProperty,
    };
    use windows_sys::Win32::System::WindowsProgramming::{DRIVE_FIXED, DRIVE_REMOVABLE};

    /// The buses that mean "the user can unplug this".
    const EXTERNAL: [STORAGE_BUS_TYPE; 4] = [BusTypeUsb, BusType1394, BusTypeSd, BusTypeMmc];

    pub fn list() -> Vec<Volume> {
        let mask = unsafe { GetLogicalDrives() };
        (0..26)
            .filter(|bit| mask & (1 << bit) != 0)
            .map(|bit| (b'A' + bit as u8) as char)
            .filter_map(describe)
            .collect()
    }

    fn describe(letter: char) -> Option<Volume> {
        let root = format!("{letter}:\\");
        let wide_root = wide(&root);

        // Network shares, optical drives and RAM disks never appear at all —
        // they cannot be cartridges, and probing a stale network mount can block
        // for a long time. Unlike an internal disk there is nothing useful to say
        // about them, so they are dropped rather than listed as refused.
        let drive_type = unsafe { GetDriveTypeW(wide_root.as_ptr()) };
        if !matches!(drive_type, DRIVE_FIXED | DRIVE_REMOVABLE) {
            return None;
        }

        // An empty card reader has a drive letter and no volume behind it.
        // Every call below fails on one, and a row saying "0 B free" for a slot
        // with nothing in it is worse than no row at all.
        let (free_bytes, total_bytes) = freeSpace(&wide_root)?;
        let root = PathBuf::from(&root);

        let bus = busType(letter);
        let eligibility = if isSystemDrive(&root) {
            // First, and unconditional. A Windows-To-Go stick sits on a USB bus
            // and must still be refused: what makes this drive unusable is what
            // is installed on it, not how it is attached.
            Eligibility::SystemDrive
        } else if isExternal(bus, drive_type) {
            Eligibility::Allowed
        } else {
            Eligibility::Internal
        };

        let (label, fs) = volumeInfo(&wide_root);
        let launcher_version = super::attestedLauncher(&root);
        Some(Volume {
            label,
            fs,
            bus: busName(bus, drive_type),
            eligibility,
            free_bytes,
            total_bytes,
            is_cartridge: launcher_version.is_some(),
            launcher_version,
            root,
        })
    }

    /// Whether the disk under this volume is attached in a way the user can
    /// unplug.
    ///
    /// `None` means the bus query failed — an unusual driver, or a volume that
    /// doesn't map onto one physical disk. The fallback is Windows' own coarse
    /// answer, taken in the conservative direction: only a drive Windows itself
    /// calls *removable* gets through, so an unidentifiable fixed disk is refused
    /// rather than guessed at.
    fn isExternal(bus: Option<STORAGE_BUS_TYPE>, drive_type: u32) -> bool {
        match bus {
            Some(bus) => EXTERNAL.contains(&bus),
            None => drive_type == DRIVE_REMOVABLE,
        }
    }

    /// The bus type of the physical disk under `letter:`.
    ///
    /// Opened with **zero** desired access — enough to send a query IOCTL, and
    /// the reason this needs no administrator. Asking for `GENERIC_READ` here
    /// would put a UAC wall in front of the whole drive picker.
    fn busType(letter: char) -> Option<STORAGE_BUS_TYPE> {
        // `\\.\E:` — the volume device, with no trailing backslash.
        let path = wide(&format!(r"\\.\{letter}:"));
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null(),
                OPEN_EXISTING,
                0,
                ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return None;
        }

        let query = STORAGE_PROPERTY_QUERY {
            PropertyId: StorageDeviceProperty,
            QueryType: PropertyStandardQuery,
            AdditionalParameters: [0],
        };
        // The descriptor is variable-length: it ends with vendor and product
        // strings that its own offsets point into. Only one fixed field near the
        // front is wanted, but the call still needs somewhere to put the rest.
        let mut buffer = [0u8; 1024];
        let mut returned = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                handle,
                IOCTL_STORAGE_QUERY_PROPERTY,
                &query as *const _ as *const _,
                size_of::<STORAGE_PROPERTY_QUERY>() as u32,
                buffer.as_mut_ptr() as *mut _,
                buffer.len() as u32,
                &mut returned,
                ptr::null_mut(),
            )
        };
        unsafe { CloseHandle(handle) };

        if ok == 0 || (returned as usize) < size_of::<STORAGE_DEVICE_DESCRIPTOR>() {
            return None;
        }
        // Read through a raw pointer rather than a reference: a byte array
        // carries no alignment guarantee for the struct.
        let descriptor =
            unsafe { ptr::read_unaligned(buffer.as_ptr() as *const STORAGE_DEVICE_DESCRIPTOR) };
        Some(descriptor.BusType)
    }

    /// A short name for the bus, for the line explaining a refusal.
    fn busName(bus: Option<STORAGE_BUS_TYPE>, drive_type: u32) -> &'static str {
        let Some(bus) = bus else {
            return if drive_type == DRIVE_REMOVABLE {
                "removable"
            } else {
                "unknown"
            };
        };
        match bus {
            b if b == BusTypeUsb => "USB",
            b if b == BusType1394 => "FireWire",
            b if b == BusTypeSd => "SD",
            b if b == BusTypeMmc => "MMC",
            b if b == BusTypeNvme => "NVMe",
            b if b == BusTypeSata => "SATA",
            b if b == BusTypeAta || b == BusTypeAtapi => "ATA",
            b if b == BusTypeSas => "SAS",
            b if b == BusTypeScsi => "SCSI",
            b if b == BusTypeRAID => "RAID",
            b if b == BusTypeFibre => "Fibre Channel",
            b if b == BusTypeSpaces => "Storage Spaces",
            b if b == BusTypeVirtual || b == BusTypeFileBackedVirtual => "virtual",
            b if b == BusTypeUfs => "UFS",
            b if b == BusTypeSCM => "SCM",
            _ => "unknown",
        }
    }

    fn freeSpace(root: &[u16]) -> Option<(u64, u64)> {
        let mut free = 0u64;
        let mut total = 0u64;
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                root.as_ptr(),
                // The first out-parameter is the space available *to this user*,
                // which is what a copy will actually be allowed to use when a
                // disk quota is in force. The second is the volume total.
                &mut free,
                &mut total,
                ptr::null_mut(),
            )
        };
        (ok != 0).then_some((free, total))
    }

    /// The volume's label and its filesystem name, both from the one call that
    /// knows them. The filesystem is read because it, and nothing else, decides
    /// what a label may be — see [`super::validateLabel`].
    fn volumeInfo(root: &[u16]) -> (String, String) {
        let mut label = [0u16; 261]; // MAX_PATH + 1, the documented size
        let mut fs = [0u16; 261];
        let ok = unsafe {
            GetVolumeInformationW(
                root.as_ptr(),
                label.as_mut_ptr(),
                label.len() as u32,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                fs.as_mut_ptr(),
                fs.len() as u32,
            )
        };
        if ok == 0 {
            return (String::new(), String::new());
        }
        (fromWide(&label), fromWide(&fs))
    }

    pub fn setLabel(root: &std::path::Path, name: &str) -> Result<(), String> {
        // `SetVolumeLabelW` wants the root *with* its trailing backslash, which
        // is the shape `Volume::root` already holds.
        let root = wide(&root.display().to_string());
        // Deleting a label is a *null* name, not an empty one — that is the
        // documented way to clear it, and the only one guaranteed to across
        // filesystems.
        let label = wide(name);
        let name = if name.is_empty() {
            ptr::null()
        } else {
            label.as_ptr()
        };
        let ok = unsafe { SetVolumeLabelW(root.as_ptr(), name) };
        if ok != 0 {
            return Ok(());
        }
        let code = unsafe { GetLastError() };
        Err(match code {
            ERROR_ACCESS_DENIED => "the drive would not allow it".into(),
            ERROR_LABEL_TOO_LONG => "the name is too long for this drive".into(),
            ERROR_INVALID_NAME => "the name has a character this drive won't take".into(),
            ERROR_WRITE_PROTECT => "the drive is write-protected".into(),
            other => format!("Windows error {other}"),
        })
    }
}

#[cfg(not(windows))]
mod platform {
    use super::Volume;

    /// Volume enumeration is Windows-only in v1. The Linux shape — mount points
    /// under `/media` and `/run/media`, which are already the removable ones —
    /// is sketched in `../structure.md` under "Future".
    pub fn list() -> Vec<Volume> {
        Vec::new()
    }

    pub fn setLabel(_root: &std::path::Path, _name: &str) -> Result<(), String> {
        Err("renaming a drive is Windows-only".into())
    }
}
