mod bootc;
mod commands;
mod config;
mod generator;
mod hash;
mod module;
mod project;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "dcli-bootc")]
#[command(version, about = "A declarative bootc image configuration manager")]
struct Cli {
    /// Path to the bootc project (defaults to current dir or registry default)
    #[arg(short, long, global = true)]
    project: Option<String>,

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

    /// Build the container image and optionally switch bootc deployment
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
    },

    /// Show bootc deployment status and project info
    Status,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { name, from, force } => {
            commands::init::run(name, from, force)?;
        }
        _ => {
            let project_path = project::resolve_project(cli.project)?;
            match cli.command {
                Commands::Init { .. } => unreachable!(),
                Commands::Gen { dry_run } => {
                    commands::gen::run(&project_path, dry_run)?;
                }
                Commands::Build {
                    tag,
                    nvidia,
                    reboot,
                    force,
                    no_cache,
                } => {
                    commands::build::run(&project_path, tag, nvidia, reboot, force, no_cache)?;
                }
                Commands::Switch {
                    tag,
                    force,
                    reboot,
                } => {
                    commands::switch::run(&project_path, tag, force, reboot)?;
                }
                Commands::Update {
                    tag,
                    nvidia,
                    force,
                    reboot,
                } => {
                    commands::update::run(&project_path, tag, force, reboot, nvidia)?;
                }
                Commands::Status => {
                    commands::status::run(&project_path)?;
                }
            }
        }
    }

    Ok(())
}
