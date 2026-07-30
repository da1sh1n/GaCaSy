// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Which drives may become a cartridge.
//!
//! **External storage only.** The drive Windows is installed on is refused
//! outright, and so is every internal disk. A cartridge is a thing you unplug
//! and carry to another PC, and making one means copying many gigabytes onto a
//! volume and — in edit mode — deleting folders from it. Pointing that at `C:\`
//! is not a supported choice that happens to be unwise; it is not offered.
//!
//! This is deliberately **stricter than the listener**, which starts a correctly
//! signed launcher from any volume it sees (`../../listener/src/volume.rs`). The
//! asymmetry is the right way round: the listener decides whether to *run*
//! something already on a disk, while this decides where to *write* gigabytes. A
//! cartridge someone hand-assembles on an internal disk still works — the
//! installer just won't make one there.
//!
//! ## What counts as external
//!
//! Not `DRIVE_REMOVABLE`, which is the tempting answer and the wrong one: a USB
//! SSD in an enclosure — precisely the drive a game cartridge wants to be —
//! reports as `DRIVE_FIXED`, exactly like the disk Windows is installed on.
//! Filtering on it would reject the best candidates and admit none of the ones
//! it was meant to catch.
//!
//! So the question is asked of the hardware instead:
//! `IOCTL_STORAGE_QUERY_PROPERTY` for the underlying disk's **bus type**. USB,
//! FireWire, SD and MMC are external; SATA, NVMe, SAS, RAID and the rest are not.
//! The query wants a handle opened with *zero* access rights, which is why none
//! of this needs administrator.
//!
//! Two consequences worth knowing:
//!
//! * A **Thunderbolt/PCIe** enclosure presents its disk as `BusTypeNvme`,
//!   indistinguishable from an internal one, so it is treated as internal and
//!   refused. That is the conservative direction to be wrong in: the cost is a
//!   drive you can't pick, not a system disk you can.
//! * **Windows-To-Go** on a USB stick passes the bus test, so the system-drive
//!   check runs first, separately, and wins.
//!
//! ## Naming
//!
//! A cartridge's name is its **volume label**, and [`set_label`] is the one
//! thing in this module that writes. There is nowhere else for a name to live:
//! `config.toml` is look and feel, `catalog.json` is the game list, and identity
//! is a signature inside `launcher.exe`. So the label is the whole of it — read
//! by [`Volume::summary`], set at the end of a write.
//!
//! Nothing here formats, partitions or erases anything. The installer writes
//! files to a volume, and a name onto it; it never repartitions one.

use std::path::{Path, PathBuf};

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
    /// `NTFS`, `exFAT`, `FAT32` — read for [`Volume::max_label_len`], since what
    /// a label may contain is a property of the filesystem and nothing else.
    pub fs: String,
    /// How the disk underneath is attached — "USB", "NVMe", "SATA". Shown so a
    /// refusal names its own reason instead of being a mystery.
    pub bus: &'static str,
    pub eligibility: Eligibility,
    pub free_bytes: u64,
    pub total_bytes: u64,
    /// Already carries a launcher — routes to edit mode instead of create.
    ///
    /// Presence only; the signature is not checked here. This picks which
    /// *screen* to show, and getting it wrong costs a wrong starting catalog,
    /// not a wrong trust decision — that one belongs to the listener, which
    /// verifies properly. Treating an unsigned launcher as "not a cartridge"
    /// would route someone's own broken cartridge into create mode and quietly
    /// drop the games already on it.
    pub is_cartridge: bool,
}

impl Volume {
    pub fn allowed(&self) -> bool {
        self.eligibility.allowed()
    }

    /// `E:\ — GACASY (57.2 GB free of 119 GB)`, the one line the picker shows.
    pub fn summary(&self) -> String {
        let mut text = self.root.display().to_string();
        if !self.label.is_empty() {
            text.push_str(" — ");
            text.push_str(&self.label);
        }
        if self.total_bytes > 0 {
            text.push_str(&format!(
                " ({} free of {})",
                human_bytes(self.free_bytes),
                human_bytes(self.total_bytes)
            ));
        }
        text
    }

    /// How long a name this drive will accept — see [`max_label_len`].
    pub fn max_label_len(&self) -> usize {
        max_label_len(&self.fs)
    }
}

/// The longest label the filesystem allows, in characters.
///
/// NTFS and ReFS take 32; the FAT family — FAT, FAT32 and exFAT alike — takes
/// 11. An unrecognised filesystem is given the larger limit rather than the
/// smaller one: this check exists to catch a too-long name before the user sits
/// through a copy, and Windows itself is the authority at the moment of writing.
/// Guessing low would refuse names that would have worked.
pub fn max_label_len(fs: &str) -> usize {
    if is_fat(fs) { 11 } else { 32 }
}

fn is_exfat(fs: &str) -> bool {
    fs.eq_ignore_ascii_case("exFAT")
}

fn is_fat(fs: &str) -> bool {
    is_exfat(fs) || fs.to_ascii_uppercase().starts_with("FAT")
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
    if is_exfat(fs) {
        &EXFAT
    } else if is_fat(fs) {
        &FAT
    } else {
        &[]
    }
}

/// Whether `name` can be this filesystem's volume label.
///
/// An **empty name is valid** and means "no label" — clearing a cartridge's name
/// is as legitimate as setting one.
///
/// Checked here rather than only at the write because the write happens at the
/// end of a job that may have spent minutes copying: a name that was never going
/// to land should stop the Review button, not the last step of the copy.
pub fn validate_label(name: &str, fs: &str) -> Result<(), String> {
    let limit = max_label_len(fs);
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
pub fn set_label(root: &Path, name: &str) -> Result<(), String> {
    platform::set_label(root, name)
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
pub fn human_bytes(bytes: u64) -> String {
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
pub fn is_system_drive(root: &Path) -> bool {
    let Some(letter) = drive_letter(root) else {
        return false;
    };
    ["SystemRoot", "windir", "SystemDrive"]
        .iter()
        .filter_map(std::env::var_os)
        .any(|value| drive_letter(Path::new(&value)) == Some(letter))
}

/// The uppercase drive letter of a path — `e:\games` → `Some('E')`.
pub(crate) fn drive_letter(path: &Path) -> Option<char> {
    let text = path.to_string_lossy();
    let mut chars = text.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    (letter.is_ascii_alphabetic() && chars.next() == Some(':')).then_some(letter)
}

#[cfg(windows)]
mod platform {
    use super::{Eligibility, Volume, is_system_drive};
    use std::path::PathBuf;
    use std::ptr;

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
        let (free_bytes, total_bytes) = free_space(&wide_root)?;
        let root = PathBuf::from(&root);

        let bus = bus_type(letter);
        let eligibility = if is_system_drive(&root) {
            // First, and unconditional. A Windows-To-Go stick sits on a USB bus
            // and must still be refused: what makes this drive unusable is what
            // is installed on it, not how it is attached.
            Eligibility::SystemDrive
        } else if is_external(bus, drive_type) {
            Eligibility::Allowed
        } else {
            Eligibility::Internal
        };

        let (label, fs) = volume_info(&wide_root);
        Some(Volume {
            label,
            fs,
            bus: bus_name(bus, drive_type),
            eligibility,
            free_bytes,
            total_bytes,
            is_cartridge: root.join(crate::cartridge::LAUNCHER_NAME).is_file(),
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
    fn is_external(bus: Option<STORAGE_BUS_TYPE>, drive_type: u32) -> bool {
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
    fn bus_type(letter: char) -> Option<STORAGE_BUS_TYPE> {
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
    fn bus_name(bus: Option<STORAGE_BUS_TYPE>, drive_type: u32) -> &'static str {
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

    fn free_space(root: &[u16]) -> Option<(u64, u64)> {
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
    /// what a label may be — see [`super::validate_label`].
    fn volume_info(root: &[u16]) -> (String, String) {
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
        (from_wide(&label), from_wide(&fs))
    }

    pub fn set_label(root: &std::path::Path, name: &str) -> Result<(), String> {
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

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// A NUL-terminated wide buffer as a `String`.
    fn from_wide(buffer: &[u16]) -> String {
        let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..end])
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

    pub fn set_label(_root: &std::path::Path, _name: &str) -> Result<(), String> {
        Err("renaming a drive is Windows-only".into())
    }
}
