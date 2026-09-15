use anyhow::{Context, Result};
use std::path::Path;

use crate::config::{PackageManager, Profile, Module, RepoConfig, Scripts, RemoteRpm};
use crate::project;

pub fn run(name: Option<String>, from: Option<String>, force: bool) -> Result<()> {
    // Determine project path
    let project_dir = if let Some(n) = &name {
        let path = Path::new(n);
        if path.exists() && !force {
            anyhow::bail!(
                "Directory '{}' already exists. Use --force to overwrite.",
                n
            );
        }
        std::fs::create_dir_all(path)?;
        path.to_path_buf()
    } else {
        std::env::current_dir()?
    };

    if let Some(from_path) = from {
        import_existing(&project_dir, &from_path, force)?;
    } else {
        scaffold_new(&project_dir, name.as_deref(), force)?;
    }

    project::create_marker(&project_dir)?;

    let image_name = project_dir
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "my-image".to_string());

    project::register_project(&project_dir, &image_name, true)?;

    println!(
        "{} Initialized bootc project '{}' at {}",
        "✓".green(),
        image_name,
        project_dir.display()
    );
    println!("  Next: edit profile.yaml and modules/*.yaml, then run 'dcli-bootc gen'");

    Ok(())
}

fn scaffold_new(project_dir: &Path, _name: Option<&str>, _force: bool) -> Result<()> {
    let image_name = project_dir
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "my-image".to_string());

    // Create directories
    std::fs::create_dir_all(project_dir.join("modules"))?;
    std::fs::create_dir_all(project_dir.join("files"))?;
    std::fs::create_dir_all(project_dir.join("build_files"))?;
    std::fs::create_dir_all(project_dir.join("rpms"))?;
    std::fs::create_dir_all(project_dir.join("appimages"))?;
    std::fs::create_dir_all(project_dir.join("dotfiles"))?;

    // Write profile.yaml
    let profile = Profile {
        name: image_name.clone(),
        base_image: "ghcr.io/ublue-os/bazzite:stable".to_string(),
        nvidia_base_image: Some("ghcr.io/ublue-os/bazzite-nvidia-open:stable".to_string()),
        default_tag: "latest".to_string(),
        enabled_modules: vec!["base".to_string()],
        package_manager: PackageManager::default(),
        dotfiles: vec![],
        overlay_module: None,
    };
    let profile_yaml = serde_yaml::to_string(&profile)?;
    std::fs::write(project_dir.join("profile.yaml"), &profile_yaml)?;

    // Write base module
    let base_module = Module {
        description: "Base system packages and configuration".to_string(),
        // dcli-bootc itself, so the tool persists in the image across
        // rebuilds (host-installed packages are lost on rebase).
        packages: vec!["tmux".to_string(), "dcli-bootc".to_string()],
        groups: vec![],
        remove: vec![],
        repos: RepoConfig {
            copr: vec!["theblackdon/dcli-bootc".to_string()],
            ..RepoConfig::default()
        },
        remote_rpms: vec![],
        local_rpms: vec![],
        appimages: vec![],
        flatpak_remotes: vec![],
        flatpaks: vec![],
        remove_flatpaks: vec![],
        files: vec![],
        services: vec!["bootc-fetch-apply-updates.timer".to_string()],
        first_boot_services: vec![],
        scripts: Scripts::default(),
        depends: vec![],
    };
    let module_yaml = serde_yaml::to_string(&base_module)?;
    std::fs::write(project_dir.join("modules").join("base.yaml"), &module_yaml)?;

    // Write .gitignore
    std::fs::write(project_dir.join(".gitignore"), "*.rpm\n*.AppImage\n*.appimage\n")?;

    Ok(())
}

fn import_existing(project_dir: &Path, from_path: &str, _force: bool) -> Result<()> {
    let source = Path::new(from_path);
    if !source.exists() {
        anyhow::bail!("Source path does not exist: {}", from_path);
    }

    let containerfile = source.join("Containerfile");
    if !containerfile.exists() {
        anyhow::bail!("No Containerfile found at {}", from_path);
    }

    let content = std::fs::read_to_string(&containerfile)
        .with_context(|| format!("Failed to read {}", containerfile.display()))?;

    // Basic parsing: extract base image and packages
    let base_image = extract_base_image(&content)
        .unwrap_or("ghcr.io/ublue-os/bazzite:stable");
    let image_name = project_dir
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "my-image".to_string());

    // Create directories
    std::fs::create_dir_all(project_dir.join("modules"))?;
    std::fs::create_dir_all(project_dir.join("files"))?;
    std::fs::create_dir_all(project_dir.join("build_files"))?;
    std::fs::create_dir_all(project_dir.join("rpms"))?;
    std::fs::create_dir_all(project_dir.join("appimages"))?;
    std::fs::create_dir_all(project_dir.join("dotfiles"))?;

    // Write profile
    let profile = Profile {
        name: image_name.clone(),
        base_image: base_image.to_string(),
        nvidia_base_image: None,
        default_tag: "latest".to_string(),
        enabled_modules: vec!["base".to_string()],
        package_manager: PackageManager::default(),
        dotfiles: vec![],
        overlay_module: None,
    };
    std::fs::write(
        project_dir.join("profile.yaml"),
        serde_yaml::to_string(&profile)?,
    )?;

    // Extract packages from dnf install and remove lines
    let install_pkgs = extract_dnf_install(&content);
    let remove_pkgs = extract_dnf_remove(&content);
    let repo_lines = extract_repos(&content);
    let enabled_repos = extract_enabled_repos(&content);

    let base_module = Module {
        description: format!("Imported from {}", containerfile.display()),
        packages: install_pkgs,
        groups: vec![],
        remove: remove_pkgs,
        repos: RepoConfig {
            copr: vec![],
            rpm_urls: vec![],
            apt_lines: vec![],
            enable: enabled_repos,
            import_keys: repo_lines.clone(),
        },
        remote_rpms: extract_remote_rpms(&content),
        local_rpms: vec![],
        appimages: vec![],
        flatpak_remotes: vec![],
        flatpaks: vec![],
        remove_flatpaks: vec![],
        files: extract_copy_files(&content),
        services: extract_services(&content),
        first_boot_services: vec![],
        scripts: Scripts::default(),
        depends: vec![],
    };
    std::fs::write(
        project_dir.join("modules").join("base.yaml"),
        serde_yaml::to_string(&base_module)?,
    )?;

    // Copy files/ directory if it exists
    let source_files = source.join("files");
    if source_files.exists() {
        copy_dir(&source_files, &project_dir.join("files"))?;
    }

    // Write .gitignore
    std::fs::write(project_dir.join(".gitignore"), "*.rpm\n*.AppImage\n*.appimage\n")?;

    println!(
        "{} Imported project from {}",
        "✓".green(),
        from_path
    );
    println!("  Extracted: {} install, {} remove, {} repo keys, {} remote RPMs",
        base_module.packages.len(),
        base_module.remove.len(),
        repo_lines.len(),
        base_module.remote_rpms.len(),
    );
    println!("  Review modules/base.yaml and adjust as needed.");

    Ok(())
}

// ── Basic Containerfile parsers (for --from import) ──

fn extract_base_image(content: &str) -> Option<&str> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("FROM ") || trimmed.starts_with("ARG BASE_IMAGE=") {
            if let Some(val) = trimmed.strip_prefix("ARG BASE_IMAGE=") {
                let val = val.trim().trim_matches('"').trim_matches('\'');
                return Some(val);
            }
            if let Some(val) = trimmed.strip_prefix("ARG BASE_IMAGE=") {
                return Some(val);
            }
        }
    }
    // Try to find FROM line
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(val) = trimmed.strip_prefix("FROM ") {
            if !val.starts_with("scratch") && !val.starts_with("$") {
                return Some(val);
            }
        }
    }
    None
}

fn strip_inline_comment(line: &str) -> &str {
    // Remove inline comments (# ...), but skip lines that start with #
    if let Some(pos) = line.find("#") {
        let before = &line[..pos].trim();
        if !before.is_empty() {
            return before;
        }
    }
    line
}

fn is_rpm_name(s: &str) -> bool {
    if s.is_empty() || s.starts_with('-') || s.starts_with('/') || s.starts_with('$') {
        return false;
    }
    let skip = [
        "dnf", "dnf5", "install", "remove", "yum", "&&", "||", "|", ";",
        "RUN", "COPY", "ARG", "FROM", "ENV", "WORKDIR", "USER", "LABEL",
        "mkdir", "cp", "mv", "rm", "curl", "wget", "echo", "cat", "chmod",
        "chown", "systemctl", "rpmkeys", "sed", "rsync", "grep", "tar",
    ];
    if skip.contains(&s) {
        return false;
    }
    true
}

fn extract_dnf_install(content: &str) -> Vec<String> {
    let mut pkgs = Vec::new();
    let mut in_install = false;
    let mut seen = std::collections::HashSet::new();

    for line in content.lines() {
        let trimmed = strip_inline_comment(line.trim());
        if trimmed.is_empty() {
            continue;
        }

        // Skip comment-only lines and heredoc markers
        let trimmed_line = trimmed.trim();
        if trimmed_line.starts_with('#') || trimmed_line == "EOR" || trimmed_line == "REPO" || trimmed_line == "SVC" {
            continue;
        }

        // Detect the start of a dnf install command
        if (trimmed.contains("dnf install") || trimmed.contains("dnf5 install"))
            && trimmed.contains("-y")
        {
            in_install = true;
            // Collect packages on the opening line
            let after_flag = trimmed.split("-y").last().unwrap_or("");
            let after_flag_clean = strip_inline_comment(after_flag);
            for raw in after_flag_clean.split_whitespace() {
                let p = raw.trim_matches('\\').trim();
                if !p.is_empty() && p != "\\" && is_rpm_name(p) {
                    if seen.insert(p.to_string()) {
                        pkgs.push(p.to_string());
                    }
                }
            }
            continue;
        }

        if in_install {
            // Terminators
            if trimmed.contains("&&") || trimmed.starts_with("RUN ") || trimmed.starts_with("COPY ")
                || trimmed.starts_with("FROM ") || trimmed.starts_with("ARG ")
            {
                in_install = false;
            } else if !trimmed.starts_with('#') && !trimmed.starts_with('-') {
                for raw in trimmed.split_whitespace() {
                    let p = raw.trim_matches('\\').trim();
                    if !p.is_empty() && p != "\\" && is_rpm_name(p) {
                        if seen.insert(p.to_string()) {
                            pkgs.push(p.to_string());
                        }
                    }
                }
            }
        }
    }
    pkgs
}

fn extract_dnf_remove(content: &str) -> Vec<String> {
    let mut pkgs = Vec::new();
    let mut in_remove = false;
    let mut seen = std::collections::HashSet::new();

    for line in content.lines() {
        let trimmed = strip_inline_comment(line.trim());
        if trimmed.is_empty() {
            continue;
        }

        let trimmed_line = trimmed.trim();
        if trimmed_line.starts_with('#') || trimmed_line == "EOR" || trimmed_line == "REPO" {
            continue;
        }

        if (trimmed.contains("dnf remove") || trimmed.contains("dnf5 remove"))
            && trimmed.contains("-y")
        {
            in_remove = true;
            let after_flag = trimmed.split("-y").last().unwrap_or("");
            for raw in after_flag.split_whitespace() {
                let p = raw.trim_matches('\\').trim();
                if !p.is_empty() && p != "\\" && is_rpm_name(p) {
                    if seen.insert(p.to_string()) {
                        pkgs.push(p.to_string());
                    }
                }
            }
            continue;
        }

        if in_remove {
            if trimmed.contains("&&") || trimmed.starts_with("RUN ") || trimmed.starts_with("COPY ")
                || trimmed.starts_with("FROM ") || trimmed.starts_with("ARG ")
            {
                in_remove = false;
            } else if !trimmed.starts_with('#') {
                for raw in trimmed.split_whitespace() {
                    let p = raw.trim_matches('\\').trim();
                    if !p.is_empty() && p != "\\" && is_rpm_name(p) {
                        if seen.insert(p.to_string()) {
                            pkgs.push(p.to_string());
                        }
                    }
                }
            }
        }
    }
    pkgs
}

fn extract_repos(content: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.contains("rpmkeys --import") {
            if let Some(key) = trimmed.split("rpmkeys --import").nth(1) {
                // Strip trailing shell chaining operators (&&, ||, |, ;, \)
                let clean = key
                    .trim()
                    .trim_end_matches(&['&', '|', ';', '\\', ' '][..])
                    .trim()
                    .trim_matches('"')
                    .trim();
                if !clean.is_empty() {
                    keys.push(clean.to_string());
                }
            }
        }
    }
    keys
}

fn extract_enabled_repos(content: &str) -> Vec<String> {
    let mut repos = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        // Match: sed -i 's/enabled=0/enabled=1/' /etc/yum.repos.d/terra.repo
        if let Some(pos) = trimmed.find("/etc/yum.repos.d/") {
            let after = &trimmed[pos + "/etc/yum.repos.d/".len()..];
            if let Some(dot) = after.find(".repo") {
                let name = &after[..dot];
                if !name.is_empty() && !repos.contains(&name.to_string()) {
                    repos.push(name.to_string());
                }
            }
        }
    }
    repos
}

fn extract_copy_files(content: &str) -> Vec<String> {
    let mut dirs = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("COPY ") {
            let src = rest.split_whitespace().next().unwrap_or("");
            if let Some(files_path) = src.strip_prefix("files/") {
                let top = files_path.split('/').next().unwrap_or("");
                if !top.is_empty() && seen.insert(top.to_string()) {
                    dirs.push(top.to_string());
                }
            }
        }
    }
    dirs
}

fn extract_remote_rpms(content: &str) -> Vec<RemoteRpm> {
    let mut rpms = Vec::new();
    for line in content.lines() {
        if line.contains("curl") && line.contains(".rpm") {
            if let Some(url_part) = line.split('"').nth(1) {
                if url_part.ends_with(".rpm") {
                    rpms.push(RemoteRpm {
                        url: url_part.to_string(),
                        sha256: None,
                        filename: None,
                    });
                }
            }
        }
    }
    rpms
}

fn extract_services(content: &str) -> Vec<String> {
    let mut services = Vec::new();
    for line in content.lines() {
        if line.contains("systemctl enable") {
            if let Some(svc) = line.split("systemctl enable").nth(1) {
                for s in svc.split_whitespace() {
                    let s = s.trim();
                    if !s.is_empty() && s != "&&" {
                        services.push(s.to_string());
                    }
                }
            }
        }
    }
    services
}

fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry?;
        let relative = entry.path().strip_prefix(src)?;
        let target = dst.join(relative);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

// Color helper
use colored::*;
use walkdir;
