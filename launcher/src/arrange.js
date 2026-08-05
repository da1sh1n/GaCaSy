// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

// ########## DRAGGING COVERS INTO A CUSTOM ORDER ##########

import { GAP, EDGE_ZONE, EDGE_SPEED } from "./theme.js";
import { stored, send, gallery } from "./dom.js";
import { cards, isArranging } from "./cards.js";
import { isLaunching } from "./launch.js";
import { order, applyOrder } from "./row.js";

// Where each visible card sat when the drag began, and where the dragged one
// has got to. Null whenever nothing is being dragged.
let drag = null;
let edge_timer = null;

function beginDrag(event, id) {
  if (!isArranging() || isLaunching() || event.button !== 0 || drag) return;

  // Every visible card's slot, measured once. These are viewport coordinates
  // and the row may scroll underneath them — which cancels out, because every
  // slot moves by the same amount and only their differences are compared.
  const visible = order.filter((other) => !cards[other].classList.contains("hidden"));
  const slots = visible.map((other) => {
    const rect = cards[other].getBoundingClientRect();
    return { id: other, width: rect.width, centre: rect.left + rect.width / 2 };
  });
  const from = visible.indexOf(id);
  if (from === -1) return;

  drag = {
    id,
    card: cards[id],
    pointer_id: event.pointerId,
    start_x: event.clientX,
    pointer_x: event.clientX,
    start_scroll: gallery.scrollLeft,
    slots,
    from,
    to: from,
    moved: false,
  };
  drag.card.setPointerCapture(event.pointerId);
  drag.card.classList.add("dragging");
  event.preventDefault();
}

cards.forEach((card, id) => {
  card.addEventListener("pointerdown", (event) => beginDrag(event, id));
});

// Everything that has to be true of the row for the pointer's current position.
// Also called on every auto-scroll frame, during which the pointer is
// stationary but the row moving under it still changes where the card lands.
function updateDrag() {
  const dx = drag.pointer_x - drag.start_x;

  // The card follows the pointer. The scroll delta is added because the card's
  // resting position has slid with the row and the pointer hasn't.
  const scrolled = gallery.scrollLeft - drag.start_scroll;
  drag.card.style.transform = `translateX(${dx + scrolled}px)`;

  // Expressed in the frame the slots were measured in. They have since slid
  // `scrolled` px left of where they were recorded, so bringing the card back
  // into their frame means adding it rather than subtracting — which is what
  // lets a drag held against the edge keep advancing as fresh covers scroll in.
  const centre = drag.slots[drag.from].centre + dx + scrolled;
  let to = drag.from;
  while (to > 0 && centre < drag.slots[to - 1].centre) to--;
  while (to < drag.slots.length - 1 && centre > drag.slots[to + 1].centre) to++;

  if (to !== drag.to) {
    drag.to = to;
    shiftSlots();
  }
}

// Opens a gap at `drag.to` by sliding everything between there and the card's
// original slot across by exactly the space the card took up, whatever the
// covers' individual widths.
function shiftSlots() {
  const shift = drag.slots[drag.from].width + GAP;
  drag.slots.forEach((slot, at) => {
    if (at === drag.from) return;
    let offset = 0;
    if (drag.from < drag.to && at > drag.from && at <= drag.to) offset = -shift;
    if (drag.from > drag.to && at >= drag.to && at < drag.from) offset = shift;
    cards[slot.id].style.transform = offset ? `translateX(${offset}px)` : "";
  });
}

// Dragging toward a cover off the side of the window has to bring it into
// reach. The direction is re-read each frame rather than captured when the loop
// starts, so crossing from one edge zone to the other turns the scroll around.
function autoScroll() {
  if (edge_timer !== null) return;

  const tick = () => {
    if (!drag) { edge_timer = null; return; }

    const rect = gallery.getBoundingClientRect();
    let step = 0;
    if (drag.pointer_x < rect.left + EDGE_ZONE) step = -EDGE_SPEED;
    if (drag.pointer_x > rect.right - EDGE_ZONE) step = EDGE_SPEED;
    if (step === 0) { edge_timer = null; return; }

    const before = gallery.scrollLeft;
    gallery.scrollLeft += step;
    // Already at the end: nothing moved, so there is nothing to keep a frame
    // loop spinning for.
    if (gallery.scrollLeft === before) { edge_timer = null; return; }

    updateDrag();
    edge_timer = requestAnimationFrame(tick);
  };
  edge_timer = requestAnimationFrame(tick);
}

document.addEventListener("pointermove", (event) => {
  if (!drag || event.pointerId !== drag.pointer_id) return;
  drag.pointer_x = event.clientX;
  if (Math.abs(event.clientX - drag.start_x) > 3) drag.moved = true;
  updateDrag();
  autoScroll();
});

document.addEventListener("pointerup", (event) => {
  if (!drag || event.pointerId !== drag.pointer_id) return;

  const { id, card, from, to, slots, moved } = drag;
  drag = null;
  if (edge_timer !== null) { cancelAnimationFrame(edge_timer); edge_timer = null; }

  card.classList.remove("dragging");
  // Every card goes back to no transform of its own; the real move is the DOM
  // reorder below, which puts them where the offsets were pretending they were.
  slots.forEach((slot) => { cards[slot.id].style.transform = ""; });

  if (!moved || from === to) return;

  // The visible row rearranged, then folded back into the full order so covers
  // hidden by a filter keep their places. Arrange mode clears the search, so in
  // practice there are none — this is what makes the fold-back correct rather
  // than merely unused.
  const visible = slots.map((slot) => slot.id);
  visible.splice(from, 1);
  visible.splice(to, 0, id);

  let at = 0;
  stored.user = order.map((other) =>
    slots.some((slot) => slot.id === other) ? visible[at++] : other
  );

  applyOrder();
  send("order:" + stored.user.join(","));
});
