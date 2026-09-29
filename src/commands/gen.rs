use anyhow::{Context, Result};
use std::path::Path;

use crate::config;
use crate::generator;
use crate::module;
use crate::project;

pub fn run(project_dir: &Path, dry_run: bool, host: Option<String>) -> Result<()> {
    let (profile, active_host) = config::resolve_effective_profile(project_dir, host.as_deref())?;

    // Validate modules
    let modules = module::load_enabled_modules(project_dir, &profile.enabled_modules)?;
    let warnings = module::validate_modules(project_dir, &modules)?;

    if !warnings.is_empty() {
        println!("{} Warnings:", "!".yellow());
        for w in &warnings {
            println!("  {}", w.yellow());
        }
        println!();
    }

    // Generate
    let files = generator::generate(project_dir, &profile, active_host.as_deref())?;

    if dry_run {
        println!("{} Dry-run: would generate:", "~".cyan());
        println!();
        println!("── Containerfile ──");
        println!("{}", files.containerfile);
        println!();
        println!("── build_files/build.sh ──");
        println!("{}", files.build_sh);
        return Ok(());
    }

    // Write generated files
    let build_files_dir = project_dir.join("build_files");
    std::fs::create_dir_all(&build_files_dir)?;

    let containerfile_path = project_dir.join("Containerfile");
    std::fs::write(&containerfile_path, &files.containerfile)
        .with_context(|| format!("Failed to write {}", containerfile_path.display()))?;

    let build_sh_path = build_files_dir.join("build.sh");
    std::fs::write(&build_sh_path, &files.build_sh)
        .with_context(|| format!("Failed to write {}", build_sh_path.display()))?;

    // Make build.sh executable
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&build_sh_path, std::fs::Permissions::from_mode(0o755))?;

    // Stage the project snapshot for `include_project` (no-op otherwise).
    generator::sync_project_snapshot(project_dir, &profile)?;

    let name = project::get_project_name(project_dir)?;
    let cf_lines = files.containerfile.lines().count();
    let bs_lines = files.build_sh.lines().count();

    println!(
        "{} Generated build files for '{}'",
        "✓".green(),
        name
    );
    if let Some(h) = &active_host {
        println!("  Host: {}", h);
    }
    println!(
        "  {}  ({} lines)",
        project_dir.join("Containerfile").display(),
        cf_lines
    );
    println!(
        "  {}  ({} lines)",
        build_sh_path.display(),
        bs_lines
    );
    println!(
        "  {} modules composed: {}",
        modules.len(),
        modules.iter().map(|m| m.name.clone()).collect::<Vec<_>>().join(", ")
    );
    println!();
    match &active_host {
        Some(h) => println!("  Next: run 'dcli-bootc build --host {}' to build the image", h),
        None => println!("  Next: run 'dcli-bootc build' to build the image"),
    }

    Ok(())
}

use colored::*;
