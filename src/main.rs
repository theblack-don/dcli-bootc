mod bootc;
mod commands;
mod config;
mod dotfiles;
mod flatpak;
mod generator;
mod hash;
mod module;
mod overlay;
mod plan;
mod project;
mod registry;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "dcli-bootc")]
#[command(version, about = "A declarative bootc image configuration manager")]
struct Cli {
    /// Path to the bootc project (defaults to current dir or registry default)
    #[arg(short, long, global = true)]
    project: Option<String>,

    /// Host to generate/build for (defaults to auto-detected hostname)
    #[arg(long, global = true)]
    host: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new bootc image project
    Init {
        /// Project name (creates ./<name> or uses current directory)
        name: Option<String>,

        /// Import from an existing Containerfile-based project
        #[arg(long)]
        from: Option<String>,

        /// Force overwrite existing files
        #[arg(short, long)]
        force: bool,
    },

    /// Generate Containerfile and build script from YAML modules
    Gen {
        /// Preview what would be generated without writing
        #[arg(short, long)]
        dry_run: bool,
    },

    /// Build the container image (warns when the base image is outdated) and optionally switch bootc deployment
    Build {
        /// Tag for the built image
        #[arg(short, long)]
        tag: Option<String>,

        /// Use NVIDIA base image variant
        #[arg(long)]
        nvidia: bool,

        /// Automatically reboot after switching
        #[arg(long)]
        reboot: bool,

        /// Skip confirmation for bootc switch
        #[arg(short, long)]
        force: bool,

        /// Disable podman layer caching for this build
        #[arg(long)]
        no_cache: bool,
    },

    /// Switch the bootc deployment to the locally built image
    Switch {
        /// Tag of the locally built image
        #[arg(short, long)]
        tag: Option<String>,

        /// Skip confirmation
        #[arg(short, long)]
        force: bool,

        /// Automatically reboot after switching
        #[arg(long)]
        reboot: bool,
    },

    /// Roll back to the previous bootc deployment
    Rollback {
        /// Skip confirmation
        #[arg(short, long)]
        force: bool,

        /// Automatically reboot after rolling back
        #[arg(long)]
        reboot: bool,
    },

    /// Pull the latest base image, rebuild, and switch to the fresh build
    Update {
        /// Target tag (e.g. "stable", "latest")
        tag: Option<String>,

        /// Use NVIDIA base image variant
        #[arg(long)]
        nvidia: bool,

        /// Skip confirmation
        #[arg(short, long)]
        force: bool,

        /// Automatically reboot after update
        #[arg(long)]
        reboot: bool,

        /// Use podman layer cache (default: disabled — update always rebuilds fresh)
        #[arg(long)]
        cache: bool,
    },

    /// Show bootc deployment status and project info
    Status,

    /// Apply profile dotfiles into ~ as symlinks (also runs on build/update)
    Dotfiles,

    /// Manage the usr-overlay that backs a module (RakuOS rum or kineticos kinpkg)
    Overlay {
        #[command(subcommand)]
        action: OverlayAction,
    },

    /// Manage user-scoped Flatpaks declared in modules
    Flatpak {
        #[command(subcommand)]
        action: FlatpakAction,
    },
}

#[derive(Subcommand)]
enum OverlayAction {
    /// Snapshot overlay-installed packages into the project's overlay module
    Import {
        /// Show what would be written without writing
        #[arg(long)]
        dry_run: bool,

        /// Overwrite an existing module that already declares packages
        #[arg(short, long)]
        force: bool,
    },

    /// Remove overlay packages that are no longer declared in any module
    Sync {
        /// Skip the confirmation prompt
        #[arg(short, long)]
        force: bool,

        /// Show what would be removed without removing
        #[arg(long)]
        dry_run: bool,
    },

    /// Upgrade the packages already installed in the host overlay
    Upgrade {
        /// Skip the confirmation prompt
        #[arg(short, long)]
        force: bool,
    },

    /// Show drift between the host overlay and the project's modules
    Status,

    /// Remove from the overlay module any packages the booted base image
    /// already provides
    Prune {
        /// Skip the confirmation prompt
        #[arg(short, long)]
        force: bool,

        /// Show what would be pruned without writing
        #[arg(long)]
        dry_run: bool,
    },

    /// Rewrite the live overlay's package lists to match the module, then
    /// offer to reset the overlay (RakuOS rum)
    Clean {
        /// Show what would change without writing
        #[arg(long)]
        dry_run: bool,

        /// Skip the confirmation prompts (includes running the reset)
        #[arg(short, long)]
        force: bool,

        /// Never offer to run 'rakuos reset-overlay --soft'
        #[arg(long)]
        no_reset: bool,
    },
}

#[derive(Subcommand)]
enum FlatpakAction {
    /// Install/update declared user_flatpaks with `flatpak --user`
    Sync {
        /// Skip the confirmation prompt
        #[arg(short, long)]
        force: bool,

        /// Show what would change without touching the user flatpak installation
        #[arg(long)]
        dry_run: bool,
    },

    /// Show drift between declared user_flatpaks and the live user installation
    Status,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { name, from, force } => {
            commands::init::run(name, from, force)?;
        }
        Commands::Rollback { force, reboot } => {
            commands::rollback::run(force, reboot)?;
        }
        _ => {
            let project_path = project::resolve_project(cli.project)?;
            match cli.command {
                Commands::Init { .. } | Commands::Rollback { .. } => unreachable!(),
                Commands::Gen { dry_run } => {
                    commands::gen::run(&project_path, dry_run, cli.host)?;
                }
                Commands::Build {
                    tag,
                    nvidia,
                    reboot,
                    force,
                    no_cache,
                } => {
                    commands::build::run(
                        &project_path,
                        tag,
                        nvidia,
                        reboot,
                        force,
                        no_cache,
                        false,
                        false,
                        cli.host,
                    )?;
                }
                Commands::Switch {
                    tag,
                    force,
                    reboot,
                } => {
                    commands::switch::run(&project_path, tag, force, reboot, cli.host)?;
                }
                Commands::Update {
                    tag,
                    nvidia,
                    force,
                    reboot,
                    cache,
                } => {
                    commands::update::run(&project_path, tag, force, reboot, nvidia, cache, cli.host)?;
                }
                Commands::Status => {
                    commands::status::run(&project_path, cli.host)?;
                }
                Commands::Dotfiles => {
                    commands::dotfiles::run(&project_path, cli.host)?;
                }
                Commands::Overlay { action } => match action {
                    OverlayAction::Import { dry_run, force } => {
                        commands::overlay::import(
                            &project_path,
                            dry_run,
                            force,
                            cli.host.as_deref(),
                        )?;
                    }
                    OverlayAction::Sync { force, dry_run } => {
                        commands::overlay::sync_command(
                            &project_path,
                            force,
                            dry_run,
                            cli.host.as_deref(),
                        )?;
                    }
                    OverlayAction::Upgrade { force } => {
                        commands::overlay::upgrade_command(
                            &project_path,
                            force,
                            cli.host.as_deref(),
                        )?;
                    }
                    OverlayAction::Status => {
                        commands::overlay::status(&project_path, cli.host.as_deref())?;
                    }
                    OverlayAction::Prune { force, dry_run } => {
                        commands::overlay::prune(
                            &project_path,
                            dry_run,
                            force,
                            cli.host.as_deref(),
                        )?;
                    }
                    OverlayAction::Clean {
                        dry_run,
                        force,
                        no_reset,
                    } => {
                        commands::overlay::clean(
                            &project_path,
                            dry_run,
                            force,
                            no_reset,
                            cli.host.as_deref(),
                        )?;
                    }
                },
                Commands::Flatpak { action } => match action {
                    FlatpakAction::Sync { force, dry_run } => {
                        flatpak::sync_command(
                            &project_path,
                            force,
                            dry_run,
                            cli.host.as_deref(),
                        )?;
                    }
                    FlatpakAction::Status => {
                        flatpak::status_command(&project_path, cli.host.as_deref())?;
                    }
                },
            }
        }
    }

    Ok(())
}
