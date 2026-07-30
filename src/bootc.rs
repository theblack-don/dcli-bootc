//! Shared helpers for interacting with the system's bootc installation.

use anyhow::{Context, Result};
use colored::*;
use serde_json::Value;
use std::process::{Command, Stdio};

/// Prompt the user for a y/N confirmation.
pub fn confirm(prompt: &str) -> bool {
    use std::io::{self, Write};
    print!("{} [y/N] ", prompt);
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().read_line(&mut input).ok();
    let input = input.trim().to_lowercase();
    input == "y" || input == "yes"
}

/// A parsed view of `bootc status --format json` (bootc 1.x Host schema).
#[derive(Debug, Default)]
pub struct BootcStatus {
    /// Image reference tracked by the spec (e.g. "localhost/donzzite:latest")
    pub tracked_image: Option<String>,
    /// Transport tracked by the spec (e.g. "containers-storage")
    pub tracked_transport: Option<String>,
    /// Image reference of the booted deployment
    pub booted_image: Option<String>,
    /// Digest of the booted deployment's image
    pub booted_digest: Option<String>,
    /// Image reference of the staged deployment (applies on next boot)
    pub staged_image: Option<String>,
    /// Digest of the staged deployment's image
    pub staged_digest: Option<String>,
    /// Image reference of the rollback deployment
    pub rollback_image: Option<String>,
    /// Digest of the rollback deployment's image
    pub rollback_digest: Option<String>,
}

/// Run `sudo bootc status --format json` and parse the output.
pub fn status() -> Result<BootcStatus> {
    let output = Command::new("sudo")
        .args(["bootc", "status", "--format", "json"])
        .output()
        .context("Failed to run 'bootc status'")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("bootc status failed: {}", stderr.trim());
    }

    let json: Value = serde_json::from_slice(&output.stdout)
        .context("Failed to parse 'bootc status' JSON output")?;
    Ok(parse_status(&json))
}

fn parse_status(json: &Value) -> BootcStatus {
    let get_str = |pointer: &str| -> Option<String> {
        json.pointer(pointer)
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };

    let deployment = |key: &str| -> (Option<String>, Option<String>) {
        let image = get_str(&format!("/status/{}/image/image/image", key));
        let digest = get_str(&format!("/status/{}/image/imageDigest", key));
        (image, digest)
    };

    let (booted_image, booted_digest) = deployment("booted");
    let (staged_image, staged_digest) = deployment("staged");
    let (rollback_image, rollback_digest) = deployment("rollback");

    BootcStatus {
        tracked_image: get_str("/spec/image/image"),
        tracked_transport: get_str("/spec/image/transport"),
        booted_image,
        booted_digest,
        staged_image,
        staged_digest,
        rollback_image,
        rollback_digest,
    }
}

/// Shorten a sha256 digest for display.
pub fn short_digest(digest: &str) -> String {
    let hex = digest.strip_prefix("sha256:").unwrap_or(digest);
    match hex.char_indices().nth(12) {
        Some((idx, _)) => format!("sha256:{}…", &hex[..idx]),
        None => digest.to_string(),
    }
}

/// Get the digest of an image in root's container storage, if it exists.
pub fn local_image_digest(image_ref: &str) -> Option<String> {
    let output = Command::new("sudo")
        .args([
            "podman",
            "image",
            "inspect",
            image_ref,
            "--format",
            "{{.Digest}}",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let digest = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if digest.is_empty() {
        None
    } else {
        Some(digest)
    }
}

/// Check whether an image exists in root's container storage.
pub fn local_image_exists(image_ref: &str) -> bool {
    Command::new("sudo")
        .args(["podman", "image", "exists", image_ref])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Switch the bootc deployment to a locally built image, then verify and
/// report exactly what was (or was not) staged.
///
/// When the system already tracks this exact image via containers-storage,
/// `bootc switch` is a no-op ("Image specification is unchanged") and the
/// freshly built digest would never be staged — in that case `bootc upgrade`
/// is run instead, which re-reads the tracked image and queues the new build.
///
/// Returns `true` when a genuinely new deployment was staged.
pub fn switch_to_local(image_ref: &str) -> Result<bool> {
    let before = status().ok();

    let already_tracking = before.as_ref().is_some_and(|s| {
        s.tracked_image.as_deref() == Some(image_ref)
            && s.tracked_transport.as_deref() == Some("containers-storage")
    });

    let mut cmd = Command::new("sudo");
    cmd.arg("bootc");
    let action = if already_tracking {
        println!(
            "  {} already tracks {} — staging the new build via 'bootc upgrade'",
            "➜".cyan(),
            image_ref
        );
        println!("  Running: sudo bootc upgrade");
        cmd.arg("upgrade");
        "upgrade"
    } else {
        println!(
            "  Running: sudo bootc switch --transport containers-storage {}",
            image_ref
        );
        cmd.args(["switch", "--transport", "containers-storage"])
            .arg(image_ref);
        "switch"
    };
    println!();

    let rc = cmd
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("Failed to execute bootc {}", action))?;

    if !rc.success() {
        anyhow::bail!("bootc {} failed", action);
    }

    println!();
    Ok(report_staged(image_ref, before))
}

/// Compare bootc status before/after a switch and state plainly what changed.
/// Returns `true` when a new (different) deployment was staged.
fn report_staged(image_ref: &str, before: Option<BootcStatus>) -> bool {
    let after = match status() {
        Ok(s) => s,
        Err(e) => {
            println!(
                "{} Switched, but could not verify the result: {}",
                "!".yellow(),
                e
            );
            println!("  Check with: sudo bootc status");
            // Assume the switch staged something so the user is still
            // offered a reboot.
            return true;
        }
    };

    match (&after.staged_image, &after.staged_digest) {
        (Some(image), digest) => {
            let changed = match (&after.booted_digest, digest) {
                (Some(booted), Some(staged)) => booted != staged,
                _ => true,
            };
            if changed {
                println!("{} New deployment staged:", "✓".green());
                print!("    {}", image);
                if let Some(d) = digest {
                    print!("  ({})", short_digest(d));
                }
                println!();
                if let Some(booted) = &after.booted_digest {
                    println!("    (booted was {})", short_digest(booted));
                }
                true
            } else {
                println!(
                    "{} bootc staged a deployment, but its digest matches the booted one.",
                    "!".yellow()
                );
                println!("    Nothing will change on next boot.");
                false
            }
        }
        _ => {
            let already_tracking = before
                .as_ref()
                .and_then(|b| b.tracked_image.as_deref())
                .map(|tracked| tracked == image_ref)
                .unwrap_or(false);
            if already_tracking {
                println!(
                    "{} No new deployment staged — {} is already the tracked boot image.",
                    "!".yellow(),
                    image_ref
                );
                println!("    If you expected changes, the rebuilt image was identical to");
                println!("    the previous one (the build may not have picked up changes).");
            } else {
                println!(
                    "{} No deployment is staged. Check 'sudo bootc status' for details.",
                    "!".yellow()
                );
            }
            false
        }
    }
}

/// Get the build (creation) date of an image in root's container storage,
/// looked up by digest. Returns a short "YYYY-MM-DD HH:MM" string.
///
/// Local builds all reuse the same tag, so the build date is often the only
/// way to tell two deployments apart. Returns `None` when the image is no
/// longer in storage (e.g. pruned).
pub fn image_created(digest: &str) -> Option<String> {
    let output = Command::new("sudo")
        .args([
            "podman",
            "image",
            "inspect",
            digest,
            "--format",
            "{{.Created}}",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // Format is like "2026-07-27 18:40:11.123456789 +0000 UTC" — keep
    // date + HH:MM for display.
    let mut parts = raw.split_whitespace();
    let date = parts.next()?;
    let time = parts.next()?;
    let hm = time.get(..5).unwrap_or(time);
    Some(format!("{} {}", date, hm))
}

/// Print " · built <date>" for a deployment digest when the image is still
/// in local container storage; print nothing otherwise.
pub fn print_built(digest: &Option<String>) {
    if let Some(d) = digest {
        if let Some(created) = image_created(d) {
            print!(" · built {}", created);
        }
    }
}

/// Queue the previous deployment for the next boot via `sudo bootc rollback`,
/// then verify and report what changed.
///
/// Returns `true` when a rollback was queued — including when the result
/// cannot be verified, so the user is still offered a reboot.
pub fn rollback() -> Result<bool> {
    let before = status().ok();

    println!("  Running: sudo bootc rollback");
    println!();

    let rc = Command::new("sudo")
        .args(["bootc", "rollback"])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("Failed to execute bootc rollback")?;

    if !rc.success() {
        anyhow::bail!("bootc rollback failed");
    }

    println!();
    Ok(report_rollback(before))
}

/// Compare bootc status before/after a rollback and state plainly what
/// changed. Returns `true` when the rollback target is queued for next boot.
fn report_rollback(before: Option<BootcStatus>) -> bool {
    let expected = before.as_ref().and_then(|b| b.rollback_image.as_deref());

    let after = match status() {
        Ok(s) => s,
        Err(e) => {
            println!(
                "{} Rolled back, but could not verify the result: {}",
                "!".yellow(),
                e
            );
            println!("  Check with: sudo bootc status");
            // Assume the rollback queued something so the user is still
            // offered a reboot.
            return true;
        }
    };

    match (&after.staged_image, &after.staged_digest) {
        (Some(image), digest) => {
            let is_expected = expected.is_none() || expected == Some(image.as_str());
            println!("{} Rollback deployment queued for next boot:", "✓".green());
            print!("    {}", image);
            if let Some(d) = digest {
                print!("  ({})", short_digest(d));
            }
            println!();
            if !is_expected {
                println!(
                    "  {} queued image differs from the recorded rollback target",
                    "!".yellow()
                );
            }
        }
        _ => {
            // `bootc rollback` reorders boot entries; on some bootc versions
            // the queued deployment may not show up as `staged`. The command
            // succeeded, so still offer the reboot.
            println!(
                "{} Rollback done — the previous deployment will boot next.",
                "➜".bold().yellow()
            );
            println!("  Verify with: sudo bootc status");
        }
    }
    true
}

/// Reboot handling after a switch.
///
/// * Nothing new staged → no reboot offered (there is nothing to apply).
/// * `--reboot` → reboot automatically after a short countdown.
/// * Otherwise → ask the user whether to reboot right away.
pub fn maybe_reboot(auto_reboot: bool, staged: bool) {
    if !staged {
        if auto_reboot {
            println!();
            println!("  Nothing new to apply — skipping reboot.");
        }
        return;
    }

    if auto_reboot {
        println!();
        println!("  Rebooting in 5 seconds... (Ctrl-C to cancel)");
        std::thread::sleep(std::time::Duration::from_secs(5));
        reboot();
        return;
    }

    println!();
    if confirm("Reboot now to apply the new deployment?") {
        println!("  Rebooting...");
        reboot();
    } else {
        println!(
            "{} The new deployment will apply on next boot.",
            "➜".bold().yellow()
        );
        println!("  Run 'sudo reboot' (or re-run with --reboot) when ready.");
    }
}

fn reboot() {
    Command::new("sudo")
        .arg("reboot")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .ok();
}
