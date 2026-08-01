use anyhow::Result;
use colored::*;
use std::path::Path;

use crate::bootc;
use crate::config;
use crate::project;

pub fn run(project_dir: &Path, host: Option<String>) -> Result<()> {
    let (profile, active_host) = config::resolve_effective_profile(project_dir, host.as_deref())?;
    let name = project::get_project_name(project_dir)?;

    // Display project info
    println!("{} Project: {}", "📦".cyan(), name);
    let profile_display = config::profile_path(project_dir)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| project_dir.join("profile.yaml").display().to_string());
    println!("  Profile: {}", profile_display);
    match &active_host {
        Some(h) => println!("  Host: {} (modules/dotfiles shown are the effective set)", h),
        None => println!("  Host: none (profile only)"),
    }
    println!("  Base image: {}", profile.base_image);
    if let Some(nv) = &profile.nvidia_base_image {
        println!("  NVIDIA variant: {}", nv);
    }
    println!(
        "  Modules ({}): {}",
        profile.enabled_modules.len(),
        profile.enabled_modules.join(", ")
    );
    if !profile.dotfiles.is_empty() {
        let entries: Vec<String> = profile
            .dotfiles
            .iter()
            .map(|d| format!("{} → {}", d.source_name(), d.target()))
            .collect();
        println!("  Dotfiles ({}): {}", entries.len(), entries.join(", "));
    }
    println!("  Default tag: {}", profile.default_tag);

    // Local build state — a host build is tagged with the host name
    let image_tag = active_host.clone().unwrap_or_else(|| profile.default_tag.clone());
    let image_ref = format!("localhost/{}:{}", name, image_tag);
    match bootc::local_image_digest(&image_ref) {
        Some(digest) => println!("  Local image: {} ({})", image_ref, bootc::short_digest(&digest)),
        None => println!("  Local image: {} (not built yet)", image_ref),
    }
    println!();

    // Display bootc status
    println!("{} Bootc Status:", "🖥".cyan());
    match bootc::status() {
        Ok(status) => {
            match (&status.tracked_transport, &status.tracked_image) {
                (Some(transport), Some(image)) => {
                    println!("  Tracking: {}:{}", transport, image)
                }
                (None, Some(image)) => println!("  Tracking: {}", image),
                _ => println!("  Tracking: (no image source)"),
            }

            match &status.booted_image {
                Some(image) => {
                    print!("  Booted:   {}", image);
                    if let Some(d) = &status.booted_digest {
                        print!(" ({})", bootc::short_digest(d));
                    }
                    bootc::print_built(&status.booted_digest);
                    println!();
                }
                None => println!("  Booted:   (not a bootc system?)"),
            }

            match &status.staged_image {
                Some(image) => {
                    print!("  Staged:   {}", image);
                    if let Some(d) = &status.staged_digest {
                        print!(" ({})", bootc::short_digest(d));
                    }
                    bootc::print_built(&status.staged_digest);
                    println!();
                    println!(
                        "  {} Staged deployment will apply on next boot.",
                        "➜".yellow()
                    );
                }
                None => println!("  Staged:   (none)"),
            }

            if let Some(image) = &status.rollback_image {
                print!("  Rollback: {}", image);
                if let Some(d) = &status.rollback_digest {
                    print!(" ({})", bootc::short_digest(d));
                }
                bootc::print_built(&status.rollback_digest);
                println!();
            }

            // Local builds all reuse the same tag, so deployments are only
            // distinguishable by digest — point that out when it matters.
            let tag_shared = [&status.staged_image, &status.rollback_image]
                .into_iter()
                .flatten()
                .any(|i| Some(i) == status.booted_image.as_ref());
            if tag_shared {
                println!(
                    "  {}",
                    "(same tag on multiple deployments — compare digests to tell builds apart)"
                        .dimmed()
                );
            }
        }
        Err(e) => {
            println!("  {} Could not read bootc status: {}", "✗".red(), e);
        }
    }

    // List available modules
    println!();
    let available = config::list_modules(project_dir)?;
    if !available.is_empty() {
        println!("{} Available modules:", "📋".cyan());
        for mod_name in &available {
            let enabled = profile.enabled_modules.contains(mod_name);
            let mark = if enabled { "✓".green() } else { " ".dimmed() };
            let desc = config::load_module(project_dir, mod_name)
                .map(|m| m.description)
                .unwrap_or_default();
            if !desc.is_empty() {
                println!("  {} {} — {}", mark, mod_name.cyan(), desc);
            } else {
                println!("  {} {}", mark, mod_name.cyan());
            }
        }
    }

    // List available hosts
    let hosts = config::list_hosts(project_dir)?;
    if !hosts.is_empty() {
        println!();
        println!("{} Hosts:", "🖥".cyan());
        for host_name in &hosts {
            let is_active = active_host.as_ref() == Some(host_name);
            let mark = if is_active { "✓".green() } else { " ".dimmed() };
            let host_cfg = config::load_host(project_dir, host_name).ok();
            let desc = host_cfg
                .as_ref()
                .and_then(|c| c.description.clone())
                .unwrap_or_default();
            let mut delta: Vec<String> = Vec::new();
            if let Some(cfg) = &host_cfg {
                delta.extend(cfg.modules.iter().map(|m| format!("+{}", m)));
                delta.extend(cfg.exclude_modules.iter().map(|m| format!("-{}", m)));
            }
            let delta_str = if delta.is_empty() {
                String::new()
            } else {
                format!("  [{}]", delta.join(", "))
            };
            if desc.is_empty() {
                println!("  {} {}{}", mark, host_name.cyan(), delta_str);
            } else {
                println!("  {} {} — {}{}", mark, host_name.cyan(), desc, delta_str);
            }
        }
    }

    Ok(())
}
