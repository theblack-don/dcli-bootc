use anyhow::{Context, Result};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use crate::bootc;
use crate::config::{OverlayBackend, PackageManager, Profile, Module, RepoConfig, Scripts, RemoteRpm};
use crate::project;

pub fn run(
    name: Option<String>,
    from: Option<String>,
    from_image: bool,
    extend: bool,
    fork: bool,
    template: Option<String>,
    force: bool,
) -> Result<()> {
    if from.is_some() && (from_image || extend || fork || template.is_some()) {
        anyhow::bail!("--from cannot be combined with --from-image/--extend/--fork/--template");
    }
    if from_image && (extend || fork) {
        anyhow::bail!("--from-image cannot be combined with --extend/--fork");
    }
    if extend && fork {
        anyhow::bail!("--extend cannot be combined with --fork");
    }
    if template.is_some() && (from_image || extend) {
        anyhow::bail!("--template cannot be combined with --from-image/--extend");
    }

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
    } else if from_image || extend {
        scaffold_from_image(&project_dir, name.as_deref(), force)?;
    } else if fork || template.is_some() {
        copy_from_template(&project_dir, template.as_deref(), force)?;
    } else if project::is_kineticos() {
        // On kineticos, let the user choose how to use the image. Interactive
        // only; scripts get the safer upstream-tracking default (extend).
        if std::io::stdin().is_terminal() {
            if prompt_extend() {
                scaffold_from_image(&project_dir, name.as_deref(), force)?;
            } else {
                copy_from_kineticos(&project_dir, name.as_deref(), force)?;
            }
        } else {
            scaffold_from_image(&project_dir, name.as_deref(), force)?;
        }
    } else {
        scaffold_new(&project_dir, name.as_deref(), force)?;
    }

    let image_name = project_dir
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "my-image".to_string());

    // Keep profile.name in sync with the project directory without reformatting
    // (a serde round-trip would strip the template's comments).
    set_profile_name(&project_dir, &image_name)?;

    project::create_marker(&project_dir)?;
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
        overlay_backend: OverlayBackend::default(),
        include_project: false,
        kernel: None,
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
        user_flatpak_remotes: vec![],
        user_flatpaks: vec![],
        user_remove_flatpaks: vec![],
        files: vec![],
        services: vec!["bootc-fetch-apply-updates.timer".to_string()],
        first_boot_services: vec![],
        scripts: Scripts::default(),
        depends: vec![],
    };
    let module_yaml = serde_yaml::to_string(&base_module)?;
    std::fs::write(project_dir.join("modules").join("base.yaml"), &module_yaml)?;

    // Write .gitignore
    std::fs::write(
        project_dir.join(".gitignore"),
        "*.rpm\n*.AppImage\n*.appimage\nbuild_files/project/\n",
    )?;

    Ok(())
}

/// Copy a kineticos project template into `project_dir`, then snapshot the
/// live overlay into the project's overlay module when one is configured.
fn copy_template(project_dir: &Path, template: &Path, force: bool) -> Result<()> {
    if !template.join("profile.yaml").is_file() {
        anyhow::bail!(
            "No dcli-bootc project template (profile.yaml) found at {}",
            template.display()
        );
    }
    if project_dir.join("profile.yaml").exists() && !force {
        anyhow::bail!(
            "{} already contains a project (profile.yaml). Use --force to overwrite.",
            project_dir.display()
        );
    }

    println!(
        "{} Copying kineticos template from {}",
        "▶".cyan(),
        template.display()
    );
    copy_dir(template, project_dir)?;

    // The copy is a new project: drop the template's marker and VCS metadata so
    // it is not tied to the upstream repository.
    let _ = std::fs::remove_file(project_dir.join(".dcli-bootc"));
    if project_dir != template {
        let _ = std::fs::remove_dir_all(project_dir.join(".git"));
    }
    for dir in ["modules", "files", "build_files", "rpms", "appimages", "dotfiles"] {
        std::fs::create_dir_all(project_dir.join(dir))?;
    }
    if !project_dir.join(".gitignore").exists() {
        std::fs::write(
            project_dir.join(".gitignore"),
            "*.rpm\n*.AppImage\n*.appimage\noutput/\nbuild_files/project/\n",
        )?;
    }

    seed_overlay_module(project_dir);
    Ok(())
}

/// Resolve the template directory to copy: an explicit `--template` wins,
/// otherwise the built-in kineticos location when it contains a project.
fn resolve_template(explicit: Option<&str>) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return Some(PathBuf::from(path));
    }
    project::kineticos_template_dir().filter(|d| d.join("profile.yaml").is_file())
}

/// Explicit `--fork` / `--template`: copy a template, or explain what is missing.
fn copy_from_template(project_dir: &Path, template: Option<&str>, force: bool) -> Result<()> {
    match resolve_template(template) {
        Some(dir) => copy_template(project_dir, &dir, force),
        None => anyhow::bail!(
            "no project template found — looked at /usr/share/kineticos/project and ~/.config/kineticos; pass --template <dir>"
        ),
    }
}

/// The user chose Fork interactively: copy the kineticos template, falling back
/// to reconstruction if the image does not carry one.
fn copy_from_kineticos(project_dir: &Path, name: Option<&str>, force: bool) -> Result<()> {
    match resolve_template(None) {
        Some(dir) => copy_template(project_dir, &dir, force),
        None => {
            println!(
                "{} No kineticos project template at /usr/share/kineticos/project or ~/.config/kineticos — reconstructing from the running image.",
                "!".yellow()
            );
            scaffold_from_image(project_dir, name, force)
        }
    }
}

/// Ask whether to extend (layer) or fork (copy). Defaults to extend.
fn prompt_extend() -> bool {
    use std::io::Write;
    println!("How do you want to use the kineticos image?");
    println!("  1) Extend  add your own packages/modules on top; kineticos keeps");
    println!("             managing the base and its updates flow through it. (default)");
    println!("  2) Fork    copy the full kineticos recipe so you can change anything;");
    println!("             you take over upstream updates.");
    print!("Choose [1]: ");
    std::io::stdout().flush().ok();
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();
    !matches!(input.trim(), "2" | "f" | "fork")
}

/// Reconstruct a project that layers on the running booted image: `base_image`
/// is the image the system tracks, so upstream kineticos updates flow through
/// the base. Seeds the overlay module from the live overlay.
fn scaffold_from_image(project_dir: &Path, name: Option<&str>, force: bool) -> Result<()> {
    if project_dir.join("profile.yaml").exists() && !force {
        anyhow::bail!(
            "{} already contains a project (profile.yaml). Use --force to overwrite.",
            project_dir.display()
        );
    }
    scaffold_new(project_dir, name, force)?;

    let mut profile = crate::config::load_profile(project_dir)?;
    if let Some(base) = bootc_image_ref() {
        profile.base_image = base;
        profile.nvidia_base_image = None;
    }
    if project::is_kineticos() {
        profile.overlay_module = Some("overlay-packages".to_string());
        profile.overlay_backend = OverlayBackend::Kinpkg;
    }
    let has_overlay = profile.overlay_module.is_some();
    std::fs::write(
        project_dir.join("profile.yaml"),
        serde_yaml::to_string(&profile)?,
    )?;

    if has_overlay {
        seed_overlay_module(project_dir);
    }
    Ok(())
}

/// Best-effort snapshot of the live overlay into the project's overlay module.
/// A missing backend (e.g. the project was copied on another machine) only
/// prints a note; it can be seeded later with `dcli-bootc overlay import`.
fn seed_overlay_module(project_dir: &Path) {
    let configured = crate::config::load_profile(project_dir)
        .map(|p| p.overlay_module.is_some())
        .unwrap_or(false);
    if !configured {
        return;
    }
    if let Err(e) = crate::commands::overlay::import(project_dir, false, true, true, None) {
        println!(
            "  {} Could not record the live overlay in the project: {:#}",
            "!".yellow(),
            e
        );
    }
}

/// The pullable image reference the running system tracks, falling back to the
/// booted image. Strips any ostree transport prefix and digest so the result
/// can be used as a `FROM` for a fork built on another machine.
fn bootc_image_ref() -> Option<String> {
    let status = bootc::status().ok()?;
    let raw = status.tracked_image.or(status.booted_image)?;
    let mut s = raw.as_str();
    for prefix in [
        "ostree-unverified-registry:",
        "ostree-unverified-image:",
        "ostree-image-signed:",
        "ostree-remote-registry:",
        "ostree-remote-image:",
        "containers-storage:",
        "docker://",
    ] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest;
            break;
        }
    }
    let s = s.split('@').next().unwrap_or(s);
    (!s.is_empty()).then(|| s.to_string())
}

/// Replace the top-level `name:` line in profile.yaml, preserving comments and
/// formatting (a full serde round-trip would drop the template's comments).
fn set_profile_name(project_dir: &Path, name: &str) -> Result<()> {
    let path = project_dir.join("profile.yaml");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    let mut out = String::with_capacity(content.len());
    let mut replaced = false;
    for line in content.lines() {
        if !replaced && line.starts_with("name:") {
            out.push_str(&format!("name: {}", name));
            replaced = true;
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    if replaced {
        std::fs::write(&path, out)
            .with_context(|| format!("Failed to update {}", path.display()))?;
    }
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
        overlay_backend: OverlayBackend::default(),
        include_project: false,
        kernel: None,
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
        user_flatpak_remotes: vec![],
        user_flatpaks: vec![],
        user_remove_flatpaks: vec![],
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
    std::fs::write(
        project_dir.join(".gitignore"),
        "*.rpm\n*.AppImage\n*.appimage\nbuild_files/project/\n",
    )?;

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
                    // Skip shell glue and systemctl flags (e.g. --force).
                    if !s.is_empty() && s != "&&" && !s.starts_with('-') {
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
