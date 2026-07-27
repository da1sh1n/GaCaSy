// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Moving a game folder onto a cartridge: chunked, measured, and interruptible.
//!
//! Game installs run to many gigabytes, so this is the one operation in the
//! installer with a real duration. Two consequences shape it:
//!
//! * It runs on a worker thread and reports bytes back as it goes, so the UI
//!   stays responsive and the progress bar means something.
//! * It copies in chunks rather than calling `fs::copy`, because `fs::copy` on a
//!   40 GB pak file is one uninterruptible call — cancel would appear to do
//!   nothing for several minutes. The chunk loop is slightly slower than the
//!   OS-level copy and buys a cancel that responds within one chunk.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Read/write unit. Large enough that the syscall overhead is irrelevant next to
/// the disk, small enough that cancel is felt immediately even on slow USB.
const CHUNK_BYTES: usize = 1024 * 1024;

pub enum Error {
    /// The user pressed cancel. Not a failure — the caller unwinds and says so.
    Cancelled,
    Io { path: PathBuf, source: io::Error },
}

impl Error {
    fn at(path: &Path, source: io::Error) -> Self {
        Error::Io {
            path: path.to_path_buf(),
            source,
        }
    }

    pub fn message(&self) -> String {
        match self {
            Error::Cancelled => "cancelled".into(),
            Error::Io { path, source } => format!("{}: {source}", path.display()),
        }
    }
}

/// Copies `src` into `dst`, creating it, calling `progress` with the file being
/// worked on and the bytes finished since the last call.
///
/// Checks `cancel` between chunks and between files, so a cancel is honoured
/// within roughly one chunk of the moment it is pressed.
pub fn directory(
    src: &Path,
    dst: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(&Path, u64),
) -> Result<(), Error> {
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    fs::create_dir_all(dst).map_err(|e| Error::at(dst, e))?;

    let entries = fs::read_dir(src).map_err(|e| Error::at(src, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| Error::at(src, e))?;
        let kind = entry.file_type().map_err(|e| Error::at(&entry.path(), e))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());

        // Not followed, matching the scan that measured the folder: following a
        // link would copy bytes the size estimate never counted, and a loop
        // would never finish.
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            directory(&from, &to, cancel, progress)?;
        } else {
            file(&from, &to, cancel, progress)?;
        }
    }
    Ok(())
}

fn file(
    src: &Path,
    dst: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(&Path, u64),
) -> Result<(), Error> {
    let mut source = File::open(src).map_err(|e| Error::at(src, e))?;
    let mut target = File::create(dst).map_err(|e| Error::at(dst, e))?;
    let mut buffer = vec![0u8; CHUNK_BYTES];

    loop {
        if cancel.load(Ordering::Relaxed) {
            // The half-written file is left for the caller's rollback to remove
            // along with the rest of this game's folder. Deleting it here would
            // only clean up the last of many.
            return Err(Error::Cancelled);
        }
        let read = source.read(&mut buffer).map_err(|e| Error::at(src, e))?;
        if read == 0 {
            break;
        }
        target
            .write_all(&buffer[..read])
            .map_err(|e| Error::at(dst, e))?;
        progress(src, read as u64);
    }
    // Explicit, so a full disk surfaces here as an error on this file rather
    // than silently at drop time.
    target.flush().map_err(|e| Error::at(dst, e))?;
    Ok(())
}

/// Writes one embedded payload file. Same error shape as the rest of the copy,
/// so a failure to write `launcher.exe` reads like any other.
pub fn bytes(dst: &Path, contents: &[u8]) -> Result<(), Error> {
    fs::write(dst, contents).map_err(|e| Error::at(dst, e))
}

/// Copies a single file (a cover image), reported as one step.
pub fn single(src: &Path, dst: &Path, cancel: &AtomicBool) -> Result<(), Error> {
    file(src, dst, cancel, &mut |_, _| {})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("gacasy-copy").join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn copies_a_tree_and_reports_every_byte() {
        let base = temp("tree");
        let src = base.join("src");
        fs::create_dir_all(src.join("data")).expect("mkdir");
        fs::write(src.join("game.exe"), vec![7u8; 3000]).expect("write");
        fs::write(src.join("data/pak0.bin"), vec![9u8; 5000]).expect("write");

        let dst = base.join("dst");
        let mut reported = 0u64;
        directory(&src, &dst, &AtomicBool::new(false), &mut |_, n| {
            reported += n
        })
        .unwrap_or_else(|e| panic!("{}", e.message()));

        assert_eq!(reported, 8000);
        assert_eq!(fs::read(dst.join("game.exe")).expect("copied").len(), 3000);
        assert_eq!(
            fs::read(dst.join("data/pak0.bin")).expect("copied").len(),
            5000
        );
    }

    #[test]
    fn cancel_stops_it() {
        let base = temp("cancel");
        let src = base.join("src");
        fs::create_dir_all(&src).expect("mkdir");
        fs::write(src.join("game.exe"), vec![7u8; 3000]).expect("write");

        let result = directory(
            &src,
            &base.join("dst"),
            &AtomicBool::new(true),
            &mut |_, _| {},
        );
        assert!(matches!(result, Err(Error::Cancelled)));
    }

    #[test]
    fn a_missing_source_names_the_path_that_failed() {
        let base = temp("missing");
        let result = directory(
            &base.join("nope"),
            &base.join("dst"),
            &AtomicBool::new(false),
            &mut |_, _| {},
        );
        match result {
            Err(Error::Io { path, .. }) => assert!(path.ends_with("nope")),
            _ => panic!("expected an io error naming the source"),
        }
    }
}
