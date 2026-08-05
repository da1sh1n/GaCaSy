// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

// ########## FITTING AND SCROLLING THE ROW ##########

import { PAD, GAP, setVar } from "./theme.js";
import { games, stage, grid, gallery, scrollbar, thumb, empty_plates } from "./dom.js";
import { cards, imgs, placeNameplate } from "./cards.js";

let overflowing = false;

export const isOverflowing = () => overflowing;

// Every cover fitted to the window height at its native ratio, never scaled up.
// Note what is NOT in here: the number of games. Covers used to get 1/n of the
// width each, which shrank a big catalog until none of it could be read; the
// row runs off the side and scrolls instead. Rust's window::size reproduces
// this from the same three numbers.
export function layout() {
  // Two PADs: the stage is one border gap from every window edge, and the
  // toolbar and name line take no height of their own. Must match
  // window::size's height_room exactly.
  const avail_h = window.innerHeight - 2 * PAD;

  // The empty state's ghost plates at native 600x900 — with no games there is
  // no image to measure, and constants.rs sizes the window from the same pair.
  if (games.length === 0) {
    const scale = Math.min(1, avail_h / 900);
    setVar("--plate-width", Math.floor(600 * scale) + "px");
    setVar("--plate-height", Math.floor(900 * scale) + "px");
    empty_plates.style.setProperty("--cover-scale", scale.toFixed(4));
    return;
  }

  imgs.forEach((img, index) => {
    const native_w = img.naturalWidth || 600;
    const native_h = img.naturalHeight || 900;
    const scale = Math.min(1, avail_h / native_h);
    const width = Math.floor(native_w * scale);
    img.style.width = width + "px";
    img.style.height = Math.floor(native_h * scale) + "px";
    // The cover's own chrome — radius and shadow — is drawn for native size and
    // multiplied by this, so a cover shrunk for the screen keeps its
    // proportions instead of gaining a radius and a blur too heavy for it.
    cards[index].style.setProperty("--cover-scale", scale.toFixed(4));
    // Keep the missing sign's stroke proportional to the cover it is drawn on.
    cards[index].style.setProperty(
      "--sign-stroke", Math.max(3, Math.round(width * 0.018)) + "px");
  });

  updateOverflow();
  updateScrollbar();
  placeNameplate();
}

// Added up from the cover widths rather than read off scrollWidth, which only
// counts what the search has left showing — narrow the results to two covers
// and a measured row would take away the box you need to get back.
function updateOverflow() {
  // No PAD in here any more: the stage carries the border gap, so the row's
  // own width is the covers and the gaps between them. Measured against the
  // STAGE, not the viewport — the viewport is padded by --cover-room on both
  // sides to leave the shadows somewhere to draw, and counting that room as
  // usable width would hide a row that really does overflow.
  let total = Math.max(0, imgs.length - 1) * GAP;
  imgs.forEach((img) => { total += img.offsetWidth; });
  overflowing = total > stage.clientWidth + 1;
}

// This one IS the live row: the bar describes where you are in what is
// currently on screen, so a filtered row that fits has nothing to say.
export function updateScrollbar() {
  // The stage and the row itself, not the viewport's own box — both of those
  // carry --cover-room and the ratio has to describe the covers, not the room
  // their shadows are drawn in.
  const visible = stage.clientWidth;
  const total = grid.offsetWidth;
  const scrollable = total > visible + 1;
  document.body.classList.toggle("scrollable", scrollable);
  if (!scrollable) return;

  thumb.style.width = ((visible / total) * 100).toFixed(3) + "%";
  thumb.style.left = ((gallery.scrollLeft / total) * 100).toFixed(3) + "%";
}

gallery.addEventListener("scroll", () => {
  updateScrollbar();
  // The selection has not changed but the cover it points at has moved.
  placeNameplate();
});

// A wheel is what a mouse reaches for and points the wrong way for a row. Only
// when there is somewhere to go, so a scroll over a row that fits does nothing
// rather than fighting the page.
gallery.addEventListener("wheel", (event) => {
  if (gallery.scrollWidth <= gallery.clientWidth) return;
  if (event.deltaY === 0) return;
  gallery.scrollLeft += event.deltaY;
  event.preventDefault();
}, { passive: false });

thumb.addEventListener("pointerdown", (event) => {
  const start_x = event.clientX;
  const start_scroll = gallery.scrollLeft;
  const track = scrollbar.clientWidth;
  // A pixel of bar is worth scrollWidth/track pixels of row.
  const move = (moved) => {
    gallery.scrollLeft =
      start_scroll + (moved.clientX - start_x) * (gallery.scrollWidth / track);
  };
  const stop = () => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", stop);
  };
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", stop);
  event.preventDefault();
});
