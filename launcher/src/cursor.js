// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

// ########## WHICH WAY THE RING IS DRAWN ##########
// The pointer is a system cursor, not an element following the mouse, so it
// cannot blend with what is under it. Instead the art under the pointer is
// measured up front and the ring is swapped for its opposite when the pixels
// call for it.

import { CURSOR_LIGHT, CURSOR_DARK, CURSOR_PIN, FIELD_IS_LIGHT, useCursor } from "./theme.js";
import { imgs } from "./cards.js";

// Coarse on purpose: the ring is 24px across and covers 600x900 art, so a cell
// here is already smaller than the ring. Reading each cover once at this size
// is what keeps the pointer's own work down to an array index per frame.
const GRID_W = 32;
const GRID_H = 48;

// 8-bit luma. Above the first the art is pale enough for a black ring, below
// the second pale enough for a white one — two thresholds rather than one, so
// art sitting on the line doesn't flicker between them as the pointer moves.
const DARK_RING_ABOVE = 150;
const LIGHT_RING_BELOW = 110;

const grids = new WeakMap();

function measure(img) {
  const canvas = document.createElement("canvas");
  canvas.width = GRID_W;
  canvas.height = GRID_H;
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  ctx.drawImage(img, 0, 0, GRID_W, GRID_H);

  let pixels;
  try {
    pixels = ctx.getImageData(0, 0, GRID_W, GRID_H).data;
  } catch (error) {
    // Only reachable if the art ever stops being same-origin with the page.
    // The cover then reads as the field does, which is the honest answer when
    // its pixels can't be looked at.
    console.warn("cover unreadable for the pointer:", img.src, error);
    return;
  }

  const grid = new Uint8Array(GRID_W * GRID_H);
  for (let i = 0; i < grid.length; i++) {
    const p = i * 4;
    // Rec. 601 weights as eighths of 256, so this stays integer.
    grid[i] = (pixels[p] * 77 + pixels[p + 1] * 150 + pixels[p + 2] * 29) >> 8;
  }
  grids.set(img, grid);
}

// The mean of a 3x3 block, so one bright speck in dark art doesn't flip the
// ring the moment the pointer crosses it.
function lumaAt(grid, col, row) {
  let total = 0;
  let count = 0;
  for (let y = Math.max(0, row - 1); y <= Math.min(GRID_H - 1, row + 1); y++) {
    for (let x = Math.max(0, col - 1); x <= Math.min(GRID_W - 1, col + 1); x++) {
      total += grid[y * GRID_W + x];
      count++;
    }
  }
  return total / count;
}

// Null wherever there is no cover art to read — off the row, or over the
// toolbar.
function lumaUnder(x, y) {
  const card = document.elementFromPoint(x, y)?.closest(".card");
  const img = card?.querySelector("img");
  const grid = img && grids.get(img);
  if (!grid) return null;

  const rect = img.getBoundingClientRect();
  if (rect.width === 0 || rect.height === 0) return null;

  const col = Math.floor(((x - rect.left) / rect.width) * GRID_W);
  const row = Math.floor(((y - rect.top) / rect.height) * GRID_H);
  if (col < 0 || col >= GRID_W || row < 0 || row >= GRID_H) return null;

  return lumaAt(grid, col, row);
}

let dark_ring = FIELD_IS_LIGHT;

function setRing(dark) {
  if (dark === dark_ring) return;
  dark_ring = dark;
  useCursor(dark ? CURSOR_DARK : CURSOR_LIGHT);
}

function pointAt(x, y) {
  // The scrim is a wash of black over everything, covers included, so while a
  // game starts there is nothing to measure and the answer is always the same.
  if (document.body.classList.contains("launching")) return setRing(false);

  const luma = lumaUnder(x, y);
  if (luma === null) return setRing(FIELD_IS_LIGHT);

  setRing(dark_ring ? luma > LIGHT_RING_BELOW : luma > DARK_RING_ABOVE);
}

// A pinned cursor_color is a cartridge saying which ring it wants; nothing here
// gets to overrule it, and none of the measuring above is worth doing.
if (!CURSOR_PIN) {
  imgs.forEach((img) => {
    if (img.complete && img.naturalWidth > 0) measure(img);
    else img.addEventListener("load", () => measure(img), { once: true });
  });

  // One read per frame at most: pointermove fires far faster than the cursor
  // can be redrawn, and every one of those events would otherwise hit-test the
  // row and average nine cells.
  let frame = 0;
  let last_x = 0;
  let last_y = 0;

  document.addEventListener("pointermove", (event) => {
    last_x = event.clientX;
    last_y = event.clientY;
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      pointAt(last_x, last_y);
    });
  });
}
