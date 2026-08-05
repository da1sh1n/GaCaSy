// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

// ########## STARTING A GAME ##########

import { MOVE_MS, OUTRO_MS, MIN_LOADING_MS, TRACK_GAP, setVar } from "./theme.js";
import { games, send, status_line } from "./dom.js";
import { cards, imgs, isArranging, placeNameplate } from "./cards.js";

// One game at a time: while a launch is in flight every other cover is on its
// way off screen anyway.
let state = "idle";
let launched_at = 0;

export const isLaunching = () => state !== "idle";

// Named so unpinning can clear exactly these. Wiping cssText instead would also
// take the --sign-stroke that layout() set on the card.
const PINNED = ["left", "top", "width", "height", "transform", "transformOrigin"];

// Freezes every card where it sits, in viewport coordinates, so the others
// leaving the flex row can't drag the chosen one sideways mid-flight. Measured
// in one pass before any are written, so no write invalidates a later read.
function pinCards() {
  const rects = cards.map((card) => card.getBoundingClientRect());
  cards.forEach((card, index) => {
    const { left, top, width, height } = rects[index];
    Object.assign(card.style, {
      left: left + "px", top: top + "px",
      width: width + "px", height: height + "px",
    });
    card.classList.add("pinned");
  });
}

function unpinCards() {
  cards.forEach((card) => {
    card.classList.remove("pinned", "dimmed", "chosen");
    PINNED.forEach((property) => { card.style[property] = ""; });
  });
}

// Measured from the <img> rather than the card, with the origin on the image's
// centre so the outro's scale-up pushes off the same point. The cover keeps the
// size it had: resizing it on the way to the centre reads as a glitch.
function centreTransform(card, img) {
  const card_rect = card.getBoundingClientRect();
  const rect = img.getBoundingClientRect();
  const dx = window.innerWidth / 2 - (rect.left + rect.width / 2);
  const dy = window.innerHeight / 2 - (rect.top + rect.height / 2);
  const origin_x = rect.left + rect.width / 2 - card_rect.left;
  const origin_y = rect.top + rect.height / 2 - card_rect.top;

  return {
    origin: `${origin_x}px ${origin_y}px`,
    transform: `translate(${dx}px, ${dy}px)`,
  };
}

function beginLaunch(index) {
  // While the covers are being arranged a press picks one up; it does not start
  // it. This is also what keeps a missing game unplayable during arranging,
  // when its button is temporarily not disabled.
  if (isArranging() || state !== "idle") return;

  const card = cards[index];
  const img = imgs[index];
  if (!card || card.disabled) return;
  state = "launching";
  launched_at = Date.now();

  // A retry clears the last failure: the message belongs to the attempt that
  // produced it, not to the game.
  card.classList.remove("failed");
  card.querySelector(".note").textContent = "";

  pinCards();
  // Flush the pinned layout so the browser has a "from" position to interpolate
  // out of rather than jumping.
  void document.body.offsetWidth;

  const target = centreTransform(card, img);

  // The cover keeps its size through the flight, so its measured box is also
  // its final box — which is what lets the progress line be placed now.
  const img_rect = img.getBoundingClientRect();
  setVar("--track-width", Math.round(img_rect.width) + "px");
  setVar("--track-top",
    Math.round(window.innerHeight / 2 + img_rect.height / 2 + TRACK_GAP) + "px");

  cards.forEach((other, i) => {
    if (i !== index) other.classList.add("dimmed");
  });
  card.classList.add("chosen");
  card.style.transformOrigin = target.origin;
  card.style.transform = target.transform;

  status_line.textContent = "Starting " + games[index].name + "…";
  document.body.classList.add("launching");

  send("launch:" + index);
}

cards.forEach((card, index) => {
  card.addEventListener("click", () => beginLaunch(index));
});

// Called from Rust once the launch resolves: `ok` means the game's window is up
// (see launch.rs) and the launcher's job is done.
window.__launchOutcome = function (index, ok, message) {
  if (state !== "launching") return;
  const card = cards[index];
  if (!card) return;

  if (ok) {
    document.body.classList.add("finishing");
    card.style.transform = card.style.transform + " scale(1.06)";
    // Rust closes on its own deadline too, so a hiccup here can never leave the
    // launcher sitting in front of a running game.
    setTimeout(() => send("close"), OUTRO_MS);
    return;
  }

  // Unwind: the covers come back and the failure is reported in the row, where
  // the player can simply choose it again — but not before the loading state
  // has been up long enough to have been seen.
  const held = Math.max(0, MIN_LOADING_MS - (Date.now() - launched_at));
  setTimeout(() => {
    document.body.classList.remove("launching");
    cards.forEach((other) => other.classList.remove("dimmed"));
    card.style.transform = "";
    setTimeout(() => {
      unpinCards();
      card.classList.add("failed");
      card.querySelector(".note").textContent = message || "Failed to start";
      placeNameplate();
      state = "idle";
    }, MOVE_MS);
  }, held);
};
