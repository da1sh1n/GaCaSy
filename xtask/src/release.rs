// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! Building a release, in the one order that works.
//!
//! Four stages, and the dependencies between them are not the kind cargo can
//! see:
//!
//! 1. build `launcher` and `listener`
//! 2. **sign them** — this rewrites the files cargo just produced
//! 3. build `installer`, whose `build.rs` embeds those now-signed bytes
//! 4. sign the installer
//!
//! Stage 3 has to come after stage 2 or the installer ships an unsigned
//! launcher, which every listener would then refuse — a cartridge that fails
//! silently at the moment someone plugs it in, having built and installed
//! without a single error. That failure is expensive enough, and quiet enough,
//! that the sequence lives in code rather than in a README.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{keys, manifest, sign};

pub fn run(root: &Path) -> Result<(), String> {
    // Before anything is built: a set of programs that disagree about their own
    // compatibility generation is not a release, and finding that out after two
    // link steps helps nobody.
    let project_version = manifest::check(root)?;
    let (_, crates) = manifest::read(root)?;
    let version = |name: &str| {
        crates
            .iter()
            .find(|c| c.name == name)
            .map(|c| c.version.clone())
            .unwrap_or_default()
    };

    let key = keys::secret_key(root)?;
    let release = root.join("target").join("release");

    println!("== building launcher and listener");
    cargo(root, &["build", "--release", "-p", "launcher", "-p", "listener"])?;

    println!("== signing them");
    let launcher = release.join(exe("launcher"));
    let listener = release.join(exe("listener"));
    sign::sign(&launcher, &key, &format!("gacasy-launcher {}", version("launcher")))?;
    sign::sign(&listener, &key, &format!("gacasy-listener {}", version("listener")))?;

    // Deliberately after signing, and deliberately a separate cargo invocation:
    // in one `--workspace` build cargo is free to run the installer's build
    // script while launcher.exe is still linking, since no dependency edge
    // orders them.
    println!("== building the installer around them");
    cargo(root, &["build", "--release", "-p", "installer"])?;

    println!("== signing the installer");
    let installer = release.join(exe("installer"));
    sign::sign(&installer, &key, &format!("gacasy-installer {}", version("installer")))?;

    println!();
    println!("project version {project_version} — these three are compatible with each other:");
    let anchors = keys::anchors(root);
    for path in [&launcher, &listener, &installer] {
        // Verifying what we just signed is not ceremony. It is the only thing
        // that proves the secret key in use actually corresponds to a public key
        // baked into the listener we just built — if it does not, every cartridge
        // from this release would be refused, and this is where we find out.
        let verified = sign::verify(path, &anchors)?;
        println!(
            "  {}  [{}]  {}",
            path.display(),
            verified.anchor,
            verified.comment
        );
    }

    if anchors.iter().all(|a| a.name != "dev") {
        println!();
        println!("Signed with the release key. Ship it.");
    } else {
        println!();
        println!(
            "Note: keys/dev.pub exists, so listeners built here also trust that key. \
             That is expected for local builds and must not be true of anything you publish."
        );
    }
    Ok(())
}

/// Runs cargo, inheriting stdio so its progress and errors reach the terminal
/// unchanged. A build tool that swallows the compiler's output is worse than no
/// build tool.
fn cargo(root: &Path, args: &[&str]) -> Result<(), String> {
    // `CARGO` is set when we were started by cargo, which is the only supported
    // way in; the fallback is for someone running the binary directly.
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(&cargo)
        .args(args)
        .current_dir(root)
        .status()
        .map_err(|e| format!("could not run cargo: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("`cargo {}` failed", args.join(" ")))
    }
}

fn exe(stem: &str) -> PathBuf {
    PathBuf::from(if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    })
}
