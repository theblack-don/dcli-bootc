use anyhow::Result;
use std::path::Path;

use crate::config;
use crate::dotfiles;

/// Apply the effective dotfiles set (profile merged with the active host)
/// into the invoking user's home, without building the image.
pub fn run(project_dir: &Path, host: Option<String>) -> Result<()> {
    let (profile, active_host) = config::resolve_effective_profile(project_dir, host.as_deref())?;

    if profile.dotfiles.is_empty() {
        match &active_host {
            Some(h) => println!(
                "No dotfiles configured (profile.yaml has no dotfiles entries; host '{}' adds none).",
                h
            ),
            None => println!("No dotfiles configured in profile.yaml."),
        }
        return Ok(());
    }

    if let Some(h) = &active_host {
        println!("Host: {} (dotfiles shown are the effective set)", h);
    }

    dotfiles::validate(project_dir, &profile.dotfiles)?;
    dotfiles::apply(project_dir, &profile.dotfiles)
}
