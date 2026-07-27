// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 da1sh1n
// This file is part of GaCaSy, licensed under the GNU General Public License
// v3.0 or later. GaCaSy comes with ABSOLUTELY NO WARRANTY. See the LICENSE file
// or <https://www.gnu.org/licenses/> for details.

//! The version contract, checked before anything is built.
//!
//! `x.y.z`, where **x is shared by every crate** and y and z belong to each one
//! alone. x means "the way these programs talk to each other"; a launcher that
//! gains a feature moves its own y and leaves everyone else's version alone.
//!
//! The listener enforces this at runtime, by asking a verified launcher for its
//! version and refusing one whose x differs. That check is worth nothing if the
//! programs ship with drifted majors in the first place, so [`check`] runs
//! before `xtask release` builds anything — a release whose pieces disagree
//! about their own compatibility generation should not reach a disk.

use std::fs;
use std::path::Path;

pub struct Crate {
    pub name: String,
    pub version: String,
}

/// The project version from `[workspace.metadata.gacasy]`, and every member's.
pub fn read(root: &Path) -> Result<(u64, Vec<Crate>), String> {
    let text = read_manifest(&root.join("Cargo.toml"))?;
    let table: toml::Table = text
        .parse()
        .map_err(|e| format!("the workspace Cargo.toml is not valid TOML: {e}"))?;

    let workspace = table
        .get("workspace")
        .and_then(|v| v.as_table())
        .ok_or("the root Cargo.toml has no [workspace]")?;

    let project_version = workspace
        .get("metadata")
        .and_then(|v| v.get("gacasy"))
        .and_then(|v| v.get("project_version"))
        .and_then(|v| v.as_integer())
        .ok_or(
            "the root Cargo.toml has no [workspace.metadata.gacasy] project_version — \
             it is the declared truth every crate's major has to match",
        )?;

    let members = workspace
        .get("members")
        .and_then(|v| v.as_array())
        .ok_or("the root Cargo.toml lists no workspace members")?;

    let crates = members
        .iter()
        .filter_map(|m| m.as_str())
        .map(|member| {
            let text = read_manifest(&root.join(member).join("Cargo.toml"))?;
            let table: toml::Table = text
                .parse()
                .map_err(|e| format!("{member}/Cargo.toml is not valid TOML: {e}"))?;
            let package = table
                .get("package")
                .ok_or_else(|| format!("{member}/Cargo.toml has no [package]"))?;
            Ok(Crate {
                name: package
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(member)
                    .to_string(),
                version: package
                    .get("version")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| format!("{member}/Cargo.toml has no version"))?
                    .to_string(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok((project_version as u64, crates))
}

/// Fails when any crate's major differs from the declared project version.
///
/// Reports *every* mismatch rather than the first, because the fix is usually
/// "these three are behind", and finding that out one rebuild at a time is a
/// waste of a release.
pub fn check(root: &Path) -> Result<u64, String> {
    let (project_version, crates) = read(root)?;

    let drifted: Vec<_> = crates
        .iter()
        .filter(|c| major(&c.version) != Some(project_version))
        .map(|c| format!("  {} is {} — expected {project_version}.y.z", c.name, c.version))
        .collect();

    if !drifted.is_empty() {
        return Err(format!(
            "these crates disagree with the project version ({project_version}):\n{}\n\
             Either bump them to {project_version}.y.z, or bump project_version in the root \
             Cargo.toml if this really is a new compatibility generation.",
            drifted.join("\n")
        ));
    }
    Ok(project_version)
}

/// The `x` of an `x.y.z`.
pub fn major(version: &str) -> Option<u64> {
    version.trim().split('.').next()?.parse().ok()
}

fn read_manifest(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("could not read {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_root() -> &'static Path {
        // xtask/ -> the workspace root.
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask has a parent")
    }

    #[test]
    fn this_workspace_agrees_with_itself() {
        // The whole point, asserted against the real manifests: if someone bumps
        // one crate's major and not the others, this fails in `cargo test`
        // rather than at release time.
        check(repo_root()).unwrap_or_else(|e| panic!("{e}"));
    }

    #[test]
    fn every_crate_is_accounted_for() {
        let (_, crates) = read(repo_root()).expect("read the manifests");
        let names: Vec<_> = crates.iter().map(|c| c.name.as_str()).collect();
        for expected in ["launcher", "listener", "installer", "sigblock", "xtask"] {
            assert!(names.contains(&expected), "{expected} is not a member");
        }
    }

    #[test]
    fn reads_the_leading_number() {
        assert_eq!(major("0.2.0"), Some(0));
        assert_eq!(major("12.0.1"), Some(12));
        assert_eq!(major(" 1.0.0 "), Some(1));
        assert_eq!(major("not-a-version"), None);
        assert_eq!(major(""), None);
    }
}
