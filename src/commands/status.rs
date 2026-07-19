use anyhow::{Context, Result};
use std::path::Path;

use crate::config;
use crate::project;

pub fn run(project_dir: &Path) -> Result<()> {
    let profile = config::load_profile(project_dir)?;
    let name = project::get_project_name(project_dir)?;

    // Parse bootc status
    let bootc_output = std::process::Command::new("sudo")
        .arg("bootc")
        .arg("status")
        .arg("--json")
        .output()
        .context("Failed to run 'bootc status'")?;

    let bootc_json: serde_json::Value = if bootc_output.status.success() {
        let stdout = String::from_utf8_lossy(&bootc_output.stdout);
        serde_json::from_str(&stdout).unwrap_or(serde_json::Value::Null)
    } else {
        serde_json::Value::Null
    };

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
    println!();

    // Display bootc status
    println!("{} Bootc Status:", "🖥".cyan());

    if bootc_json.is_object() {
        if let Some(booted) = bootc_json.get("booted") {
            if let Some(image) = booted.get("image") {
                let image_str = image.as_str().unwrap_or("unknown");
                println!("  Booted image: {}", image_str);
            }
        }
        if let Some(staged) = bootc_json.get("staged") {
            if let Some(image) = staged.get("image") {
                if let Some(image_str) = image.as_str() {
                    if !image_str.is_empty() {
                        println!("  Staged (next boot): {}", image_str);
                    }
                }
            }
        }
        if let Some(rollback) = bootc_json.get("rollback") {
            if let Some(image) = rollback.get("image") {
                if let Some(image_str) = image.as_str() {
                    if !image_str.is_empty() {
                        println!("  Rollback image: {}", image_str);
                    }
                }
            }
        }
        if let Some(status) = bootc_json.get("status") {
            if let Some(s) = status.as_str() {
                println!("  Status: {}", s);
            }
        }
    } else {
        // Fallback: run bootc status without JSON
        let plain = std::process::Command::new("sudo")
            .arg("bootc")
            .arg("status")
            .output()
            .context("Failed to run bootc status")?;
        let stdout = String::from_utf8_lossy(&plain.stdout);
        for line in stdout.lines() {
            println!("  {}", line);
        }
        let stderr = String::from_utf8_lossy(&plain.stderr);
        for line in stderr.lines() {
            if !line.trim().is_empty() {
                println!("  {}", line);
            }
        }
    }

    // List available modules
    println!();
    let available = config::list_modules(project_dir)?;
    if !available.is_empty() {
        println!("{} Available modules:", "📋".cyan());
        for mod_name in &available {
            let enabled = profile.enabled_modules.contains(mod_name);
            let status = if enabled { "✓".green() } else { " ".dimmed() };
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
                println!("  {} {} — {}", status, mod_name.cyan(), desc);
            } else {
                println!("  {} {}", status, mod_name.cyan());
            }
        }
    }

    Ok(())
}

use colored::*;
