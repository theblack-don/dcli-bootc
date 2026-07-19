use anyhow::{Context, Result};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::config;

pub fn run(project_dir: &Path, tag: Option<String>, force: bool, auto_reboot: bool) -> Result<()> {
    let profile = config::load_profile(project_dir)?;

    let target_tag = tag.unwrap_or_else(|| profile.default_tag.clone());
    let full_image = format!("{}:{}", profile.base_image, target_tag);

    println!(
        "{} Updating to {}",
        "▶".cyan(),
        full_image
    );
    println!();

    // ── Pull the image ──
    println!("  Pulling {} ...", full_image);
    let status = Command::new("sudo")
        .arg("podman")
        .arg("pull")
        .arg(&full_image)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("Failed to pull image")?;

    if !status.success() {
        eprintln!("{} Pull failed", "✗".red());
        std::process::exit(1);
    }

    println!("{} Pulled {}", "✓".green(), full_image);

    // ── Bootc switch ──
    println!("  Switching deployment...");
    let status = Command::new("sudo")
        .arg("bootc")
        .arg("switch")
        .arg(&full_image)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("Failed to execute bootc switch")?;

    if !status.success() {
        eprintln!("{} bootc switch failed", "✗".red());
        std::process::exit(1);
    }

    println!("{} Switched to {}", "✓".green(), full_image);

    if auto_reboot {
        println!("  Rebooting in 5 seconds...");
        std::thread::sleep(std::time::Duration::from_secs(5));
        Command::new("sudo")
            .arg("reboot")
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .ok();
    } else if force || confirm("Reboot now?") {
        println!("  Rebooting...");
        Command::new("sudo")
            .arg("reboot")
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .ok();
    } else {
        println!("  Run 'sudo reboot' to apply the update.");
    }

    Ok(())
}

fn confirm(prompt: &str) -> bool {
    use std::io::{self, Write};
    print!("{} [y/N] ", prompt);
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().read_line(&mut input).ok();
    let input = input.trim().to_lowercase();
    input == "y" || input == "yes"
}

use colored::*;
