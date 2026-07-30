use anyhow::{Context, Result};
use colored::*;

use crate::bootc;

/// Roll back = queue the previous bootc deployment for the next boot.
///
/// This is a purely system-level operation: it never touches the project,
/// profile, or host configuration, so it works from any directory and is
/// unaffected by multi-host setups or tags.
pub fn run(force: bool, auto_reboot: bool) -> Result<()> {
    let status =
        bootc::status().context("Could not read bootc status — is this a bootc system?")?;

    let Some(rollback_image) = status.rollback_image.clone() else {
        anyhow::bail!(
            "No rollback deployment is available.\n  Rollback requires a previous deployment recorded by bootc.\n  Check with: sudo bootc status"
        );
    };

    println!("{} Roll back bootc deployment", "◀".cyan());
    match &status.booted_image {
        Some(image) => {
            print!("  Booted:   {}", image);
            if let Some(d) = &status.booted_digest {
                print!(" ({})", bootc::short_digest(d));
            }
            bootc::print_built(&status.booted_digest);
            println!();
        }
        None => println!("  Booted:   (unknown)"),
    }
    print!("  Rollback: {}", rollback_image);
    if let Some(d) = &status.rollback_digest {
        print!(" ({})", bootc::short_digest(d));
    }
    bootc::print_built(&status.rollback_digest);
    println!();
    // A staged deployment is discarded by `bootc rollback` — spell out the
    // consequence loudly, because you then land on the rollback deployment,
    // i.e. further back than just undoing the pending update.
    let has_staged = status.staged_image.is_some();
    if let Some(staged) = &status.staged_image {
        println!();
        println!(
            "  {} A newer deployment is staged for the next boot:",
            "!".bold().red()
        );
        print!("      {}", staged);
        if let Some(d) = &status.staged_digest {
            print!("  ({})", bootc::short_digest(d));
        }
        bootc::print_built(&status.staged_digest);
        println!();
        println!(
            "  {} Rolling back {} it and boots the rollback deployment above —",
            "!".bold().red(),
            "DISCARDS".bold()
        );
        println!("    you end up further back than just undoing the pending update.");
        println!("    To cancel only the pending update instead, answer no and run:");
        println!("      sudo ostree admin undeploy 0");
    }
    println!();

    let prompt = if has_staged {
        "Discard the staged deployment AND roll back to the previous one?"
    } else {
        "Roll back to the previous deployment?"
    };

    if force || bootc::confirm(prompt) {
        let queued = bootc::rollback()?;
        bootc::maybe_reboot(auto_reboot, queued);
    } else {
        println!("  Aborted.");
    }

    Ok(())
}
