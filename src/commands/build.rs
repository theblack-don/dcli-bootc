use anyhow::{Context, Result};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::config;
use crate::project;

pub fn run(project_dir: &Path, tag: Option<String>, nvidia: bool, auto_reboot: bool, force: bool) -> Result<()> {
    let profile = config::load_profile(project_dir)?;

    let base_image = if nvidia {
        profile
            .nvidia_base_image
            .as_deref()
            .unwrap_or(&profile.base_image)
    } else {
        &profile.base_image
    };

    let image_tag = tag.unwrap_or_else(|| profile.default_tag.clone());
    let image_name = project::get_project_name(project_dir)?;

    println!(
        "{} Building {}:{} from {}",
        "▶".cyan(),
        image_name,
        image_tag,
        base_image
    );
    println!();

    // ── podman build (inherits stdin so sudo can prompt) ──
    let status = Command::new("sudo")
        .arg("podman")
        .arg("build")
        .arg("--build-arg")
        .arg(format!("BASE_IMAGE={}", base_image))
        .arg("-t")
        .arg(format!("{}:{}", image_name, image_tag))
        .arg(project_dir)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("Failed to execute podman build")?;

    if !status.success() {
        eprintln!("{} Build failed", "✗".red());
        std::process::exit(1);
    }

    println!();
    println!("{} Build complete", "✓".green());

    // ── Bootc switch prompt ──
    println!();
    if force || confirm("Switch bootc deployment to this image?") {
        println!("  Switching...");
        let status = Command::new("sudo")
            .arg("bootc")
            .arg("switch")
            .arg("--transport")
            .arg("containers-storage")
            .arg(format!("localhost/{}:{}", image_name, image_tag))
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .context("Failed to execute bootc switch")?;

        if !status.success() {
            eprintln!("{} bootc switch failed", "✗".red());
            std::process::exit(1);
        }

        println!("  Deployment switched.");

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
        } else {
            println!("  Run 'sudo reboot' to apply the update.");
        }
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
