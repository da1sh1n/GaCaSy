// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

// ########## THE MOVING BACKGROUND ##########

// After Coalesce, from the Codrops Ambient Canvas Backgrounds set, with the
// spiral reversed to run outward and hexagons in place of its squares.
//
// Its glow is two full-window blur passes per frame, which is cheap as a GPU
// shader and unusable in software — this webview runs with --disable-gpu. The
// blur is therefore done ONCE, into a sprite per tint and rotation, and the
// per-frame loop is nothing but blits.

import { BACKDROP_EFFECT, BACKDROP_RAMP } from "./theme.js";

const { PI, cos, sin, atan2, random, round, floor } = Math;
const TAU = 2 * PI;
const HALF_PI = 0.5 * PI;
const rand = (n) => n * random();
const lerp = (from, to, t) => (1 - t) * from + t * to;

// Where a particle is, not how old it is.
//
// Coalesce fades on lifespan, which suits a field converging on the middle. Ours
// is born in the middle — behind the covers — and the middle is the one place
// nothing should be visible, so age is the wrong axis entirely. A hexagon
// instead comes up out of nothing at the centre, is solid by the time it clears
// the covers, and stays solid all the way out.
//
// Measured in half-windows, so 1 is the edge whichever way it went.
const FADE_BY = 0.2;
function radialFade(x, y) {
  const rx = (x - centre_x) / centre_x;
  const ry = (y - centre_y) / centre_y;
  const reach = Math.sqrt(rx * rx + ry * ry);
  return reach >= FADE_BY ? 1 : reach / FADE_BY;
}

// Sprites are square and the hexagon sits well inside them: a blur reaches
// about three times its radius, and anything clipped by the sprite edge shows
// as a straight line across the glow.
const SPRITE_PX = 96;
const HEX_RADIUS = 24;
const GLOW_BLUR = 6;
// How much of the glow pass lands. The shape is the point; the bloom is there
// to stop it reading as a wireframe, not to be the thing you notice.
const GLOW_ALPHA = 0.26;
// Six covers every appearance a hexagon has — it repeats every 60 degrees — and
// cycling through all six is one full apparent turn.
const HEX_ROTATIONS = 6;
const HALO_PX = 96;

const PARTICLE_COUNT = 140;
const PARTICLE_PROPS = 9;
// Drawn every other frame, like the fog, and for a reason that turned out to
// dominate everything else: a canvas is handed to the compositor and stretched
// over the window once per frame it is touched, and with no GPU that resample
// is the largest single cost in the effect. Halving how often it happens halves
// it. The speeds below are doubled to match, so the field moves at the pace it
// looks like it should.
const FIELD_EVERY = 2;
// One canvas pixel per CSS pixel — not per DEVICE pixel, which on a scaled
// display would be a third more work again for a glow nobody is inspecting.
//
// Not lower than 1, either. Fog can be drawn at a fortieth and stretched back
// because fog has no edges; a hexagon has six, and stretching turns them into a
// smear. The saving has to come from how OFTEN this is drawn, not from how
// sharp it is.
const FIELD_SCALE = 1;
// Every sixth particle carries a halo. Coalesce blurs the composite, so its
// particles bloom into each other; per-sprite glow cannot, and this is what
// gets some of that mass back without a per-frame filter. Sparing, because a
// halo is the most expensive thing here — it blends a box several times the
// area of the hexagon inside it.
const HALO_EVERY = 10;
const HALO_SCALE = 1.7;
const HALO_ALPHA = 0.2;

// Only a backstop now that the fade is radial and `offField` recycles anything
// that leaves: nothing should ever reach this, and a particle that somehow
// stalls mid-window still gets collected.
const BASE_TTL = 1800;
const RANGE_TTL = 1200;
const BASE_SPEED = 0.18;
const RANGE_SPEED = 0.68;
const BASE_SIZE = 26;
const RANGE_SIZE = 44;
// The orbit, in half-window units per frame before `speed` scales it. Applied
// through the aspect below, so this is the pace along the SHORT axis and the
// long one is proportionally quicker — which is what makes one lap take the
// same time whichever way round the ellipse it goes.
const ORBIT = 2;
// Particles are born on an ellipse around the centre rather than anywhere on
// screen: outward motion drains a uniform spawn within seconds and leaves a
// ring. Measured as a fraction of the window rather than in pixels, so the
// spread is a proportion of the shelf on any size of display. The floor keeps a
// particle off the exact centre, where its outward direction is undefined.
const SPAWN_FLOOR = 0.06;
const SPAWN_SPREAD = 0.3;
// Kept from Coalesce. Without it the field is a starburst rather than a spiral.
const SWIRL = 0.75 * HALF_PI;
const STEER = 0.05;
// No trail. Coalesce fades the canvas instead of clearing it, which works for a
// filled particle because what is left behind is a smear. A hexagon is an
// OUTLINE: what a fade leaves behind is last frame's outline, offset by however
// far the thing moved, and a few of those at once read as ghosts rather than as
// motion. Cleared outright, every frame, so there is exactly one of each.
// How far past the window a particle runs before it is recycled rather than
// drawn where nobody can see it.
const EDGE_SLACK = 40;

// Fog is not made of blobs. Soft circles read as soft circles however many you
// use, because real smoke has structure at several scales at once and a lane of
// dark between the billows. It is drawn per pixel instead, from domain-warped
// noise — fractal noise whose coordinates are themselves displaced by more
// noise, which is what turns clouds into something that looks like it is
// flowing.
//
// Per pixel is affordable for exactly one reason: the buffer is 160 across.
// That is about a fortieth of the window's pixels, and the one blit that blows
// it back up smooths it on the way — so the softness costs nothing and no blur
// is ever computed. Smoke wants to be soft, so the cheap thing and the right
// thing are the same thing here.
const FOG_BUFFER_W = 160;
const FOG_OCTAVES = 3;
// How far the warp displaces the field it samples. The whole difference between
// "clouds" and "smoke".
const FOG_WARP = 3.4;
const FOG_ZOOM = 2.6;
// Per frame, in noise units. The visible field is about one unit tall, so these
// are "a full drift up the window in roughly fifteen seconds" and a slow churn
// underneath it. Smoke that crosses the window in a second reads as steam off a
// kettle, not as something the room is sitting in.
const FOG_RISE = 0.0012;
const FOG_DRIFT = 0.0006;
// Where the density ramp starts and how quickly it climbs. Tightening this is
// what gives the field its contrast — a wide ramp is a flat grey wash.
const FOG_FLOOR = 0.34;
const FOG_SPAN = 0.42;
// Recomputed every other frame. At the pace above a plume moves about a
// thousandth of the window between frames, so half of them are redrawing an
// image that has not visibly changed — and the noise is the one thing in this
// file that costs real arithmetic. The canvas keeps the last one meanwhile.
const FOG_EVERY = 2;

const RESIZE_MS = 150;

const REDUCED = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

// ========== The Sprite Sheet ==========
// Built once. Every per-pixel cost in the effect — the glow, the soft edge of a
// plume — is paid here rather than sixty times a second.

// #rrggbb, always: these come from toHex in theme.js.
const channelsOf = (hex) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
// Named rather than using "transparent" as a gradient stop, which resolves to
// transparent BLACK and fringes a coloured plume with grey.
const rgbaOf = (hex, alpha) => `rgba(${channelsOf(hex).join(", ")}, ${alpha})`;

function makeSprite(px, paint) {
  const sprite = document.createElement("canvas");
  sprite.width = px;
  sprite.height = px;
  paint(sprite.getContext("2d"));
  return sprite;
}

// A blurred stroke on its own is all halo and no shape. Stroked twice — once
// through the blur for the glow, once clean for the core — is what keeps the
// hexagon readable at the small end of the size range.
function paintHexagon(ctx, colour, turn) {
  const centre = SPRITE_PX / 2;

  ctx.beginPath();
  for (let corner = 0; corner < 6; corner++) {
    const angle = turn + (corner * TAU) / 6;
    const x = centre + HEX_RADIUS * cos(angle);
    const y = centre + HEX_RADIUS * sin(angle);
    if (corner === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  }
  ctx.closePath();

  ctx.strokeStyle = colour;
  ctx.lineJoin = "round";
  ctx.lineWidth = 5;
  ctx.globalAlpha = GLOW_ALPHA;
  ctx.filter = `blur(${GLOW_BLUR}px)`;
  ctx.stroke();

  // Drawn well below the sprite's own size, so the line has to be laid down
  // thick enough to survive that: a hairline here is under a pixel by the time
  // it lands, and an edge thinner than a pixel is an edge the eye loses.
  ctx.globalAlpha = 1;
  ctx.filter = "none";
  ctx.lineWidth = 4;
  ctx.stroke();
}

function paintGlobe(ctx, px, colour, stops) {
  const centre = px / 2;
  const fill = ctx.createRadialGradient(centre, centre, 0, centre, centre, centre);
  stops.forEach(([at, alpha]) => fill.addColorStop(at, rgbaOf(colour, alpha)));
  ctx.fillStyle = fill;
  ctx.fillRect(0, 0, px, px);
}

// Indexed [tint][rotation]. Built on demand and only for the effect that is
// actually on, so a cartridge set to "simple" pays for none of this.
let hex_sprites = [];
let halo_sprites = [];

function buildSprites() {
  // Fog draws its own pixels and wants none of these.
  if (BACKDROP_EFFECT === "fog") return;
  hex_sprites = BACKDROP_RAMP.map((colour) =>
    Array.from({ length: HEX_ROTATIONS }, (_, step) =>
      makeSprite(SPRITE_PX, (paint) =>
        paintHexagon(paint, colour, (step / HEX_ROTATIONS) * (TAU / 6)))));
  halo_sprites = BACKDROP_RAMP.map((colour) =>
    makeSprite(HALO_PX, (paint) => paintGlobe(paint, HALO_PX, colour, [[0, 0.85], [1, 0]])));
}

// ========== The Field ==========

let screen = null;
let ctx = null;
let solid_fill = "";

const props = new Float32Array(PARTICLE_COUNT * PARTICLE_PROPS);

let view_w = 0;
let view_h = 0;
let centre_x = 0;
let centre_y = 0;

// The orbit is worked out in window-relative space rather than in pixels, which
// is the whole reason the spiral is an ellipse and not a circle. A circle in a
// window nearly three times wider than it is tall leaves both ends of the shelf
// empty; an ellipse on the window's own proportions sweeps all of it.
const headingAt = (x, y) => atan2((y - centre_y) / view_h, (x - centre_x) / view_w) + SWIRL;

// Where a particle starts. `spread` puts it anywhere in the window rather than
// in the spawn disc at the middle, and is for the FIRST fill only — see
// seedField. Everything recycled afterwards is born in the middle, because that
// is what the effect is.
function initParticle(i, spread) {
  let x;
  let y;
  if (spread) {
    x = rand(view_w);
    y = rand(view_h);
  } else {
    const angle = rand(TAU);
    const reach = SPAWN_FLOOR + rand(SPAWN_SPREAD);
    x = centre_x + cos(angle) * reach * view_w;
    y = centre_y + sin(angle) * reach * view_h;
  }

  const theta = headingAt(x, y);
  props.set([
    x, y,
    cos(theta) * ORBIT * (view_w / view_h), sin(theta) * ORBIT,
    spread ? rand(BASE_TTL) : 0,
    BASE_TTL + rand(RANGE_TTL),
    BASE_SPEED + rand(RANGE_SPEED),
    BASE_SIZE + rand(RANGE_SIZE),
    rand(BACKDROP_RAMP.length) | 0,
  ], i);
}

// The window opens with the field already across it, rather than with everything
// stacked in the middle working its way out. Seeded the way it recycles, the
// shelf spends its first several seconds visibly filling up — which is time the
// launcher does not have, since the whole point of it is to be looked at the
// moment a drive is plugged in. Scattered, it opens mid-flow.
//
// Ages are scattered with them, so they do not all reach the edge together and
// leave the window empty at once.
//
// Fog has no state to seed — every one of its frames is computed from the clock.
function seedField() {
  if (BACKDROP_EFFECT === "fog") return;
  for (let i = 0; i < props.length; i += PARTICLE_PROPS) initParticle(i, true);
}

// ---------- the hexagon field ----------

const offField = (x, y) =>
  x < -EDGE_SLACK || x > view_w + EDGE_SLACK || y < -EDGE_SLACK || y > view_h + EDGE_SLACK;

function stepParticle(i) {
  const x = props[i];
  const y = props[i + 1];
  // Measured from the centre TO the particle, where Coalesce takes it the other
  // way round. That one swap is the whole of "outward instead of inward".
  const theta = headingAt(x, y);
  const vx = lerp(props[i + 2], ORBIT * cos(theta) * (view_w / view_h), STEER);
  const vy = lerp(props[i + 3], ORBIT * sin(theta), STEER);
  const life = props[i + 4];
  const ttl = props[i + 5];
  const size = props[i + 7];
  const tint = props[i + 8];

  const alpha = radialFade(x, y);
  if (alpha > 0) {
    // Advancing the sprite index with age tumbles the hexagon for free: six
    // steps of ten degrees is one full turn of a shape that repeats at sixty.
    const turn = ((i / PARTICLE_PROPS) + (life * 0.25)) | 0;
    if (i % (PARTICLE_PROPS * HALO_EVERY) === 0) {
      const reach = size * HALO_SCALE;
      ctx.globalAlpha = alpha * HALO_ALPHA;
      ctx.drawImage(halo_sprites[tint], x - reach / 2, y - reach / 2, reach, reach);
    }
    ctx.globalAlpha = alpha;
    ctx.drawImage(hex_sprites[tint][turn % HEX_ROTATIONS], x - size / 2, y - size / 2, size, size);
  }

  props[i] = x + vx * props[i + 6];
  props[i + 1] = y + vy * props[i + 6];
  props[i + 2] = vx;
  props[i + 3] = vy;
  props[i + 4] = life + 1;

  // Recycled on either count. Outward motion means most particles reach an edge
  // long before their lifespan runs out, and one drawn off screen costs the
  // same as one drawn on it.
  if (life > ttl || offField(x, y)) initParticle(i, false);
}

let field_time = 0;

function drawParticles() {
  field_time += 1;
  if (field_time % FIELD_EVERY !== 0) return;

  ctx.globalCompositeOperation = "source-over";
  ctx.globalAlpha = 1;
  ctx.fillStyle = solid_fill;
  ctx.fillRect(0, 0, view_w, view_h);

  ctx.globalCompositeOperation = "lighter";
  for (let i = 0; i < props.length; i += PARTICLE_PROPS) stepParticle(i);
}

// ---------- fog ----------

// Value noise off an integer hash rather than a permutation table: no setup, no
// allocation, and the lattice is generated where it is read.
function hash2(x, y) {
  let h = (x | 0) * 374761393 + (y | 0) * 668265263;
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  return ((h ^ (h >>> 16)) >>> 0) / 4294967296;
}

function valueNoise(x, y) {
  const xi = floor(x);
  const yi = floor(y);
  const xf = x - xi;
  const yf = y - yi;
  // Smoothstep on both axes, so the lattice never shows as a diamond grid.
  const u = xf * xf * (3 - 2 * xf);
  const v = yf * yf * (3 - 2 * yf);
  const a = hash2(xi, yi);
  const b = hash2(xi + 1, yi);
  const c = hash2(xi, yi + 1);
  const d = hash2(xi + 1, yi + 1);
  return a + (b - a) * u + (c - a) * v + (a - b - c + d) * u * v;
}

function fbm(x, y) {
  let sum = 0;
  let amplitude = 0.5;
  let fx = x;
  let fy = y;
  for (let octave = 0; octave < FOG_OCTAVES; octave++) {
    sum += amplitude * valueNoise(fx, fy);
    // Not exactly 2, so the octaves never line their lattices up and repeat.
    fx *= 2.07;
    fy *= 2.07;
    amplitude *= 0.5;
  }
  return sum;
}

let fog_buffer = null;
let fog_ctx = null;
let fog_image = null;
let fog_pixels = null;
let fog_w = 0;
let fog_h = 0;
let fog_time = 0;
// The two ends of the ramp: what the thin air is, and what a dense core is.
let fog_low = [0, 0, 0];
let fog_high = [255, 255, 255];

function resizeFog() {
  fog_w = FOG_BUFFER_W;
  fog_h = Math.max(2, round(FOG_BUFFER_W * (view_h / view_w)));
  fog_buffer = document.createElement("canvas");
  fog_buffer.width = fog_w;
  fog_buffer.height = fog_h;
  fog_ctx = fog_buffer.getContext("2d");
  fog_image = fog_ctx.createImageData(fog_w, fog_h);
  // A 32-bit view, so a pixel is one store instead of four.
  fog_pixels = new Uint32Array(fog_image.data.buffer);
}

function drawFog() {
  fog_time += 1;
  if (fog_time % FOG_EVERY !== 0) return;

  const rise = fog_time * FOG_RISE;
  const drift = fog_time * FOG_DRIFT;
  const aspect = fog_h / fog_w;
  let at = 0;

  for (let py = 0; py < fog_h; py++) {
    const ny = (py / fog_h) * FOG_ZOOM * aspect;
    for (let px = 0; px < fog_w; px++) {
      const nx = (px / fog_w) * FOG_ZOOM;

      // The warp: a first noise field displaces the coordinates the second one
      // is read at. Without this line the whole thing is clouds.
      const warp = fbm(nx + drift, ny - rise * 0.35);
      const density = fbm(
        nx + warp * FOG_WARP + drift * 0.5,
        ny + warp * FOG_WARP - rise,
      );

      let lit = (density - FOG_FLOOR) / FOG_SPAN;
      lit = lit < 0 ? 0 : lit > 1 ? 1 : lit;

      const r = fog_low[0] + (fog_high[0] - fog_low[0]) * lit;
      const g = fog_low[1] + (fog_high[1] - fog_low[1]) * lit;
      const b = fog_low[2] + (fog_high[2] - fog_low[2]) * lit;
      // Little-endian: 0xAABBGGRR.
      fog_pixels[at++] = 0xff000000 | (b << 16) | (g << 8) | r;
    }
  }

  fog_ctx.putImageData(fog_image, 0, 0);
  ctx.globalCompositeOperation = "source-over";
  ctx.globalAlpha = 1;
  // The upscale is the softness. Nothing here is ever blurred.
  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = "high";
  ctx.drawImage(fog_buffer, 0, 0, view_w, view_h);
}

const drawFrame = BACKDROP_EFFECT === "fog" ? drawFog : drawParticles;

// ========== When It Runs ==========
// Three things stop it, and each one matters more here than it would on a web
// page: the launcher shares a machine with the game it is starting.

let frame = 0;
let running = false;
let launch_paused = false;
let blur_paused = !document.hasFocus();

function sync() {
  const wanted = !REDUCED && !launch_paused && !blur_paused;
  if (wanted === running) return;
  running = wanted;
  if (running) {
    frame = requestAnimationFrame(tick);
  } else {
    cancelAnimationFrame(frame);
    frame = 0;
  }
}

function tick() {
  drawFrame();
  if (running) frame = requestAnimationFrame(tick);
}

// The canvas holds fewer pixels than the window and CSS stretches it back over
// it. The context is scaled to match, so the rest of this file goes on working
// in CSS pixels and knows nothing about it.
function resizeField() {
  view_w = window.innerWidth;
  view_h = window.innerHeight;
  screen.width = Math.max(1, round(view_w * FIELD_SCALE));
  screen.height = Math.max(1, round(view_h * FIELD_SCALE));
  // Resizing a canvas resets its context, this transform included.
  ctx.setTransform(FIELD_SCALE, 0, 0, FIELD_SCALE, 0, 0);
  centre_x = view_w / 2;
  centre_y = view_h / 2;
  // Its own buffer follows the window's proportions, not its size — the whole
  // point is that it stays 160 across whatever the window does.
  if (BACKDROP_EFFECT === "fog") resizeFog();
}

function startBackdrop() {
  const host = document.getElementById("backdrop");
  if (!host) return;

  // The page colour, straight from the variable theme.js resolved rather than
  // worked out again here.
  const primary_rgb =
    getComputedStyle(document.documentElement).getPropertyValue("--primary-rgb").trim()
    || "25, 19, 37";
  solid_fill = `rgb(${primary_rgb})`;

  // Thin air is the page's own colour and a dense core is the top of the ramp,
  // so the fog spans the full range the palette allows rather than sitting as a
  // pale wash somewhere in the middle of it.
  fog_low = primary_rgb.split(",").map((part) => parseInt(part.trim(), 10));
  fog_high = channelsOf(BACKDROP_RAMP[BACKDROP_RAMP.length - 1]);

  screen = document.createElement("canvas");
  ctx = screen.getContext("2d");
  host.appendChild(screen);

  buildSprites();
  resizeField();
  seedField();

  // Reduced motion gets the field as a still: seeded across the whole window
  // rather than in the spawn disc, drawn once, and then nothing. Movement is
  // the only thing that goes — the same rule the stylesheet already follows.
  if (REDUCED) {
    drawFrame();
    return;
  }

  let resize_timer = 0;
  window.addEventListener("resize", () => {
    clearTimeout(resize_timer);
    resize_timer = setTimeout(resizeField, RESIZE_MS);
  });

  // A launch has the whole machine to itself. Watched rather than called from
  // launch.js so that this file stays out of the launch path — and because a
  // launch that FAILS unwinds the class again, which should bring the field
  // back rather than leave a dead canvas.
  new MutationObserver(() => {
    launch_paused = document.body.classList.contains("launching");
    sync();
  }).observe(document.body, { attributes: true, attributeFilter: ["class"] });

  // --disable-backgrounding-occluded-windows (ui.rs) defeats the throttling
  // that would otherwise stop rAF for a window nobody is looking at, so this
  // has to be done by hand.
  //
  // Polled as well as listened for, and the poll is the part that actually
  // works: the blur and focus events did not fire when the window was minimised
  // here — measured, not assumed — and a field left running behind a game is
  // the one case this whole guard exists for. Twice a second costs nothing next
  // to the frames it saves.
  const watchFocus = () => {
    const away = !document.hasFocus() || document.hidden;
    if (away === blur_paused) return;
    blur_paused = away;
    sync();
  };
  window.addEventListener("blur", watchFocus);
  window.addEventListener("focus", watchFocus);
  document.addEventListener("visibilitychange", watchFocus);
  setInterval(watchFocus, 500);

  sync();
}

if (BACKDROP_EFFECT !== "simple") startBackdrop();
