// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Turns a stored id list into a usable cover order: `normalize` repairs a
//! partial, duplicated or out-of-range list into a full permutation, and
//! `promote` moves one id to the front.

// ########## COVER ORDER ##########

/// A stored id list turned into a complete permutation of `0..count`.
///
/// Anything out of range, or repeated, is dropped; then every id the list never
/// mentioned is appended in catalog order. So a list written when the cartridge
/// held three games still works after a fourth is added (the newcomer lands at
/// the end), and an empty list yields plain catalog order.
pub fn normalize(stored: &[usize], count: usize) -> Vec<usize> {
    let mut seen = vec![false; count];
    let mut order = Vec::with_capacity(count);

    for &id in stored {
        // The bounds check and the duplicate check in one: an out-of-range id
        // has no slot to have been seen in.
        if seen.get(id) == Some(&false) {
            seen[id] = true;
            order.push(id);
        }
    }
    order.extend((0..count).filter(|&id| !seen[id]));
    order
}

/// `id` moved to the front, with everything else keeping its relative place.
///
/// The whole of "last opened first": the game that just started goes first, and
/// the row is otherwise as the player last saw it. An `id` outside `0..count`
/// changes nothing but is still normalized, so a bad stored list is repaired by
/// the same call that would have promoted into it.
pub fn promote(stored: &[usize], count: usize, id: usize) -> Vec<usize> {
    let mut order = normalize(stored, count);
    if let Some(at) = order.iter().position(|&other| other == id) {
        order.remove(at);
        order.insert(0, id);
    }
    order
}

/// The four values `order_mode` can hold, and what each one means to the page.
///
/// Kept here rather than in `config` because the page's order control offers exactly
/// this list: they are one set of names with two readers, not a config detail.
pub const MODES: [&str; 4] = ["usage", "alphabetic", "catalog", "user"];

/// Whether `name` is one of [`MODES`]. Used both when reading the config and
/// when the page asks to change it — an unknown mode is left at the default
/// rather than stored and puzzled over on the next run.
pub fn isMode(name: &str) -> bool {
    MODES.contains(&name)
}
