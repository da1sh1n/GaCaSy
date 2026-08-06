// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Reads pixel dimensions out of a PNG, WebP, GIF or JPEG header without
//! decoding the image, and phrases the 2:3 shape warning. The format is decided
//! by the bytes, not the extension.

// ########## COVER DIMENSIONS ##########

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

/// A sentence about this cover's shape, or `None` when there is nothing to
/// say. A warning only — the file is copied either way.
pub fn ratioWarning(path: &Path) -> Option<String> {
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

pub(crate) fn parse(bytes: &[u8]) -> Option<(u32, u32)> {
    png(bytes)
        .or_else(|| webp(bytes))
        .or_else(|| gif(bytes))
        .or_else(|| jpeg(bytes))
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
                leBe16(&bytes[at + 7..at + 9]),
                leBe16(&bytes[at + 5..at + 7]),
            ));
        }
        // Standalone markers carry no length field to skip over.
        if matches!(marker, 0xd8 | 0xd9) || (0xd0..=0xd7).contains(&marker) {
            at += 2;
            continue;
        }
        let length = leBe16(&bytes[at + 2..at + 4]) as usize;
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

fn leBe16(bytes: &[u8]) -> u32 {
    u16::from_be_bytes([bytes[0], bytes[1]]) as u32
}

fn le16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

fn le24(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0])
}
