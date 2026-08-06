// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of Romzeta, licensed under the GNU General Public License
// v3.0 or later. Romzeta comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

// Romzeta's build tool. Never shipped; nothing it depends on is linked into
// anything a user runs.
//
// It exists because a release is a four-stage sequence whose ordering
// constraint cargo cannot see (release.rs), and getting it wrong produces a
// cartridge that builds cleanly and is then refused by every listener.
//
//   keys.rs      where the signing key is, and which public keys a build trusts
//   manifest.rs  the shared-major version contract, checked before building
//   sign.rs      putting a signature into a binary, and reading one back
//   release.rs   the four stages, in order

// Functions are camelCase in this project while variables stay snake_case,
// which rustc's default lints object to. Silenced once, at the crate root.
#![allow(non_snake_case)]

mod keys;
mod manifest;
mod release;
mod sign;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

// ########## THE COMMAND LINE ##########

const USAGE: &str = "\
Romzeta build tool.

  cargo run -p xtask -- release          build and sign launcher, listener, installer
  cargo run -p xtask -- keygen           generate a dev signing key -> keys/dev.pub
  cargo run -p xtask -- keygen --release the one release key -> keys/romzeta.pub (committed)
  cargo run -p xtask -- sign <exe>...    sign in place
  cargo run -p xtask -- verify <exe>...  check against keys/romzeta.pub and keys/dev.pub
  cargo run -p xtask -- version          show the project version and every crate's
";

fn main() -> ExitCode {
    let root = repoRoot();
    let mut args = std::env::args().skip(1);
    // `unwrap_or_default` turns "no arguments at all" into the empty string,
    // which the match below already treats as a request for the usage text.
    let command = args.next().unwrap_or_default();
    let rest: Vec<PathBuf> = args.map(PathBuf::from).collect();

    // Every arm hands back the same Result, so the exit code is decided once.
    let result = match command.as_str() {
        "release" => release::run(&root),
        "keygen" => keys::keygen(&root, rest.iter().any(|a| a == Path::new("--release"))),
        "sign" => signAll(&root, &rest),
        "verify" => verifyAll(&root, &rest),
        "version" => showVersions(&root),
        "" | "help" | "-h" | "--help" => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        other => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            // stderr, so `xtask verify … > list.txt` keeps the failure visible.
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

// ========== Commands ==========

/// Signs every path in `paths` in place, taking the role name from each file's
/// own stem. Fails if the list is empty or the signing key cannot be loaded.
fn signAll(root: &Path, paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Err("nothing to sign — pass one or more paths".to_string());
    }
    // Loaded once, outside the loop: an encrypted key would otherwise prompt
    // for its password per file.
    let key = keys::secretKey(root)?;
    for path in paths {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("romzeta");
        sign::sign(path, &key, &format!("romzeta-{name}"))?;
        println!("signed {}", path.display());
    }
    Ok(())
}

/// Verifies every path in `paths` against the repo's public keys, printing one
/// line each. Fails if any of them was refused.
fn verifyAll(root: &Path, paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Err("nothing to verify — pass one or more paths".to_string());
    }
    let anchors = keys::anchors(root);
    // Every path is reported before the first failure decides the exit code:
    // "which of these four is the bad one" is the actual question being asked.
    let mut failed = false;
    for path in paths {
        match sign::verify(path, &anchors) {
            Ok(verified) => println!(
                "ok       {}  [{}]  {}",
                path.display(),
                verified.anchor,
                verified.comment
            ),
            Err(message) => {
                println!("REFUSED  {message}");
                failed = true;
            }
        }
    }
    if failed {
        Err("not everything verified".to_string())
    } else {
        Ok(())
    }
}

/// Prints the project version and every crate's, marking with `!` any whose
/// major has drifted. Fails if one has, via the same check a release runs.
fn showVersions(root: &Path) -> Result<(), String> {
    let (project_version, crates) = manifest::read(root)?;
    println!("project version {project_version} — the x in every x.y.z below");
    for c in &crates {
        let ok = if manifest::major(&c.version) == Some(project_version) {
            " "
        } else {
            "!"
        };
        // `{:<10}` left-pads the name so the versions form a column.
        println!("{ok} {:<10} {}", c.name, c.version);
    }
    // The list is printed first either way, so a drift report shows what drifted.
    manifest::check(root).map(|_| ())
}

/// The workspace root: this crate's own directory, one level up. Resolved from
/// `CARGO_MANIFEST_DIR` at compile time, so it holds wherever xtask is run from.
fn repoRoot() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives in the workspace root")
        .to_path_buf()
}
