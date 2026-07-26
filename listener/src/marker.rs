// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! The `.cartridge` marker — the cartridge half of the identity contract
//! (see `../structure.md`, "Cartridge identification system").
//!
//! TOML at the volume root, written by the installer:
//!
//! ```toml
//! version = 1
//! key = "3f9a1c…"
//! launcher = "launcher.exe"
//! ```
//!
//! The launcher itself never reads or writes this file; it is purely the thing
//! that gets started.

use std::fmt;
use std::fs;
use std::path::Path;

pub const MARKER_FILE: &str = ".cartridge";

/// The only marker format this listener understands. A cartridge written by a
/// future installer is refused rather than guessed at — see [`Error::Version`].
const SUPPORTED_VERSION: i64 = 1;

pub struct Marker {
    pub key: String,
    /// The binary to start, relative to the volume root. Read from the file
    /// rather than hardcoded, which is what lets a Linux cartridge name a
    /// different binary with no listener change.
    pub launcher: String,
}

pub enum Error {
    /// No `.cartridge` at the volume root — an ordinary drive, not a
    /// cartridge. By far the most common outcome and not a problem.
    Missing,
    Unreadable(String),
    Malformed(String),
    /// A required field is absent or the wrong type.
    Field(&'static str),
    /// The marker declares a format version this build predates.
    Version(i64),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Missing => write!(f, "no {MARKER_FILE} at the volume root"),
            Error::Unreadable(e) => write!(f, "{MARKER_FILE} could not be read: {e}"),
            Error::Malformed(e) => write!(f, "{MARKER_FILE} is not valid TOML: {e}"),
            Error::Field(name) => write!(f, "{MARKER_FILE} has no usable `{name}`"),
            Error::Version(v) => write!(
                f,
                "{MARKER_FILE} is version {v}, this listener understands {SUPPORTED_VERSION}"
            ),
        }
    }
}

/// Reads and validates `<root>/.cartridge`.
///
/// Unlike the listener's own config.toml this is *not* parsed forgivingly: a
/// marker missing its key or naming no launcher describes no cartridge worth
/// launching, so it fails as a whole and the volume is ignored with a reason.
pub fn read(root: &Path) -> Result<Marker, Error> {
    let path = root.join(MARKER_FILE);
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Error::Missing),
        Err(e) => return Err(Error::Unreadable(e.to_string())),
    };

    let table = contents
        .parse::<toml::Table>()
        .map_err(|e| Error::Malformed(e.to_string()))?;

    // Version first: on a marker from a newer format, every field below could
    // mean something else, so nothing else is worth reading.
    let version = table
        .get("version")
        .and_then(|v| v.as_integer())
        .ok_or(Error::Field("version"))?;
    if version != SUPPORTED_VERSION {
        return Err(Error::Version(version));
    }

    let key = non_empty(&table, "key")?;
    let launcher = non_empty(&table, "launcher")?;

    Ok(Marker { key, launcher })
}

fn non_empty(table: &toml::Table, field: &'static str) -> Result<String, Error> {
    table
        .get(field)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or(Error::Field(field))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Writes `contents` as a marker in a fresh temp folder and reads it back.
    fn read_marker(name: &str, contents: &str) -> Result<Marker, Error> {
        let dir = std::env::temp_dir().join(format!("gacasy-marker-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join(MARKER_FILE), contents).expect("write marker");
        read(&dir)
    }

    #[test]
    fn reads_a_well_formed_marker() {
        let marker = read_marker(
            "ok",
            "version = 1\nkey = \" 3F9A1C \"\nlauncher = \"launcher.exe\"\n",
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(marker.key, "3F9A1C"); // trimmed, case preserved
        assert_eq!(marker.launcher, "launcher.exe");
    }

    #[test]
    fn missing_file_is_not_an_error_worth_shouting_about() {
        let dir = std::env::temp_dir().join("gacasy-marker-absent");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        assert!(matches!(read(&dir), Err(Error::Missing)));
    }

    #[test]
    fn rejects_incomplete_and_future_markers() {
        assert!(matches!(
            read_marker("nokey", "version = 1\nlauncher = \"l.exe\"\n"),
            Err(Error::Field("key"))
        ));
        assert!(matches!(
            read_marker(
                "blankkey",
                "version = 1\nkey = \"  \"\nlauncher = \"l.exe\"\n"
            ),
            Err(Error::Field("key"))
        ));
        assert!(matches!(
            read_marker("nolauncher", "version = 1\nkey = \"a\"\n"),
            Err(Error::Field("launcher"))
        ));
        assert!(matches!(
            read_marker("v2", "version = 2\nkey = \"a\"\nlauncher = \"l.exe\"\n"),
            Err(Error::Version(2))
        ));
        assert!(matches!(
            read_marker("junk", "this is not toml"),
            Err(Error::Malformed(_))
        ));
    }
}
