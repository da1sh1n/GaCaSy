// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Collects the installer's payload — the files it writes onto a cartridge and
//! into the PC — and stages them in `OUT_DIR` for `include_bytes!`.
//!
//! Everything the installer produces is carried inside it: it never downloads
//! anything. That makes **build order a hard dependency**: `launcher` and
//! `listener` must be built `--release` before this crate, because their
//! binaries are the payload.
//!
//! Staging through `OUT_DIR` rather than pointing `include_bytes!` straight at
//! `target/release/launcher.exe` is what lets a missing artifact fail with a
//! sentence you can act on, instead of a compiler error about a path.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Set this to build an installer whose binary payload is empty.
///
/// For working on the UI without a 3-minute release build in front of every
/// iteration. The resulting installer is not shippable and knows it: `payload.rs`
/// reports the empty slots and every screen that would write one refuses to run
/// — so a payload-less build is loud, not silently useless.
const OPTIONAL: &str = "GACASY_PAYLOAD_OPTIONAL";

/// One file the installer carries.
struct Item {
    /// Name it is staged under in `OUT_DIR`; `payload.rs` includes these.
    staged: &'static str,
    /// Env var that overrides where it comes from, for builds that put
    /// artifacts somewhere unusual.
    env_override: &'static str,
    /// Human-readable instruction printed when it is missing.
    remedy: &'static str,
}

fn main() {
    println!("cargo::rerun-if-env-changed={OPTIONAL}");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let release = release_dir(&out_dir, &manifest);

    // The two binaries, then the three seed files. Seeds come from the source
    // tree, so they are always present and always current — the binaries are the
    // only part with a build-order requirement.
    let binaries = [
        (
            Item {
                staged: "launcher.exe",
                env_override: "GACASY_LAUNCHER_EXE",
                remedy: "cargo build --release -p launcher",
            },
            release.join(exe_name("launcher")),
        ),
        (
            Item {
                staged: "listener.exe",
                env_override: "GACASY_LISTENER_EXE",
                remedy: "cargo build --release -p listener",
            },
            release.join(exe_name("listener")),
        ),
    ];

    // The listener's config.toml used to be here. It has none now — what it
    // trusts is compiled into it, and nothing else in the file was worth a file.
    let seeds = [
        ("launcher-config.toml", manifest.join("../launcher/src/config.toml")),
        ("launcher-catalog.json", manifest.join("../launcher/src/catalog.json")),
    ];

    let optional = env::var_os(OPTIONAL).is_some_and(|v| !v.is_empty());
    let mut missing = Vec::new();

    for (item, default) in &binaries {
        println!("cargo::rerun-if-env-changed={}", item.env_override);
        let source = env::var_os(item.env_override)
            .map(PathBuf::from)
            .unwrap_or_else(|| default.clone());
        println!("cargo::rerun-if-changed={}", source.display());

        if source.is_file() {
            // Skipped under the escape hatch: that build is for working on the
            // UI, already refuses to install anything, and demanding a signed
            // payload from it would defeat the point of having it.
            if !optional
                && let Err(problem) = check_signature(&source, &manifest)
            {
                missing.push(problem);
            }
            stage(&source, &out_dir.join(item.staged));
        } else if optional {
            fs::write(out_dir.join(item.staged), []).expect("stage an empty payload slot");
        } else {
            missing.push(format!(
                "{} is missing — build it first with `{}` (or set {} to its path)",
                source.display(),
                item.remedy,
                item.env_override
            ));
        }
    }

    for (staged, source) in &seeds {
        println!("cargo::rerun-if-changed={}", source.display());
        if source.is_file() {
            stage(source, &out_dir.join(staged));
        } else {
            // A seed going missing means the repo is broken, not that a build
            // step was skipped, so it is fatal even under the escape hatch.
            missing.push(format!("{} is missing from the source tree", source.display()));
        }
    }

    for line in &missing {
        println!("cargo::error=payload: {line}");
    }
    if !missing.is_empty() {
        println!(
            "cargo::error=the installer embeds everything it writes, so it cannot be built \
             without its payload. Run `cargo build --release` first (it builds launcher and \
             listener), then `cargo build --release -p installer` — or set {OPTIONAL}=1 for a \
             UI-only build that refuses to install."
        );
    }
}

/// The `--release` output folder holding `launcher.exe` and `listener.exe`.
///
/// `OUT_DIR` is `<target>/<profile>/build/<crate>-<hash>/out`, so the target
/// directory is five levels up — which is the only way to find it that survives
/// `CARGO_TARGET_DIR` being set. The workspace-relative guess is the fallback
/// for when that shape ever changes.
fn release_dir(out_dir: &Path, manifest: &Path) -> PathBuf {
    out_dir
        .ancestors()
        .nth(4)
        .map(|target| target.join("release"))
        .filter(|dir| dir.is_dir())
        .unwrap_or_else(|| manifest.join("../target/release"))
}

/// Refuses a payload binary that the listener we are about to embed alongside it
/// would not accept.
///
/// This is the check that makes the whole scheme fail loudly instead of quietly.
/// Signing happens *after* `cargo build` and *before* this crate is built (see
/// `xtask release`), and every way of getting that order wrong — rebuilding the
/// launcher after signing it, running a plain `cargo build --release`, signing
/// with a key no listener trusts — produces an installer that works perfectly,
/// writes a cartridge that looks perfect, and is then silently ignored by every
/// listener on earth. The user's only symptom would be nothing happening.
///
/// So it fails here, at the one moment both halves are in the same room.
fn check_signature(binary: &Path, manifest: &Path) -> Result<(), String> {
    let anchors = trust_anchors(manifest);
    if anchors.is_empty() {
        return Err(format!(
            "no public key to check {} against — expected {}",
            binary.display(),
            manifest.join("../keys/gacasy.pub").display()
        ));
    }

    let bytes = fs::read(binary).map_err(|e| format!("{} could not be read: {e}", binary.display()))?;
    let (payload, signature) = sigblock::split(&bytes);
    let Some(signature) = signature else {
        return Err(format!(
            "{} is not signed — build with `cargo run -p xtask -- release`, which signs \
             the binaries before this crate embeds them",
            binary.display()
        ));
    };
    let signature = minisign_verify::Signature::decode(signature)
        .map_err(|e| format!("{}'s signature is malformed: {e}", binary.display()))?;

    let verified = anchors.iter().any(|key| {
        minisign_verify::PublicKey::from_base64(key)
            .is_ok_and(|key| key.verify(payload, &signature, false).is_ok())
    });
    if verified {
        Ok(())
    } else {
        Err(format!(
            "{} is signed by a key none of keys/*.pub names, so the listener in this same \
             payload would refuse it. Re-sign it with `cargo run -p xtask -- release`",
            binary.display()
        ))
    }
}

/// The public keys in `keys/`, read the same way `listener/build.rs` reads them.
fn trust_anchors(manifest: &Path) -> Vec<String> {
    ["gacasy.pub", "dev.pub"]
        .iter()
        .filter_map(|file| {
            let path = manifest.join("../keys").join(file);
            println!("cargo::rerun-if-changed={}", path.display());
            let text = fs::read_to_string(path).ok()?;
            text.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with("untrusted comment:"))
                .next_back()
                .map(str::to_string)
        })
        .collect()
}

fn exe_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    }
}

/// Copies one payload file into `OUT_DIR`. A failure here is a broken build
/// environment, not a user mistake, so it panics rather than reporting.
fn stage(source: &Path, staged: &Path) {
    fs::copy(source, staged).unwrap_or_else(|e| {
        panic!(
            "failed to stage {} as {}: {e}",
            source.display(),
            staged.display()
        )
    });
}
