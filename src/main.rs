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

        /// Layer on the running booted image (base image = the tracked/booted
        /// image ref). Generic; on kineticos prefer --extend
        #[arg(long)]
        from_image: bool,

        /// Layer on the running kineticos image: add modules on top; kineticos
        /// keeps managing the base and its updates flow through it
        #[arg(long)]
        extend: bool,

        /// Copy the full project template for full control (base stays the
        /// template's base; you take over upstream updates)
        #[arg(long)]
        fork: bool,

        /// Project template to copy (default: /usr/share/kineticos/project,
        /// then ~/.config/kineticos)
        #[arg(long)]
        template: Option<String>,

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

        /// Named base image from profile.yaml's `base_images` map; the image
        /// tag defaults to this name (`--base-image deck` builds `:deck`)
        #[arg(long, value_name = "NAME")]
        base_image: Option<String>,

        /// Alias for `--base-image nvidia`
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

        /// Do not snapshot the live overlay into the overlay module
        #[arg(long)]
        no_import: bool,

        /// Build only; never prompt to switch the bootc deployment (for CI)
        #[arg(long)]
        no_switch: bool,
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

        /// Named base image from profile.yaml's `base_images` map; the image
        /// tag defaults to this name (`--base-image deck` builds `:deck`)
        #[arg(long, value_name = "NAME")]
        base_image: Option<String>,

        /// Alias for `--base-image nvidia`
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

        /// Do not snapshot the live overlay into the overlay module
        #[arg(long)]
        no_import: bool,

        /// Build only; never prompt to switch the bootc deployment (for CI)
        #[arg(long)]
        no_switch: bool,
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

    /// Generate shell completion scripts (bash, fish, zsh)
    Completions {
        /// Shell to emit completions for
        #[arg(value_enum)]
        shell: clap_complete::Shell,

        /// Binary name to generate for
        #[arg(long, default_value = "dcli-bootc")]
        bin: String,
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
        Commands::Init {
            name,
            from,
            from_image,
            extend,
            fork,
            template,
            force,
        } => {
            commands::init::run(name, from, from_image, extend, fork, template, force)?;
        }
        Commands::Rollback { force, reboot } => {
            commands::rollback::run(force, reboot)?;
        }
        Commands::Completions { shell, bin } => {
            commands::completions::run(<Cli as clap::CommandFactory>::command(), shell, &bin)?;
        }
        _ => {
            let project_path = project::resolve_project(cli.project)?;
            match cli.command {
                Commands::Init { .. }
                | Commands::Rollback { .. }
                | Commands::Completions { .. } => unreachable!(),
                Commands::Gen { dry_run } => {
                    commands::gen::run(&project_path, dry_run, cli.host)?;
                }
                Commands::Build {
                    tag,
                    base_image,
                    nvidia,
                    reboot,
                    force,
                    no_cache,
                    no_import,
                    no_switch,
                } => {
                    let base_image =
                        base_image.or_else(|| nvidia.then(|| "nvidia".to_string()));
                    commands::build::run(
                        &project_path,
                        tag,
                        base_image,
                        reboot,
                        force,
                        no_cache,
                        no_import,
                        no_switch,
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
                    base_image,
                    nvidia,
                    force,
                    reboot,
                    cache,
                    no_import,
                    no_switch,
                } => {
                    let base_image =
                        base_image.or_else(|| nvidia.then(|| "nvidia".to_string()));
                    commands::update::run(
                        &project_path,
                        tag,
                        force,
                        reboot,
                        base_image,
                        cache,
                        no_import,
                        no_switch,
                        cli.host,
                    )?;
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
                            false,
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
