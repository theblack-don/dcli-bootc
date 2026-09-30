use anyhow::Result;
use clap::Command;

/// Emit a shell completion script for `bin` to stdout.
pub fn run(mut cmd: Command, shell: clap_complete::Shell, bin: &str) -> Result<()> {
    let mut out = std::io::stdout();
    clap_complete::generate(shell, &mut cmd, bin, &mut out);
    Ok(())
}
