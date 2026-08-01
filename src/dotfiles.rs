//! Host-side dotfiles management.
//!
//! Dotfiles live in the project's `dotfiles/` directory and are symlinked
//! into the invoking user's home (default target `~/.config/<name>`), so
//! changes apps make in ~/.config land in the project and survive image
//! rebuilds. This runs host-side — container builds cannot touch user homes.

use anyhow::{Context, Result};
use colored::*;
use std::path::{Path, PathBuf};

use crate::config::DotfileEntry;

/// Directory (relative to the project) holding the managed dotfiles.
pub const DOTFILES_DIR: &str = "dotfiles";

/// Ensure every declared dotfiles entry is well-formed and its source exists
/// under the project's dotfiles/ directory — fail at gen time instead of
/// halfway through applying.
pub fn validate(project_dir: &Path, entries: &[DotfileEntry]) -> Result<()> {
    for entry in entries {
        if let DotfileEntry::Mapped(map) = entry {
            if map.len() != 1 {
                anyhow::bail!(
                    "Mapped dotfiles entries must have exactly one 'source: target' pair, got {}",
                    map.len()
                );
            }
        }

        let name = entry.source_name();
        if name.is_empty() || name == "." || name.contains('/') {
            anyhow::bail!(
                "Dotfiles entry '{}' is invalid — entries must be a single file or directory name under the project's dotfiles/ directory",
                name
            );
        }

        let target = entry.target();
        if !target.starts_with("~/") && !target.starts_with('/') {
            anyhow::bail!(
                "Dotfiles entry '{}' has target '{}' — targets must start with ~/ or be an absolute path",
                name,
                target
            );
        }

        let src = project_dir.join(DOTFILES_DIR).join(&name);
        if !src.exists() {
            anyhow::bail!(
                "profile.yaml declares dotfiles entry '{}' but '{}' does not exist — put the file/dir in the project's dotfiles/ directory",
                name,
                src.display()
            );
        }
    }
    Ok(())
}

/// Apply the effective dotfiles set: symlink each entry from its target in
/// the invoking user's home into the project's dotfiles/ directory.
///
/// Idempotent: correct symlinks are left untouched, wrong symlinks are
/// re-pointed, and existing real files/dirs are first moved to
/// `<target>.dcli-bak` (numbered when a backup already exists). Failures on
/// individual entries are reported but do not abort the rest.
pub fn apply(project_dir: &Path, entries: &[DotfileEntry]) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }

    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .context("HOME is not set — cannot apply dotfiles")?;

    println!("{} Applying dotfiles ({} entries)", "▶".cyan(), entries.len());

    let mut linked = 0;
    let mut already = 0;
    let mut backed_up = 0;

    for entry in entries {
        let name = entry.source_name();
        let src = project_dir.join(DOTFILES_DIR).join(&name);
        let dst = expand_target(&entry.target(), &home);

        if let Some(parent) = dst.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!(
                    "  {} {}: cannot create {}: {}",
                    "✗".red(),
                    name,
                    parent.display(),
                    e
                );
                continue;
            }
        }

        match std::fs::symlink_metadata(&dst) {
            Ok(meta) if meta.file_type().is_symlink() => {
                let current = std::fs::read_link(&dst).unwrap_or_default();
                if current == src {
                    println!(
                        "  {} {} → {} {}",
                        "✓".green(),
                        dst.display(),
                        src.display(),
                        "(already linked)".dimmed()
                    );
                    already += 1;
                    continue;
                }
                if let Err(e) = std::fs::remove_file(&dst) {
                    eprintln!(
                        "  {} {}: cannot replace symlink (points at {}): {}",
                        "✗".red(),
                        dst.display(),
                        current.display(),
                        e
                    );
                    continue;
                }
                println!(
                    "  {} {} re-pointed from {}",
                    "~".yellow(),
                    dst.display(),
                    current.display()
                );
            }
            Ok(_) => {
                // Real file or directory: back up, then link.
                let backup = backup_path(&dst);
                if let Err(e) = std::fs::rename(&dst, &backup) {
                    eprintln!(
                        "  {} {}: cannot back up to {}: {}",
                        "✗".red(),
                        dst.display(),
                        backup.display(),
                        e
                    );
                    continue;
                }
                backed_up += 1;
                println!(
                    "  {} {} backed up to {}",
                    "~".yellow(),
                    dst.display(),
                    backup.display()
                );
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                eprintln!("  {} {}: {}", "✗".red(), dst.display(), e);
                continue;
            }
        }

        if let Err(e) = std::os::unix::fs::symlink(&src, &dst) {
            eprintln!(
                "  {} {} → {}: {}",
                "✗".red(),
                dst.display(),
                src.display(),
                e
            );
            continue;
        }
        linked += 1;
        println!("  {} {} → {}", "✓".green(), dst.display(), src.display());
    }

    println!(
        "{} Dotfiles: {} linked, {} already linked, {} backed up",
        "✓".green(),
        linked,
        already,
        backed_up
    );
    Ok(())
}

/// Expand a leading `~/` against the given home directory; absolute paths
/// pass through unchanged. (validate() guarantees one of the two forms.)
fn expand_target(target: &str, home: &Path) -> PathBuf {
    match target.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(target),
    }
}

/// Pick a free backup path: `<path>.dcli-bak`, or `<path>.dcli-bak.<n>` when
/// earlier backups already exist.
fn backup_path(dst: &Path) -> PathBuf {
    let base = format!("{}.dcli-bak", dst.display());
    let candidate = PathBuf::from(&base);
    if !path_exists(&candidate) {
        return candidate;
    }
    for n in 1.. {
        let numbered = PathBuf::from(format!("{}.{}", base, n));
        if !path_exists(&numbered) {
            return numbered;
        }
    }
    unreachable!()
}

/// Existence check that does not follow symlinks (a dangling symlink still
/// counts as existing).
fn path_exists(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok()
}
