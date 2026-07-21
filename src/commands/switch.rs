use anyhow::Result;
use colored::*;
use std::path::Path;

use crate::bootc;
use crate::config;
use crate::project;

pub fn run(
    project_dir: &Path,
    tag: Option<String>,
    force: bool,
    auto_reboot: bool,
    host: Option<String>,
) -> Result<()> {
    let (profile, active_host) = config::resolve_effective_profile(project_dir, host.as_deref())?;
    // Explicit --tag wins; otherwise a host build is tagged with the host name
    let image_tag = tag
        .or_else(|| active_host.clone())
        .unwrap_or_else(|| profile.default_tag.clone());
    let image_name = project::get_project_name(project_dir)?;
    let image_ref = format!("localhost/{}:{}", image_name, image_tag);

    if !bootc::local_image_exists(&image_ref) {
        let hint = match &active_host {
            Some(h) => format!(" --host {}", h),
            None if image_tag != profile.default_tag => format!(" --tag {}", image_tag),
            None => String::new(),
        };
        anyhow::bail!(
            "Image {} not found in local container storage.\n  Build it first with: dcli-bootc build{}",
            image_ref,
            hint
        );
    }

    println!("{} Switch bootc deployment", "▶".cyan());
    println!("  Target: {}", image_ref);
    if let Some(digest) = bootc::local_image_digest(&image_ref) {
        println!("  Digest: {}", bootc::short_digest(&digest));
    }
    if let Ok(status) = bootc::status() {
        if let Some(tracked) = &status.tracked_image {
            match &status.tracked_transport {
                Some(transport) => println!("  Currently tracking: {}:{}", transport, tracked),
                None => println!("  Currently tracking: {}", tracked),
            }
        }
        if let Some(booted) = &status.booted_digest {
            println!("  Booted digest: {}", bootc::short_digest(booted));
        }
    }
    println!();

    if force || bootc::confirm("Switch bootc deployment to this image?") {
        let staged = bootc::switch_to_local(&image_ref)?;
        bootc::maybe_reboot(auto_reboot, staged);
    } else {
        println!("  Aborted.");
    }

    Ok(())
}
