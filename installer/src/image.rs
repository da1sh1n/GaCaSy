// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Cover art: reading a picture's size without decoding it.
//!
//! The launcher is built around **600×900 (2:3)** covers
//! (`COVER_NATIVE_WIDTH` / `COVER_NATIVE_HEIGHT` in
//! `../../launcher/src/constants.rs`), and its layout fits covers by their real
//! `naturalWidth`/`naturalHeight` — so a cover of the wrong shape doesn't break
//! anything, it just sits at a different size to its neighbours. v1 therefore
//! **warns and copies the file as-is** rather than resizing it, which is what
//! keeps this module a header parser instead of an image library.
//!
//! Only the first few dozen bytes of the file are read, and only enough of each
//! format to find the dimensions. Anything unrecognised returns `None` and the
//! UI simply doesn't comment on it — a covers-must-be-recognised rule would
//! reject formats the webview renders perfectly well.
//!
//! Renamed files are the norm here, not the exception: cover art is routinely an
//! animated WebP saved as `.png`, so the format is decided by the *bytes* and
//! the extension is never consulted.

use std::fs::File;
use std::io::Read;
use std::path::Path;

/// The shape the launcher's covers are cut to.
const TARGET_RATIO: f64 = 600.0 / 900.0;

/// How far off 2:3 a cover may be before it is worth mentioning. A percent or
/// two is rounding in whatever tool produced the file.
const RATIO_TOLERANCE: f64 = 0.02;

/// Enough for every header this module understands, and for the JPEG segment
/// walk to reach a real SOF marker in practice.
const HEADER_BYTES: usize = 64 * 1024;

/// Pixel dimensions of the image at `path`, or `None` if the format isn't one
/// this recognises.
pub fn dimensions(path: &Path) -> Option<(u32, u32)> {
    let mut header = Vec::new();
    File::open(path)
        .ok()?
        .take(HEADER_BYTES as u64)
        .read_to_end(&mut header)
        .ok()?;
    parse(&header)
}

/// A sentence about this cover's shape, or `None` when there is nothing to say.
///
/// Deliberately a warning and not a rejection: it is the user's cartridge, and
/// the launcher will render whatever they give it.
pub fn ratio_warning(path: &Path) -> Option<String> {
    let (width, height) = dimensions(path)?;
    if width == 0 || height == 0 {
        return None;
    }
    let ratio = width as f64 / height as f64;
    ((ratio - TARGET_RATIO).abs() > TARGET_RATIO * RATIO_TOLERANCE).then(|| {
        format!(
            "{width}×{height} is not the 2:3 shape the launcher lays out (600×900). \
             It will still be shown, at a different size to the others."
        )
    })
}

fn parse(bytes: &[u8]) -> Option<(u32, u32)> {
    png(bytes).or_else(|| webp(bytes)).or_else(|| gif(bytes)).or_else(|| jpeg(bytes))
}

fn png(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if bytes.len() < 24 || bytes[..8] != SIGNATURE || &bytes[12..16] != b"IHDR" {
        return None;
    }
    Some((be32(&bytes[16..20]), be32(&bytes[20..24])))
}

/// RIFF/WEBP, in all three of its chunk shapes.
///
/// `VP8X` is the one that matters most here: it is what an *animated* WebP uses,
/// which is exactly the kind of file this project's covers turn out to be.
fn webp(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 30 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return None;
    }
    match &bytes[12..16] {
        // Extended format: 4 flag bytes, then canvas width-1 and height-1 as
        // 24-bit little-endian.
        b"VP8X" => Some((le24(&bytes[24..27]) + 1, le24(&bytes[27..30]) + 1)),
        // Lossless: a 0x2f signature byte, then 14 bits of width-1 and 14 of
        // height-1 packed little-endian.
        b"VP8L" if bytes[20] == 0x2f => {
            let bits = u32::from_le_bytes([bytes[21], bytes[22], bytes[23], bytes[24]]);
            Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
        }
        // Lossy: a VP8 keyframe, found by its sync code, with 14-bit dimensions
        // after it (the top 2 bits of each are a scaling hint, not size).
        b"VP8 " if bytes[23..26] == [0x9d, 0x01, 0x2a] => Some((
            (le16(&bytes[26..28]) & 0x3fff) as u32,
            (le16(&bytes[28..30]) & 0x3fff) as u32,
        )),
        _ => None,
    }
}

fn gif(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 10 || (&bytes[..6] != b"GIF87a" && &bytes[..6] != b"GIF89a") {
        return None;
    }
    Some((le16(&bytes[6..8]) as u32, le16(&bytes[8..10]) as u32))
}

/// Walks JPEG segments to the first start-of-frame, which is where the size is.
///
/// A JPEG has no size in its header — it is behind a variable number of
/// metadata segments, and a phone photo's EXIF thumbnail can push it a long way
/// in. Anything past the buffer we read is treated as unknown rather than
/// guessed at.
fn jpeg(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 4 || bytes[0] != 0xff || bytes[1] != 0xd8 {
        return None;
    }
    let mut at = 2;
    while at + 9 < bytes.len() {
        if bytes[at] != 0xff {
            at += 1; // resync over padding between segments
            continue;
        }
        let marker = bytes[at + 1];
        // Start-of-frame markers, minus the three in that range that mean
        // something else (DHT, JPG, DAC).
        if (0xc0..=0xcf).contains(&marker) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
            return Some((
                le_be16(&bytes[at + 7..at + 9]),
                le_be16(&bytes[at + 5..at + 7]),
            ));
        }
        // Standalone markers carry no length field to skip over.
        if matches!(marker, 0xd8 | 0xd9) || (0xd0..=0xd7).contains(&marker) {
            at += 2;
            continue;
        }
        let length = le_be16(&bytes[at + 2..at + 4]) as usize;
        if length < 2 {
            return None;
        }
        at += 2 + length;
    }
    None
}

fn be32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn le_be16(bytes: &[u8]) -> u32 {
    u16::from_be_bytes([bytes[0], bytes[1]]) as u32
}

fn le16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

fn le24(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes
    }

    fn riff(chunk: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(b"WEBP");
        bytes.extend_from_slice(chunk);
        bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
        bytes.extend_from_slice(body);
        bytes
    }

    #[test]
    fn reads_a_png() {
        assert_eq!(parse(&png_header(600, 900)), Some((600, 900)));
    }

    #[test]
    fn reads_an_animated_webp() {
        // VP8X — the shape the covers in this project actually are, and the
        // reason the format is sniffed from bytes rather than trusted from the
        // `.png` name they are usually saved under.
        let mut body = vec![0x10, 0, 0, 0]; // flags: has animation
        body.extend_from_slice(&[0x57, 0x02, 0x00]); // 600-1, 24-bit LE
        body.extend_from_slice(&[0x83, 0x03, 0x00]); // 900-1
        assert_eq!(parse(&riff(b"VP8X", &body)), Some((600, 900)));
    }

    #[test]
    fn reads_a_lossless_webp() {
        let bits: u32 = (599) | (899 << 14);
        let mut body = vec![0x2f];
        body.extend_from_slice(&bits.to_le_bytes());
        body.extend_from_slice(&[0; 8]);
        assert_eq!(parse(&riff(b"VP8L", &body)), Some((600, 900)));
    }

    #[test]
    fn reads_a_lossy_webp() {
        let mut body = vec![0x00, 0x00, 0x00, 0x9d, 0x01, 0x2a];
        body.extend_from_slice(&600u16.to_le_bytes());
        body.extend_from_slice(&900u16.to_le_bytes());
        body.extend_from_slice(&[0; 8]);
        assert_eq!(parse(&riff(b"VP8 ", &body)), Some((600, 900)));
    }

    #[test]
    fn reads_a_gif() {
        let mut bytes = b"GIF89a".to_vec();
        bytes.extend_from_slice(&600u16.to_le_bytes());
        bytes.extend_from_slice(&900u16.to_le_bytes());
        assert_eq!(parse(&bytes), Some((600, 900)));
    }

    #[test]
    fn reads_a_jpeg_behind_a_metadata_segment() {
        let mut bytes = vec![0xff, 0xd8];
        // An APP1 (EXIF) segment the size walk has to step over.
        bytes.extend_from_slice(&[0xff, 0xe1, 0x00, 0x10]);
        bytes.extend_from_slice(&[0u8; 14]);
        // SOF0: length, precision, height, width.
        bytes.extend_from_slice(&[0xff, 0xc0, 0x00, 0x11, 0x08]);
        bytes.extend_from_slice(&900u16.to_be_bytes());
        bytes.extend_from_slice(&600u16.to_be_bytes());
        bytes.extend_from_slice(&[0u8; 8]);
        assert_eq!(parse(&bytes), Some((600, 900)));
    }

    #[test]
    fn an_unrecognised_file_is_not_a_failure() {
        assert_eq!(parse(b"this is not a picture at all"), None);
        assert_eq!(parse(&[]), None);
    }

    #[test]
    fn only_a_real_departure_from_2_by_3_is_worth_a_warning() {
        let warn = |w: u32, h: u32| {
            let ratio = w as f64 / h as f64;
            (ratio - TARGET_RATIO).abs() > TARGET_RATIO * RATIO_TOLERANCE
        };
        assert!(!warn(600, 900));
        assert!(!warn(1200, 1800));
        assert!(!warn(602, 900)); // rounding from whatever made the file
        assert!(warn(1920, 1080));
        assert!(warn(900, 900));
    }
}
