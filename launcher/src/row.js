// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

// ########## ORDER, SEARCH AND ROW NAVIGATION ##########

import { setVar } from "./theme.js";
import {
  games, stored, send, grid, gallery, mode_group, mode_buttons,
  arrange_btn, search_box,
} from "./dom.js";
import { cards, isArranging, setArranging } from "./cards.js";
import { isOverflowing, updateScrollbar } from "./layout.js";
import { isLaunching } from "./launch.js";

// The ids, left to right, as currently shown.
export let order = [];

const MODES = mode_buttons.map((button) => button.dataset.mode);
let mode = typeof stored.mode === "string" && MODES.includes(stored.mode)
  ? stored.mode
  : "usage";

// ========== What Order The Covers Are In ==========

// A stored id list turned into a complete permutation of 0..games.length. Same
// rule as Rust's order::normalize — see that module for why it is in both.
function normalizeOrder(list) {
  const seen = new Array(games.length).fill(false);
  const result = [];
  for (const id of Array.isArray(list) ? list : []) {
    if (Number.isInteger(id) && id >= 0 && id < games.length && !seen[id]) {
      seen[id] = true;
      result.push(id);
    }
  }
  games.forEach((_, id) => { if (!seen[id]) result.push(id); });
  return result;
}

function computeOrder() {
  if (mode === "catalog") return games.map((_, id) => id);
  if (mode === "alphabetic") {
    return games
      .map((_, id) => id)
      .sort((a, b) => games[a].name.localeCompare(
        games[b].name, undefined, { sensitivity: "base", numeric: true }
      ));
  }
  return normalizeOrder(mode === "user" ? stored.user : stored.usage);
}

// Re-appending a card that is already in #grid moves it, so this reorders the
// row without rebuilding anything. cards[] and imgs[] stay indexed by catalog
// id, which is what `launch:<id>` and __launchOutcome speak.
export function applyOrder() {
  order = computeOrder();
  order.forEach((id) => grid.appendChild(cards[id]));
  document.body.classList.toggle("user-order", mode === "user");
  updateScrollbar();
}

// ========== The Order Control ==========

// Every segment is the width of the widest label, so the pill marking the
// active one is a fixed box that only moves. Measured with the track's own
// width cleared first, or a second pass measures the widths it set last time
// and ratchets them upward.
export function sizeSegments() {
  setVar("--segment-width", "auto");
  const widest = mode_buttons.reduce((most, b) => Math.max(most, b.offsetWidth), 0);
  setVar("--segment-width", widest + "px");
  moveModePill();
}

// A transform, so the slide runs on the compositor and never touches layout.
function moveModePill() {
  const at = MODES.indexOf(mode);
  const width = mode_buttons[0] ? mode_buttons[0].offsetWidth : 0;
  setVar("--segment-offset", Math.max(0, at) * width + "px");
}

export function setMode(next, announce) {
  // Switching to "user" with nothing stored starts from the row as it is on
  // screen. Only on a real switch: at startup `order` is not computed yet, and
  // writing an empty user_order to the cartridge would answer a question
  // nobody asked.
  if (announce && next === "user" &&
      (!Array.isArray(stored.user) || stored.user.length === 0)) {
    stored.user = order.slice();
    send("order:" + stored.user.join(","));
  }
  mode = next;
  mode_buttons.forEach((button) => {
    button.setAttribute("aria-checked", String(button.dataset.mode === mode));
    // Only the active segment is a tab stop, which is how a radiogroup behaves:
    // Tab reaches the control, the arrow keys move within it.
    button.tabIndex = button.dataset.mode === mode ? 0 : -1;
  });
  moveModePill();
  if (mode !== "user") stopArranging();
  applyOrder();
  if (announce) send("mode:" + mode);
}

// `false`: this is the stored mode being applied, not a change to report. The
// launcher writing back the setting it just read would be a disk write on every
// start.
export function applyStoredMode() {
  setMode(mode, false);
}

mode_buttons.forEach((button) => {
  button.addEventListener("click", () => setMode(button.dataset.mode, true));
});

// Left/right inside the group move between segments. This also stops the event:
// the document handler below moves the SELECTED COVER on the same keys, and
// without this one arrow press would change the order and jump along the row.
mode_group.addEventListener("keydown", (event) => {
  if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
  const at = MODES.indexOf(mode);
  const step = event.key === "ArrowRight" ? 1 : -1;
  const next = (at + step + MODES.length) % MODES.length;
  setMode(MODES[next], true);
  mode_buttons[next].focus();
  event.preventDefault();
  event.stopPropagation();
});

// ========== Finding A Cover ==========

// Case- and accent-insensitive, so "pokemon" finds "Pokémon".
const plain = (text) =>
  text.normalize("NFD").replace(/\p{Diacritic}/gu, "").toLowerCase();

const searchable = games.map((game) => plain(game.name));

function applyFilter() {
  const needle = plain(search_box.value.trim());
  cards.forEach((card, id) => {
    card.classList.toggle("hidden", needle !== "" && !searchable[id].includes(needle));
  });
  gallery.scrollLeft = 0;
  updateScrollbar();
}

// The one place the box is shown or hidden: an inline `display` set anywhere
// else would beat any stylesheet rule the other condition tried to use.
export function syncSearchVisibility() {
  search_box.style.display = isOverflowing() && !isArranging() ? "block" : "none";
}

search_box.addEventListener("input", applyFilter);
search_box.addEventListener("keydown", (event) => {
  // Escape clears rather than closing the launcher, which is what it would
  // otherwise be reaching for.
  if (event.key === "Escape" && search_box.value !== "") {
    search_box.value = "";
    applyFilter();
    event.stopPropagation();
  }
});

// ========== Arrange Mode ==========

function stopArranging() {
  if (isArranging()) toggleArranging(false);
}

function toggleArranging(on) {
  setArranging(on);
  // Arranging a filtered row would write an order for covers that aren't all on
  // screen. Clearing the search — and putting it away for the duration — is the
  // honest way out of that.
  if (on && search_box.value !== "") {
    search_box.value = "";
    applyFilter();
  }
  syncSearchVisibility();
}

arrange_btn.addEventListener("click", () => toggleArranging(!isArranging()));

// ========== Walking The Row ==========

// A single line of buttons, which Tab alone walks badly.
document.addEventListener("keydown", (event) => {
  if (isLaunching()) return;
  if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
  if (document.activeElement === search_box) return;
  // The order control uses the same keys and stops the event itself; this is
  // the belt to that pair of braces, for focus resting on the group rather than
  // on one of its buttons.
  if (mode_group.contains(document.activeElement)) return;

  const reachable = order.filter(
    (id) => !cards[id].classList.contains("hidden") && !cards[id].disabled
  );
  if (reachable.length === 0) return;

  const at = reachable.indexOf(cards.indexOf(document.activeElement));
  const step = event.key === "ArrowRight" ? 1 : -1;
  // From nowhere in the row, either arrow enters it at the near end.
  const next = at === -1
    ? (step === 1 ? 0 : reachable.length - 1)
    : Math.min(reachable.length - 1, Math.max(0, at + step));

  cards[reachable[next]].focus();
  cards[reachable[next]].scrollIntoView({ block: "nearest", inline: "nearest" });
  event.preventDefault();
});
