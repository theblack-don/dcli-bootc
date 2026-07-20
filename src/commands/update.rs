use anyhow::{Context, Result};
use colored::*;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::commands::build;
use crate::config;

/// Update = pull the latest base image, rebuild the custom image on top of
/// it, and (after confirmation) switch the deployment to the fresh build.
pub fn run(
    project_dir: &Path,
    tag: Option<String>,
    force: bool,
    auto_reboot: bool,
    nvidia: bool,
    cache: bool,
) -> Result<()> {
    let profile = config::load_profile(project_dir)?;

    let base_image = if nvidia {
        profile
            .nvidia_base_image
            .as_deref()
            .unwrap_or(&profile.base_image)
    } else {
        &profile.base_image
    };

    println!("{} Updating base image {}", "▶".cyan(), base_image);
    println!();

    // ── Pull the latest base image ──
    println!("  Pulling {} ...", base_image);
    let status = Command::new("sudo")
        .arg("podman")
        .arg("pull")
        .arg(base_image)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("Failed to pull base image")?;

    if !status.success() {
        eprintln!("{} Pull failed", "✗".red());
        std::process::exit(1);
    }

    println!("{} Pulled {}", "✓".green(), base_image);
    println!();

    // ── Rebuild the custom image on the fresh base, then switch ──
    build::run(project_dir, tag, nvidia, auto_reboot, force, !cache)
}
