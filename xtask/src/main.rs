// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

// GaCaSy's build tool. Never shipped; nothing it depends on is linked into
// anything a user runs.
//
// It exists for one reason: a GaCaSy release is a four-stage sequence whose
// ordering constraint cargo cannot see (see release.rs), and the failure from
// getting it wrong is a cartridge that builds cleanly and is then silently
// refused by every listener. That sequence belongs in code.
//
//   keys.rs      where the signing key is, and which public keys a build trusts
//   manifest.rs  the shared-major version contract, checked before building
//   sign.rs      putting a signature into a binary, and reading one back
//   release.rs   the four stages, in order
//
//   cargo run -p xtask -- keygen           make a signing key (once, per machine)
//   cargo run -p xtask -- sign  <exe>...   sign in place
//   cargo run -p xtask -- verify <exe>...  check against keys/*.pub
//   cargo run -p xtask -- release          build and sign everything

mod keys;
mod manifest;
mod release;
mod sign;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
GaCaSy build tool.

  cargo run -p xtask -- release          build and sign launcher, listener, installer
  cargo run -p xtask -- keygen           generate a dev signing key -> keys/dev.pub
  cargo run -p xtask -- keygen --release the one release key -> keys/gacasy.pub (committed)
  cargo run -p xtask -- sign <exe>...    sign in place
  cargo run -p xtask -- verify <exe>...  check against keys/gacasy.pub and keys/dev.pub
  cargo run -p xtask -- version          show the project version and every crate's
";

fn main() -> ExitCode {
    let root = repo_root();
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_default();
    let rest: Vec<PathBuf> = args.map(PathBuf::from).collect();

    let result = match command.as_str() {
        "release" => release::run(&root),
        "keygen" => keys::keygen(&root, rest.iter().any(|a| a == Path::new("--release"))),
        "sign" => sign_all(&root, &rest),
        "verify" => verify_all(&root, &rest),
        "version" => show_versions(&root),
        "" | "help" | "-h" | "--help" => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        other => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn sign_all(root: &Path, paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Err("nothing to sign — pass one or more paths".to_string());
    }
    let key = keys::secret_key(root)?;
    for path in paths {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("gacasy");
        sign::sign(path, &key, &format!("gacasy-{name}"))?;
        println!("signed {}", path.display());
    }
    Ok(())
}

fn verify_all(root: &Path, paths: &[PathBuf]) -> Result<(), String> {
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

fn show_versions(root: &Path) -> Result<(), String> {
    let (project_version, crates) = manifest::read(root)?;
    println!("project version {project_version} — the x in every x.y.z below");
    for c in &crates {
        let ok = if manifest::major(&c.version) == Some(project_version) {
            " "
        } else {
            "!"
        };
        println!("{ok} {:<10} {}", c.name, c.version);
    }
    manifest::check(root).map(|_| ())
}

/// The workspace root: this crate's directory, one level up.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives in the workspace root")
        .to_path_buf()
}
