# AGENTS.md

Guidance for agents working in this repository.

## Always plan first

Before writing or changing any code, produce a short plan and get agreement on
it. Do not start implementing until the approach is settled. Investigate the
code and the environment first, then propose the plan.

## Never build — only `cargo check`

Do **not** run `cargo build`, `cargo run`, or any project build script
(`build-dev.sh`, `make`, `make srpm`, etc.).

The only cargo command you may run is:

```bash
cargo check
```

That is the full extent of verification permitted. If a task seems to require
compiling or running the binary, stop and ask before doing it.
