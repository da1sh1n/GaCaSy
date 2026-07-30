// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! The window, the GL context and the event loop — everything under the wizard.
//!
//! This used to be four lines calling `eframe::run_native`. It is a file now for
//! one reason: `eframe` asks for `egui-winit`'s `clipboard` and `links` features
//! in its own manifest, and cargo features are additive, so no dependent can turn
//! them off. Those two features cost this installer:
//!
//! * `arboard`, requested with `image-data`, which pulls in `image` and `png` so
//!   that a *picture* can be pasted — into a program whose only text field holds
//!   a game's name. See [`crate::clipboard`] for the two Win32 calls that replace
//!   it.
//! * `webbrowser`, which pulls in `url`, which pulls in `idna`, which pulls in
//!   the seven `icu_*` crates and their Unicode tables — to open links. This
//!   program contains no links.
//!
//! `egui_glow` offers the same integration with those two behind flags of its
//! own, left off here, so what remains is the window and the event loop that
//! `eframe` would have run. The shape below is the one from `egui_glow`'s
//! `pure_glow` example, with the clipboard wired through the middle of it.
//!
//! Nothing about the wizard itself lives here; [`crate::ui`] draws every frame,
//! and [`App::ui`] is called with the whole window to draw into.

use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};

// Both of these come through `egui_glow` rather than being asked for by name, so
// there is no way for the versions to drift apart from the one it was built for.
use egui_glow::{egui_winit, glow};
use egui_winit::winit;
use glutin::context::NotCurrentGlContext as _;
use glutin::display::{GetGlDisplay as _, GlDisplay as _};
use glutin::surface::GlSurface as _;
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::raw_window_handle::HasWindowHandle as _;

use crate::app::App;
use crate::ui;

/// Opens the window and runs until it closes.
pub fn run(app: App) -> Result<(), winit::error::EventLoopError> {
    let event_loop = EventLoop::<Wake>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    event_loop.run_app(&mut Shell {
        app,
        proxy,
        gl: None,
        egui: None,
        shown: false,
    })
}

/// A repaint asked for from somewhere that is not the event loop.
///
/// The copy runs on a worker thread and calls `request_repaint` after every
/// message — see [`crate::work`]. Without something to wake winit, the loop would
/// sit in `Wait` and the progress bar would move only when the mouse did.
#[derive(Debug)]
struct Wake(Duration);

struct Shell {
    app: App,
    proxy: EventLoopProxy<Wake>,
    gl: Option<GlWindow>,
    egui: Option<Egui>,
    /// The window is created hidden so it never flashes at the wrong size or
    /// colour while the GL context is being set up. It is shown at the top of the
    /// first frame — see [`Shell::frame`] for why not the bottom.
    shown: bool,
}

/// egui's half: the context, the winit translation layer, and the GL painter.
/// Together these are `egui_glow::EguiGlow`, kept apart here because the frame
/// loop has to reach between them to get at the clipboard.
struct Egui {
    ctx: egui::Context,
    state: egui_winit::State,
    painter: egui_glow::Painter,
    viewport: egui::ViewportInfo,
}

impl ApplicationHandler<Wake> for Shell {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gl.is_some() {
            return; // Resumed twice. Windows does not do this, but the trait allows it.
        }

        let gl_window = GlWindow::new(event_loop);
        let gl = Arc::clone(&gl_window.gl);

        let ctx = egui::Context::default();
        ui::configure(&ctx);

        // Every `request_repaint`, from any thread, becomes an event this loop
        // can see.
        let proxy = egui::mutex::Mutex::new(self.proxy.clone());
        ctx.set_request_repaint_callback(move |info| {
            let _ = proxy.lock().send_event(Wake(info.delay));
        });

        let painter = match egui_glow::Painter::new(gl, "", None, true) {
            Ok(painter) => painter,
            // No GL, no wizard. There is no software path to fall back to and
            // nothing useful to say in a window we cannot open.
            Err(error) => {
                eprintln!("The installer could not start its display: {error}");
                event_loop.exit();
                return;
            }
        };
        let state = egui_winit::State::new(
            ctx.clone(),
            egui::ViewportId::ROOT,
            event_loop,
            None,
            event_loop.system_theme(),
            Some(painter.max_texture_side()),
        );

        self.egui = Some(Egui {
            ctx,
            state,
            painter,
            viewport: egui::ViewportInfo::default(),
        });

        self.gl = Some(gl_window);

        // Draw the first frame here rather than asking for one. A hidden window
        // is never sent a paint message, so a `request_redraw` at this point
        // waits for an event that cannot arrive while the thing it would draw is
        // what makes the window worth showing. `frame` shows it once it has
        // something on it.
        self.frame(event_loop);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match &event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => {
                event_loop.exit();
                return;
            }
            WindowEvent::RedrawRequested => {
                self.frame(event_loop);
                return;
            }
            WindowEvent::Resized(size) => {
                if let Some(gl) = &self.gl {
                    gl.resize(*size);
                }
            }
            // egui's own paste handling reads a clipboard that isn't compiled in,
            // and swallows the keystroke whole when it comes back empty — no
            // `Paste`, and no key event either. So the paste is put into the
            // queue here, before egui-winit gets the chance to drop it.
            WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => {
                if let Some(egui) = &mut self.egui
                    && is_paste(egui.state.egui_input().modifiers, event)
                    && let Some(text) = paste()
                    && !text.is_empty()
                {
                    let text = text.replace("\r\n", "\n");
                    egui.state
                        .egui_input_mut()
                        .events
                        .push(egui::Event::Paste(text));
                }
            }
            _ => {}
        }

        let (Some(gl), Some(egui)) = (&self.gl, &mut self.egui) else {
            return;
        };
        if egui.state.on_window_event(&gl.window, &event).repaint {
            gl.window.request_redraw();
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, Wake(delay): Wake) {
        let Some(gl) = &self.gl else { return };
        if delay.is_zero() {
            gl.window.request_redraw();
        } else if let Some(deadline) = Instant::now().checked_add(delay) {
            event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
        }
    }

    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        // A `WaitUntil` came due — this is the animated progress bar ticking.
        if let StartCause::ResumeTimeReached { .. } = cause
            && let Some(gl) = &self.gl
        {
            gl.window.request_redraw();
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(egui) = &mut self.egui {
            egui.painter.destroy();
        }
    }
}

impl Shell {
    /// One frame: run the wizard, deal with what it asked for, paint the result.
    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        // Before painting, not after. A buffer swapped to a hidden window goes
        // nowhere, and showing the window afterwards does not ask for another
        // frame — so a window shown last is a window that stays empty until
        // something else happens to make it redraw.
        if !self.shown
            && let Some(gl) = &self.gl
        {
            self.shown = true;
            gl.window.set_visible(true);
        }

        let Shell { app, gl, egui, .. } = self;
        let (Some(gl), Some(egui)) = (gl.as_ref(), egui.as_mut()) else {
            return;
        };
        let window = &gl.window;

        let input = egui.state.take_egui_input(window);
        let mut output = egui.ctx.run_ui(input, |ui| app.ui(ui));

        // Copy and cut arrive as commands on the way out. egui's integration would
        // hand them to the clipboard it doesn't have, so they are taken here
        // instead — this is the other half of the paste above.
        for command in std::mem::take(&mut output.platform_output.commands) {
            match command {
                egui::OutputCommand::CopyText(text) => copy(&text),
                // Nothing in this program copies an image or opens a link.
                egui::OutputCommand::CopyImage(_) | egui::OutputCommand::OpenUrl(_) => {}
            }
        }

        let repaint_after = output
            .viewport_output
            .values()
            .map(|viewport| viewport.repaint_delay)
            .min()
            .unwrap_or(Duration::MAX);
        for (_, viewport) in output.viewport_output {
            let mut requested = Vec::new();
            egui_winit::process_viewport_commands(
                &egui.ctx,
                &mut egui.viewport,
                viewport.commands,
                window,
                &mut requested,
            );
        }
        egui.state
            .handle_platform_output_with_event_loop(window, event_loop, output.platform_output);

        // Paint. Textures the frame introduced have to be uploaded before it is
        // drawn, and the ones it dropped freed only after.
        for (id, delta) in output.textures_delta.set {
            egui.painter.set_texture(id, &delta);
        }
        // Only ever seen in the moment between a resize and the panels being
        // drawn at their new size, which is exactly when a mismatched colour
        // would look like a flash.
        let background = egui::Rgba::from(egui.ctx.style_of(egui.ctx.theme()).visuals.panel_fill);
        unsafe {
            use glow::HasContext as _;
            gl.gl
                .clear_color(background[0], background[1], background[2], 1.0);
            gl.gl.clear(glow::COLOR_BUFFER_BIT);
        }
        let primitives = egui.ctx.tessellate(output.shapes, output.pixels_per_point);
        egui.painter.paint_primitives(
            window.inner_size().into(),
            output.pixels_per_point,
            &primitives,
        );
        for id in output.textures_delta.free {
            egui.painter.free_texture(id);
        }
        let _ = gl.swap_buffers();

        event_loop.set_control_flow(if repaint_after.is_zero() {
            gl.window.request_redraw();
            ControlFlow::Poll
        } else {
            match Instant::now().checked_add(repaint_after) {
                Some(deadline) => ControlFlow::WaitUntil(deadline),
                None => ControlFlow::Wait,
            }
        });
    }
}

/// Whether this keystroke means paste.
///
/// The same three spellings egui recognises. `logical_key` is what the layout
/// produces and `physical_key` is where the key sits, so a layout with no Latin
/// `V` still pastes from the position `V` occupies on a US keyboard.
fn is_paste(modifiers: egui::Modifiers, event: &winit::event::KeyEvent) -> bool {
    use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};

    match &event.logical_key {
        Key::Named(NamedKey::Paste) => return true,
        Key::Named(NamedKey::Insert) => return modifiers.shift,
        Key::Character(character) if character.eq_ignore_ascii_case("v") => {
            return modifiers.command;
        }
        _ => {}
    }
    modifiers.command && event.physical_key == PhysicalKey::Code(KeyCode::KeyV)
}

#[cfg(windows)]
fn paste() -> Option<String> {
    crate::clipboard::get()
}

#[cfg(windows)]
fn copy(text: &str) {
    crate::clipboard::set(text);
}

// The installer only really runs on Windows; the other targets exist so the
// non-UI half can be built and tested there. Same reasoning as font.rs.
#[cfg(not(windows))]
fn paste() -> Option<String> {
    None
}

#[cfg(not(windows))]
fn copy(_text: &str) {}

/// The window, its GL context, and the surface the two share.
///
/// Lifted from `egui_glow`'s `pure_glow` example, which took it from `eframe` —
/// there is one correct way to spell this and it is not interesting.
struct GlWindow {
    window: winit::window::Window,
    gl: Arc<glow::Context>,
    context: glutin::context::PossiblyCurrentContext,
    surface: glutin::surface::Surface<glutin::surface::WindowSurface>,
}

impl GlWindow {
    fn new(event_loop: &ActiveEventLoop) -> GlWindow {
        let attributes = winit::window::WindowAttributes::default()
            .with_title("GaCaSy Installer")
            .with_inner_size(winit::dpi::LogicalSize::new(920.0, 660.0))
            .with_min_inner_size(winit::dpi::LogicalSize::new(760.0, 520.0))
            .with_resizable(true)
            .with_visible(false);

        let template = glutin::config::ConfigTemplateBuilder::new()
            .prefer_hardware_accelerated(None)
            .with_depth_size(0)
            .with_stencil_size(0)
            .with_transparency(false);

        let (window, config) = glutin_winit::DisplayBuilder::new()
            .with_preference(glutin_winit::ApiPreference::FallbackEgl)
            .with_window_attributes(Some(attributes.clone()))
            .build(event_loop, template, |mut configs| {
                configs.next().expect("a GL config the display supports")
            })
            .expect("a GL display");

        let display = config.display();
        let handle = window
            .as_ref()
            .map(|window| window.window_handle().expect("a window handle").as_raw());

        // A core context first, then GLES. Some drivers — remote desktop and the
        // basic display adapter among them — only offer the second.
        let wanted = glutin::context::ContextAttributesBuilder::new().build(handle);
        let fallback = glutin::context::ContextAttributesBuilder::new()
            .with_context_api(glutin::context::ContextApi::Gles(None))
            .build(handle);
        let context = unsafe {
            display
                .create_context(&config, &wanted)
                .or_else(|_| display.create_context(&config, &fallback))
                .expect("a GL context")
        };

        // The window exists already unless the config search needed to try
        // several, in which case it is made now against the one that won.
        let window = window.unwrap_or_else(|| {
            glutin_winit::finalize_window(event_loop, attributes, &config)
                .expect("a window for the chosen GL config")
        });

        let (width, height) = window.inner_size().into();
        let surface_attributes =
            glutin::surface::SurfaceAttributesBuilder::<glutin::surface::WindowSurface>::new()
                .build(
                    window.window_handle().expect("a window handle").as_raw(),
                    NonZeroU32::new(width).unwrap_or(NonZeroU32::MIN),
                    NonZeroU32::new(height).unwrap_or(NonZeroU32::MIN),
                );
        let surface = unsafe {
            display
                .create_window_surface(&config, &surface_attributes)
                .expect("a drawing surface")
        };
        let context = context
            .make_current(&surface)
            .expect("the GL context to become current");

        // Wait for vblank. This is a wizard; there is nothing to gain from
        // drawing it faster than the screen shows it.
        let _ = surface.set_swap_interval(
            &context,
            glutin::surface::SwapInterval::Wait(NonZeroU32::MIN),
        );

        let gl = unsafe {
            glow::Context::from_loader_function(|symbol| {
                let symbol = std::ffi::CString::new(symbol).expect("a symbol name without a NUL");
                display.get_proc_address(&symbol)
            })
        };

        GlWindow {
            window,
            gl: Arc::new(gl),
            context,
            surface,
        }
    }

    fn resize(&self, size: winit::dpi::PhysicalSize<u32>) {
        // A minimised window reports zero, which is not a surface size.
        let (Some(width), Some(height)) = (
            NonZeroU32::new(size.width),
            NonZeroU32::new(size.height),
        ) else {
            return;
        };
        self.surface.resize(&self.context, width, height);
    }

    fn swap_buffers(&self) -> glutin::error::Result<()> {
        self.surface.swap_buffers(&self.context)
    }
}


