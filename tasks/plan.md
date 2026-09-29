# Implementation Plan: kineticos overlay onboarding + fork workflow

## Overview

Make `dnf`/`kinpkg` overlay installs survive `dcli-bootc build`/`update`
without the user remembering `overlay import`, keep the project's
`overlay-packages.yaml` in sync automatically, and give kineticos users a
one-command way to reproduce/fork the running image into their own project.

Core insight: `kinpkg` already auto-records every install/remove in
`/var/lib/kineticos/overlay/installed.txt`. The footgun is that `build`/`update`
treat `overlay-packages.yaml` as authoritative and delete any live package it
does not list. Fix the destructive default first; the YAML sync is then a
convenience, not a safety requirement.

## Architecture Decisions

- **Live overlay is non-destructive by default.** `build`/`update` never remove
  overlay packages; they only install declared ones. Only an explicit
  `dcli-bootc overlay sync` is authoritative and may remove. (Decision 1.)
- **Auto-snapshot is project-local (2a).** When a project exists,
  `build`/`update` snapshot live overlay state into
  `modules/overlay-packages.yaml`. The manifest ends up as
  `union(declared, live)` by running the additive reconcile first, then the
  snapshot.
- **Fork default = layer on the upstream kineticos image.** `init --from-image`
  sets `base_image` to the booted/tracked image ref, so upstream updates flow
  through the base. Copying upstream modules for full control is the secondary
  path (still supported, manual rebase).
- **Canonical kineticos template is baked into the image at
  `/usr/share/kineticos/project/`** (read-only, version-matched with the running
  image) via a generator `include_project` option. User projects are created in
  `./<name>` and indexed by the registry. Bare `init` on kineticos prompts for
  **Extend** (layer on the running image, upstream-tracked) vs **Fork** (full
  copy, you own the upstream recipe). `--from-image` is retained for
  non-kineticos/other use cases.

## Behavior Changes

| Command | Today | After |
|---|---|---|
| `dcli-bootc build` / `update` | sync removes undeclared live packages | sync is additive; then auto-imports live into the overlay module |
| `dcli-bootc overlay sync` | authoritative (add + remove) | unchanged: authoritative |
| `dcli-bootc overlay import` | manual snapshot | unchanged; `build`/`update` call it automatically |
| `dcli-bootc init` on kineticos | generic Bazzite scaffold | copies `~/.config/kineticos`, seeds overlay yaml, sets name |
| `dcli-bootc init --from-image` | n/a | reconstruct project from booted image on any bootc host |

Empty manifest still means "leave the live overlay alone" and is never allowed
to wipe it.

## Task List

### Phase 1: Non-destructive overlay reconcile (Layer 1)
- [ ] Task 1: Add `remove_extra: bool` to `overlay::sync`.
- [ ] Task 2: Thread the flag through call sites: build/update pass `false`,
      `overlay sync` passes `true`.

### Checkpoint: Phase 1
- [ ] `cargo check` clean.
- [ ] A live package absent from the module survives a simulated `update` sync.
- [ ] `overlay sync` still removes undeclared packages.

### Phase 2: Auto-snapshot (Layer 2a)
- [ ] Task 3: Add `--no-import` to `build` and `update`.
- [ ] Task 4: After the additive reconcile, auto-run `overlay::import`
      (quiet, `force = true`) unless `--no-import` or no `overlay_module`.

### Checkpoint: Phase 2
- [ ] `cargo check` clean.
- [ ] `dnf install foo` (recorded live) appears in `modules/overlay-packages.yaml`
      after a build/update, without `overlay import`.
- [ ] A package declared in yaml but not live is installed and stays in yaml.

### Phase 3: init --from-image + kineticos template
- [ ] Task 5: Add a `is_kineticos()` detector (os-release / runtime markers).
- [ ] Task 6: Add `init --from-image` and `--template <dir>` to the CLI.
- [ ] Task 7: Seed `base_image` from `bootc status` (booted/tracked image).
- [ ] Task 8: On kineticos, bare `init` copies `~/.config/kineticos/`; seed
      `modules/overlay-packages.yaml` from the live overlay; set `profile.name`;
      register the project.

### Checkpoint: Phase 3
- [ ] `cargo check` clean.
- [ ] On kineticos with a template dir, `init demo` yields a project with the
      live overlay recorded and `name: demo`.
- [ ] On a non-kineticos bootc host, `init --from-image demo` sets `base_image`
      to the booted image ref.

### Phase 4: Docs
- [ ] Task 9: Rewrite README overlay section: non-destructive default, auto
      snapshot, and the explicit-`overlay sync`-to-remove flow.
- [ ] Task 10: Document the kineticos `init` flow and `~/.config/kineticos`
      contract; note the installer population is future work.

## Phase 5: revised init (baked template + explicit modes)

Supersedes the template-location parts of Phase 3.

- [x] Task 11: Generator `include_project` option bakes the project tree into
      `/usr/share/<name>/project/` (whole tree; exclude `.git`, `.dcli-bootc`,
      `output/`, `target/`, `build_files/`).
- [x] Task 12: Template resolution `--template` →
      `/usr/share/kineticos/project` → `~/.config/kineticos`.
- [x] Task 13: Bare `init` on kineticos prompts **Extend** vs **Fork**.
- [x] Task 14: Add `--extend` / `--fork`; keep `--from-image` for generic use.
- [x] Task 15: Update README and the kineticos README for the `/usr` template.

## Files Likely Touched

- `src/commands/overlay.rs` — `sync` signature, quiet import helper.
- `src/commands/build.rs` — additive sync call, auto-import.
- `src/commands/update.rs` — `--no-import` plumbing.
- `src/main.rs` — CLI flags (`--no-import`, `init --from-image/--template`).
- `src/commands/init.rs` — from-image / template / kineticos copy flow.
- `src/project.rs` or `src/config.rs` — kineticos detection helper.
- `README.md` — behavior docs.

## Out of Scope (tabled)

- Baking a project snapshot into the OS image (#4).
- Populating `~/.config/kineticos/` from the ISO installer / first boot.
- A system-wide `kinpkg`-maintained manifest (option 2b).

## Risks and Mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| Auto-snapshot overwrites hand-edited yaml | Med | Live is the documented source of truth; `--no-import` preserves strict declarative mode; removals via explicit `overlay sync` |
| Manifest becomes union, so yaml edits can't remove | Med | Document remove flow: `overlay sync` (authoritative), then yaml follows live |
| Kineticos detection false-positive | Low | Detect via os-release + runtime markers; allow `--template`/`--from-image` to override |
| `~/.config/kineticos` missing | Low | Fall back to reconstructing a minimal project from `bootc status` |

## Resolved During Implementation

- Bare `init` on kineticos **prompts Extend vs Fork** (Phase 5, superseding the
  earlier auto-copy). Extend layers on the running image and tracks upstream;
  Fork copies the full recipe. Non-interactive defaults to Extend.
- The canonical template is **baked into the image** at
  `/usr/share/kineticos/project` (`include_project: true`), so no installer
  seeding of `~/.config/kineticos` is needed; that path remains a manual/legacy
  fallback.
- `overlay sync` stayed "explicit sync removes" — no extra `--no-remove` guard.
  build/update are additive, which was the safety goal.
- The old interactive "import before sync?" prompt was replaced by an
  informational note under `--no-import`, since additive sync removes nothing.
- Auto-snapshot is idempotent: `import` skips rewriting when the module already
  matches the live overlay.

## Open Questions

- None outstanding. The baked template removes the earlier installer-seeding
  work item.
