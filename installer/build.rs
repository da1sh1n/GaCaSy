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
//!
//! The two binaries are **compressed** on the way in — an exe is about half its
//! size deflated, and this installer is the one file a user downloads. They come
//! back out byte-identical, which they have to: the signature inside
//! `launcher.exe` is what makes a cartridge a cartridge, and it is checked here
//! against the uncompressed original before any of this happens.

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
    /// Constant written into `sizes.rs` holding the size this unpacks back to.
    /// The free-space check needs it, and the compressed length does not answer
    /// the question "will this fit on the drive".
    size_const: &'static str,
    /// Env var that overrides where it comes from, for builds that put
    /// artifacts somewhere unusual.
    env_override: &'static str,
    /// Human-readable instruction printed when it is missing.
    remedy: &'static str,
    /// The role its signature has to declare. Checked as well as the signature
    /// itself, so a build cannot embed a signed *listener* in the slot the
    /// cartridge's launcher comes out of — both are genuine, and only one of
    /// them is the right program for the job.
    role: &'static str,
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
                staged: "launcher.exe.z",
                size_const: "LAUNCHER_BYTES",
                env_override: "GACASY_LAUNCHER_EXE",
                remedy: "cargo build --release -p launcher",
                role: trust::LAUNCHER_ROLE,
            },
            release.join(exe_name("launcher")),
        ),
        (
            Item {
                staged: "listener.exe.z",
                size_const: "LISTENER_BYTES",
                env_override: "GACASY_LISTENER_EXE",
                remedy: "cargo build --release -p listener",
                role: trust::LISTENER_ROLE,
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
    let mut sizes = String::new();

    for (item, default) in &binaries {
        println!("cargo::rerun-if-env-changed={}", item.env_override);
        let source = env::var_os(item.env_override)
            .map(PathBuf::from)
            .unwrap_or_else(|| default.clone());
        println!("cargo::rerun-if-changed={}", source.display());

        let mut unpacked = 0;
        if source.is_file() {
            // Skipped under the escape hatch: that build is for working on the
            // UI, already refuses to install anything, and demanding a signed
            // payload from it would defeat the point of having it.
            if !optional
                && let Err(problem) = check_signature(&source, &manifest, item.role)
            {
                missing.push(problem);
            }
            // Signature checked above, against these same bytes, before they are
            // packed. Nothing downstream can verify a compressed launcher.
            unpacked = squeeze(&source, &out_dir.join(item.staged));
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
        sizes.push_str(&format!(
            "pub const {}: u64 = {unpacked};\n",
            item.size_const
        ));
    }
    fs::write(out_dir.join("sizes.rs"), sizes).expect("write the unpacked payload sizes");

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

    stage_launcher_version(&out_dir, &manifest);
    stage_trust_anchors(&out_dir, &manifest);
}

/// Writes `LAUNCHER_VERSION`, read from `../launcher/Cargo.toml`'s `[package].version`
/// — not from the built exe. That manifest field is the one place the launcher's
/// version is declared, so it is the only place the installer should read it from.
///
/// Unlike the binary payload above, there is no escape hatch here: this reads
/// source, not a build artifact, so a missing or malformed manifest means the repo
/// itself is broken and the build should fail loudly rather than stage a fallback.
fn stage_launcher_version(out_dir: &Path, manifest: &Path) {
    let path = manifest.join("../launcher/Cargo.toml");
    println!("cargo::rerun-if-changed={}", path.display());

    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
    let table: toml::Table = text
        .parse()
        .unwrap_or_else(|e| panic!("{} is not valid TOML: {e}", path.display()));
    let version = table
        .get("package")
        .and_then(|v| v.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("{} has no [package].version", path.display()));

    fs::write(
        out_dir.join("launcher-version.rs"),
        format!("pub const LAUNCHER_VERSION: &str = {version:?};\n"),
    )
    .expect("write the bundled launcher's version");
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
fn check_signature(binary: &Path, manifest: &Path, role: &str) -> Result<(), String> {
    let anchors = trust_anchors(manifest);
    if anchors.is_empty() {
        return Err(format!(
            "no public key to check {} against — expected {}",
            binary.display(),
            manifest.join("../keys/gacasy.pub").display()
        ));
    }
    let anchors: Vec<trust::Anchor> = anchors
        .iter()
        .map(|(name, base64)| trust::Anchor {
            name,
            base64,
        })
        .collect();

    let bytes = fs::read(binary).map_err(|e| format!("{} could not be read: {e}", binary.display()))?;

    // The same call the listener will make against the same bytes. That is the
    // point of routing this through `trust` rather than open-coding a verify
    // here: a build-time check that agreed with itself but not with the shipped
    // listener would bless a payload that is then silently ignored on every PC.
    match trust::attest(&bytes, &anchors, role) {
        Ok(_) => Ok(()),
        Err(trust::Refusal::Unsigned) => Err(format!(
            "{} is not signed — build with `cargo run -p xtask -- release`, which signs \
             the binaries before this crate embeds them",
            binary.display()
        )),
        Err(trust::Refusal::Untrusted) => Err(format!(
            "{} is signed by a key none of keys/*.pub names, so the listener in this same \
             payload would refuse it. Re-sign it with `cargo run -p xtask -- release`",
            binary.display()
        )),
        Err(trust::Refusal::WrongRole { expected, found }) => Err(format!(
            "{} is a signed {found}, but this payload slot needs a {expected}. \
             The build put the wrong binary in the wrong place",
            binary.display()
        )),
        Err(reason) => Err(format!("{}: {reason}", binary.display())),
    }
}

/// The public keys in `keys/`, read the same way `listener/build.rs` reads them.
fn trust_anchors(manifest: &Path) -> Vec<(String, String)> {
    [("release", "gacasy.pub"), ("dev", "dev.pub")]
        .iter()
        .filter_map(|(name, file)| {
            let path = manifest.join("../keys").join(file);
            println!("cargo::rerun-if-changed={}", path.display());
            let text = fs::read_to_string(path).ok()?;
            let key = text
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with("untrusted comment:"))
                .next_back()?;
            Some((name.to_string(), key.to_string()))
        })
        .collect()
}

/// Writes the same `ANCHORS` constant `listener/build.rs` writes, for the
/// installer to verify a cartridge's existing `launcher.exe` at runtime.
///
/// The installer needs this for the same reason the listener does and for one
/// more: it is the program that decides whether a drive is already a cartridge,
/// and it used to decide that by looking for a *file name* and then running it.
/// Compiled in rather than read from `keys/` at runtime, because an anchor the
/// user can edit is an anchor an attacker can edit.
///
/// Absent keys are not fatal here the way they are for the listener: a
/// payload-less UI build (`GACASY_PAYLOAD_OPTIONAL`) has nothing to verify and
/// already refuses to install anything. An empty list means nothing verifies,
/// which is the safe direction.
fn stage_trust_anchors(out_dir: &Path, manifest: &Path) {
    let mut rust = String::from(
        "// Generated by build.rs. The public keys this binary trusts, in the\n\
         // order it tries them. Not configurable at runtime, by design.\n\
         pub const ANCHORS: &[Anchor] = &[\n",
    );
    for (name, key) in trust_anchors(manifest) {
        rust.push_str(&format!(
            "    Anchor {{ name: {name:?}, base64: {key:?} }},\n"
        ));
    }
    rust.push_str("];\n");
    fs::write(out_dir.join("trust_anchors.rs"), rust).expect("write trust_anchors.rs");
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
/// Compresses one payload binary into `OUT_DIR`, returning its original size.
///
/// zlib rather than raw deflate, for the six bytes of header and the Adler-32
/// trailer. These are the bytes a cartridge's identity is made of — see
/// payload.rs — so a checksum over them is worth more than the six bytes it
/// costs. The exes go to roughly half their size.
fn squeeze(source: &Path, staged: &Path) -> u64 {
    use std::io::Write as _;

    let bytes = fs::read(source)
        .unwrap_or_else(|e| panic!("failed to read {} for packing: {e}", source.display()));

    let mut encoder =
        flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    encoder
        .write_all(&bytes)
        .and_then(|()| encoder.finish())
        .map(|packed| fs::write(staged, packed))
        .unwrap_or_else(|e| panic!("failed to pack {}: {e}", source.display()))
        .unwrap_or_else(|e| panic!("failed to stage {}: {e}", staged.display()));

    bytes.len() as u64
}

fn stage(source: &Path, staged: &Path) {
    fs::copy(source, staged).unwrap_or_else(|e| {
        panic!(
            "failed to stage {} as {}: {e}",
            source.display(),
            staged.display()
        )
    });
}
