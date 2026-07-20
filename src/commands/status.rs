use anyhow::Result;
use colored::*;
use std::path::Path;

use crate::bootc;
use crate::config;
use crate::project;

pub fn run(project_dir: &Path) -> Result<()> {
    let profile = config::load_profile(project_dir)?;
    let name = project::get_project_name(project_dir)?;

    // Display project info
    println!("{} Project: {}", "📦".cyan(), name);
    println!("  Profile: {}", project_dir.join("profile.yaml").display());
    println!("  Base image: {}", profile.base_image);
    if let Some(nv) = &profile.nvidia_base_image {
        println!("  NVIDIA variant: {}", nv);
    }
    println!(
        "  Modules ({}): {}",
        profile.enabled_modules.len(),
        profile.enabled_modules.join(", ")
    );
    println!("  Default tag: {}", profile.default_tag);

    // Local build state
    let image_ref = format!("localhost/{}:{}", name, profile.default_tag);
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
                    println!();
                    println!(
                        "  {} Staged deployment will apply on next boot.",
                        "➜".yellow()
                    );
                }
                None => println!("  Staged:   (none)"),
            }

            if let Some(image) = &status.rollback_image {
                println!("  Rollback: {}", image);
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
            let mod_path = project_dir.join("modules").join(format!("{}.yaml", mod_name));
            let desc = if mod_path.exists() {
                if let Ok(m) = config::load_module(project_dir, mod_name) {
                    m.description
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            if !desc.is_empty() {
                println!("  {} {} — {}", mark, mod_name.cyan(), desc);
            } else {
                println!("  {} {}", mark, mod_name.cyan());
            }
        }
    }

    Ok(())
}
