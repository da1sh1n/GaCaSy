// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Resolves the desktop's UI font through the registry, reads the face file off
//! disk, and builds the egui font definitions. Falls back to Ubuntu-Light.

// ########## THE SYSTEM UI FONT ##########

use std::path::PathBuf;
use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

/// Keys into [`FontDefinitions::font_data`]. Names, not faces — what the desktop
/// is actually using is decided at runtime and is nobody's business here.
pub(crate) const SYSTEM: &str = "system-ui";
pub(crate) const FALLBACK: &str = "ubuntu-light";

/// What to hand [`egui::Context::set_fonts`], before the first frame.
pub fn definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::empty();
    let mut chain = Vec::new();

    if let Some(bytes) = systemUiFont() {
        // Face 0 of the file. A `.ttc` holds several — Yu Gothic UI's holds
        // Regular, Light and Semilight — and the first is the regular weight in
        // every collection Windows ships as a UI font.
        fonts
            .font_data
            .insert(SYSTEM.to_owned(), Arc::new(FontData::from_owned(bytes)));
        chain.push(SYSTEM.to_owned());
    }
    fonts.font_data.insert(
        FALLBACK.to_owned(),
        Arc::new(FontData::from_static(epaint_default_fonts::UBUNTU_LIGHT)),
    );
    chain.push(FALLBACK.to_owned());

    // Both families, even though nothing in this program asks for monospace:
    // epaint panics the first time a family with no fonts behind it is used, and
    // it does that lazily, so an empty Monospace is a crash waiting for whichever
    // egui internal reaches for one.
    fonts
        .families
        .insert(FontFamily::Proportional, chain.clone());
    fonts.families.insert(FontFamily::Monospace, chain);
    fonts
}

/// The bytes of the desktop's UI font, or `None` if anything at all went wrong.
///
/// Every failure here is the same failure — we don't get to use the system font —
/// so none of them is reported. The fallback is not a degraded mode worth warning
/// about; it is a different typeface.
#[cfg(windows)]
fn systemUiFont() -> Option<Vec<u8>> {
    let bytes = std::fs::read(faceFile(&messageFontFace()?)?).ok()?;
    looksLikeAFont(&bytes).then_some(bytes)
}

#[cfg(not(windows))]
fn systemUiFont() -> Option<Vec<u8>> {
    // No fontconfig, no CoreText. The installer only really runs on Windows; the
    // other targets exist so the non-UI half can be built and tested there.
    None
}

/// The face name Windows uses for dialog text, e.g. `"Segoe UI"`.
///
/// `lfMessageFont` and not `lfCaptionFont`: the caption font is the title bar,
/// and this is a window full of body text. Only the *name* is taken — `lfHeight`
/// is in the system's DPI, and egui does its own scaling, so reading it would
/// mean handling two sizing schemes to end up where we already are.
#[cfg(windows)]
fn messageFontFace() -> Option<String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS, SystemParametersInfoW,
    };

    let size = size_of::<NONCLIENTMETRICSW>() as u32;
    // The struct grew an `iPaddedBorderWidth` in Vista, and the call uses cbSize
    // to decide which version it was handed. Getting it wrong fails the call
    // rather than corrupting anything.
    let mut metrics = NONCLIENTMETRICSW {
        cbSize: size,
        ..Default::default()
    };
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETNONCLIENTMETRICS,
            size,
            std::ptr::from_mut(&mut metrics).cast(),
            0,
        )
    };
    if ok == 0 {
        return None;
    }

    let name = &metrics.lfMessageFont.lfFaceName;
    let end = name.iter().position(|c| *c == 0).unwrap_or(name.len());
    let face = String::from_utf16_lossy(&name[..end]);
    (!face.is_empty()).then_some(face)
}

/// Where the file for a face lives, via the installed-fonts list.
///
/// Machine-wide fonts first, then this account's — that is the order Windows
/// itself resolves them in, and the machine list is where every stock UI font is.
#[cfg(windows)]
fn faceFile(face: &str) -> Option<PathBuf> {
    const FONTS: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts";

    for (root, dir) in [
        (crate::reg::HKEY_LOCAL_MACHINE, machineFontDir()),
        (crate::reg::HKEY_CURRENT_USER, userFontDir()),
    ] {
        let (Some(key), Some(dir)) = (crate::reg::open(root, FONTS, crate::reg::READ), dir) else {
            continue;
        };
        let Some(value) = fontValue(&key, face) else {
            continue;
        };
        // The machine list stores a bare filename; a per-user install stores the
        // whole path, because it isn't in the shared folder.
        let path = PathBuf::from(&value);
        let path = if path.is_absolute() {
            path
        } else {
            dir.join(path)
        };
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

/// The value under the fonts key that holds `face`'s filename.
#[cfg(windows)]
fn fontValue(key: &crate::reg::Key, face: &str) -> Option<String> {
    // One face, one file, named after itself. Every stock UI font is this.
    for suffix in [" (TrueType)", " (OpenType)"] {
        if let Some(file) = crate::reg::querySz(key, Some(&format!("{face}{suffix}"))) {
            return Some(file);
        }
    }

    // A collection is named after everything inside it, so there is no name to
    // ask for: `"Yu Gothic UI Regular & Yu Gothic UI Semilight & … (TrueType)"`.
    // The `&`/`(` check is what keeps "Segoe UI" from matching "Segoe UI Emoji".
    let name = crate::reg::enumValueNames(key).into_iter().find(|name| {
        name.strip_prefix(face)
            .is_some_and(|rest| rest.starts_with(" (") || rest.starts_with(" &"))
    })?;
    crate::reg::querySz(key, Some(&name))
}

#[cfg(windows)]
fn machineFontDir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("WINDIR")?).join("Fonts"))
}

#[cfg(windows)]
fn userFontDir() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join(r"Microsoft\Windows\Fonts"))
}

/// Whether these bytes are worth handing to epaint.
///
/// epaint *panics* on a font it cannot parse, so a truncated or mistyped file in
/// the fonts folder would take the installer down on its first frame. This is not
/// a validation — a well-formed header over corrupt tables still gets through —
/// but it costs four bytes and catches the case where the registry pointed us at
/// something that isn't a font at all.
fn looksLikeAFont(bytes: &[u8]) -> bool {
    bytes.len() > 12
        && matches!(
            bytes.first_chunk::<4>(),
            Some(b"\x00\x01\x00\x00" | b"true" | b"OTTO" | b"ttcf")
        )
}
