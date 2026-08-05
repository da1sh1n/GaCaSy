// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

// ########## THE COVERS ##########

import { PAD, TYPE_UNIT, setVar } from "./theme.js";
import { games, grid, nameplate, arrange_btn } from "./dom.js";

const template = document.getElementById("card-template");

export const cards = [];

export const imgs = games.map((game, index) => {
  const card = template.content.firstElementChild.cloneNode(true);
  const img = card.querySelector("img");
  const note = card.querySelector(".note");

  img.src = game.image;
  img.alt = game.name;
  card.title = game.name;

  // Rust checked at startup, so a game whose exe is absent is settled before
  // the player touches anything rather than discovered by a click that does
  // nothing. A disabled button takes no pointer events, so hover and the arrow
  // keys skip it and it is never the selected cover.
  if (game.available === false) {
    card.classList.add("unavailable");
    card.disabled = true;
    card.title = game.name + " — missing " + game.exe;
    note.textContent = "Game files missing";
  } else {
    card.addEventListener("pointerenter", () => select(index));
    // Focus and hover feed the same index, so a cover reached with the arrow
    // keys is lifted and named as well as outlined.
    card.addEventListener("focus", () => select(index));

    // Art that won't load otherwise leaves a bare rectangle reading as
    // placeholder art. The card stays clickable: the game itself still runs.
    img.addEventListener("error", () => {
      card.classList.add("unavailable", "no-cover");
      note.textContent = "Cover missing";
      // Cleared so the webview doesn't lay alt text out over the card above.
      img.alt = "";
    });
  }

  grid.appendChild(card);
  cards.push(card);
  return img;
});

// ========== Which Cover Is Pointed At ==========
// One index, fed by both the mouse and the keyboard. Separate hover and focus
// states can disagree, and then the row lifts one cover while the name line
// describes another. The focus RING is still drawn by :focus-visible.

let selected = -1;
let note_covers_name = false;

export function select(index) {
  if (index === selected) return;
  if (index >= 0 && (!cards[index] || cards[index].disabled)) return;

  if (cards[selected]) cards[selected].classList.remove("selected");
  selected = index;
  if (cards[selected]) cards[selected].classList.add("selected");

  nameplate.textContent = index >= 0 ? games[index].name : "";
  yieldToNote();
  placeNameplate();
}

// A missing or failed cover already carries a message where the name would go,
// and that message is the more important of the two. Settled here rather than
// inside placeNameplate(), which runs per scroll frame and must not force a
// style flush on a webview with no GPU to spare.
export function yieldToNote() {
  const note = cards[selected] && cards[selected].querySelector(".note");
  note_covers_name = !!note && note.textContent !== "" &&
    getComputedStyle(note).display !== "none";
  nameplate.style.visibility = note_covers_name ? "hidden" : "";
}

// The name sits under the cover it names, clamped inside the border gap so a
// long name at either end stays on screen. Centred on the window instead would
// put the leftmost cover's name under the middle one.
export function placeNameplate() {
  if (selected < 0 || !cards[selected] || note_covers_name) return;

  const rect = cards[selected].getBoundingClientRect();
  const half = nameplate.offsetWidth / 2;
  const centre = rect.left + rect.width / 2;
  const clamped = Math.max(PAD + half, Math.min(window.innerWidth - PAD - half, centre));
  nameplate.style.left = (clamped - half).toFixed(1) + "px";
}

// Taken from the gap that is actually there, so a tight border_gap gets a
// smaller line rather than a clipped one. Steps rather than scales: under a
// pixel face there is nothing between 11 and 22px that is not blurry.
export function sizeNameplate() {
  // Must agree with #nameplate's height in style.css.
  const band = Math.max(0, PAD - 17);
  const steps = Math.min(2, Math.max(1, Math.floor((band * 0.85) / TYPE_UNIT)));
  setVar("--nameplate-size", (steps * TYPE_UNIT).toFixed(3) + "px");
}

// ========== Arrange Mode ==========

let arranging = false;

export const isArranging = () => arranging;

export function setArranging(on) {
  arranging = on;
  document.body.classList.toggle("arranging", on);
  arrange_btn.setAttribute("aria-pressed", String(on));

  // A missing game's cover still holds a place in the row and still has to be
  // draggable, and a disabled button takes no pointer events at all. The launch
  // guard is what keeps it unplayable in the meantime.
  cards.forEach((card, id) => {
    card.disabled = on ? false : games[id].available === false;
  });
}
