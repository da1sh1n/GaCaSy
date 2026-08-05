// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

// ########## PAGE ELEMENTS AND THE CATALOG ##########

// Set by Rust's init_script, which runs before the parser produces any DOM.
export const games = window.__GAMES__ || [];
export const stored = window.__ORDER__ || {};
const show_captions = window.__SHOW_CAPTIONS__ === true;

const byId = (id) => document.getElementById(id);

export const grid = byId("grid");
export const gallery = byId("gallery");
export const toolbar_left = byId("toolbar-left");
export const mode_group = byId("mode");
export const arrange_btn = byId("arrange");
export const search_box = byId("search");
export const nameplate = byId("nameplate");
export const scrollbar = byId("scrollbar");
export const thumb = byId("scrollbar-thumb");
// Not named `status`: that would shadow window.status.
export const status_line = byId("status");
export const mode_buttons = Array.from(mode_group.querySelectorAll("button"));

export const send = (message) => window.ipc.postMessage(message);

// An empty catalog is a normal state, not a failure. The toolbar's CONTENTS go
// rather than the row itself — the close button lives in that row and an empty
// cartridge still has to be closeable.
if (games.length === 0) {
  gallery.style.display = "none";
  toolbar_left.style.visibility = "hidden";
  search_box.style.visibility = "hidden";
  document.body.classList.add("empty");
} else if (show_captions) {
  document.body.classList.add("captioned");
}
