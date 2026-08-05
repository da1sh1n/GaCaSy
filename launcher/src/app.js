// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

// ########## STARTUP ##########

import { imgs, cards, select, sizeNameplate } from "./cards.js";
import { layout } from "./layout.js";
import { order, applyStoredMode, sizeSegments, syncSearchVisibility } from "./row.js";
import { isLaunching } from "./launch.js";
import "./arrange.js";

// Fitting the row settles whether the search box is worth offering and how wide
// the order segments have to be, so the three always move together.
function refit() {
  layout();
  syncSearchVisibility();
  sizeSegments();
  sizeNameplate();
}

applyStoredMode();

// The first cover in the row, so the shelf opens with something already offered
// rather than uniformly veiled. order[0] and not id 0: which cover is first
// depends on the order mode, and "first" means what is on the left.
if (order.length > 0) select(order.find((id) => !cards[id].disabled) ?? -1);

const pending = imgs
  .filter((img) => !img.complete)
  .map((img) => new Promise((res) => { img.onload = img.onerror = res; }));

refit(); // first pass in case the images are already cached
Promise.all(pending).then(refit);

window.addEventListener("resize", () => {
  if (!isLaunching()) refit();
});

// Departure Mono arrives over app:// after this runs and `font-display: swap`
// paints the fallback first. Re-measure once it lands: it is monospace and the
// fallback may not be, so every width measured before it existed used different
// metrics.
if (document.fonts && document.fonts.ready) {
  document.fonts.ready.then(() => {
    sizeSegments();
    sizeNameplate();
  });
}
