# TODO: kineticos overlay onboarding + fork workflow

Plan: `tasks/plan.md`

## Phase 1: Non-destructive overlay reconcile

### Task 1: Add `remove_extra` to `overlay::sync`
**Description:** Give `overlay::sync` a `remove_extra: bool` parameter. When
false, skip the removal half of the diff (still install declared packages and
still restage local RPMs). Removal stays available to explicit callers.

**Acceptance criteria:**
- [x] `sync(..., remove_extra = false)` never calls `backend.remove_packages`.
- [x] `sync(..., remove_extra = true)` behaves exactly as today.
- [x] Empty-manifest safety and base-provided retention are unchanged.

**Verification:**
- [x] `cargo check`
- [x] Unit-level reasoning/check of the diff branch (no project build allowed).

**Dependencies:** None

**Files likely touched:**
- `src/commands/overlay.rs`

**Estimated scope:** S

### Task 2: Thread the flag through call sites
**Description:** `build`/`update` call `sync` with `remove_extra = false`;
`overlay sync` calls it with `true`.

**Acceptance criteria:**
- [x] `build`/`update` never remove overlay packages.
- [x] `dcli-bootc overlay sync` still removes undeclared packages.

**Verification:**
- [x] `cargo check`

**Dependencies:** Task 1

**Files likely touched:**
- `src/commands/build.rs`
- `src/commands/overlay.rs`

**Estimated scope:** XS

## Checkpoint: Phase 1
- [x] `cargo check` clean
- [x] Additive build/update path confirmed
- [ ] Review with human before Phase 2

## Phase 2: Auto-snapshot

### Task 3: Add `--no-import` to `build` and `update`
**Description:** New CLI flag to disable auto-snapshot.

**Acceptance criteria:**
- [x] `dcli-bootc build --no-import` and `dcli-bootc update --no-import` parse.
- [x] Flag is plumbed to the auto-import step.

**Verification:**
- [x] `cargo check`

**Dependencies:** None

**Files likely touched:**
- `src/main.rs`
- `src/commands/build.rs`
- `src/commands/update.rs`

**Estimated scope:** S

### Task 4: Auto-run `overlay import` after additive reconcile
**Description:** Unless `--no-import` or no `overlay_module`, snapshot live
overlay state into `modules/overlay-packages.yaml` after the additive reconcile,
so the manifest becomes `union(declared, live)`. Use a quiet helper so build/update
output is not noisy.

**Acceptance criteria:**
- [x] After a build/update, a live package not previously declared appears in
      the overlay module.
- [x] A package declared but not live is installed first, then present in the module.
- [x] `--no-import` leaves the module untouched.
- [x] No project / no overlay backend is a no-op.

**Verification:**
- [x] `cargo check`

**Dependencies:** Tasks 1-3

**Files likely touched:**
- `src/commands/overlay.rs`
- `src/commands/build.rs`

**Estimated scope:** M

## Checkpoint: Phase 2
- [x] `cargo check` clean
- [x] Auto-snapshot flow reasoned end-to-end
- [ ] Review with human before Phase 3

## Phase 3: init --from-image + kineticos template

### Task 5: `is_kineticos()` detection helper
**Description:** Detect a kineticos host via os-release and/or runtime markers
(overlay dir, `kinpkg`). Keep it conservative and overridable.

**Acceptance criteria:**
- [x] Returns true on a kineticos-like host, false otherwise.
- [x] Failure to read os-release does not panic.

**Verification:**
- [x] `cargo check`

**Dependencies:** None

**Files likely touched:**
- `src/project.rs` or `src/config.rs`

**Estimated scope:** XS

### Task 6: CLI for `init --from-image` / `--template`
**Description:** Add the flags to the `Init` subcommand and pass them through.

**Acceptance criteria:**
- [x] Flags parse and reach `commands::init::run`.

**Verification:**
- [x] `cargo check`

**Dependencies:** None

**Files likely touched:**
- `src/main.rs`
- `src/commands/init.rs`

**Estimated scope:** S

### Task 7: Seed `base_image` from `bootc status`
**Description:** `init --from-image` (and the kineticos fallback when no
template exists) uses the booted/tracked image ref as `base_image`; keeps the
nvidia variant when it can be inferred, else drops it.

**Acceptance criteria:**
- [x] `base_image` equals the booted image ref when available.
- [x] Falls back to the default base when `bootc status` is unavailable.

**Verification:**
- [x] `cargo check`

**Dependencies:** Task 6

**Files likely touched:**
- `src/commands/init.rs`
- `src/bootc.rs` (reuse `status()`)

**Estimated scope:** S

### Task 8: Kineticos template copy + live-overlay seed
**Description:** On kineticos, bare `init <name>` copies the project from
`~/.config/kineticos/` (or `--template`), sets `profile.name`, seeds
`modules/overlay-packages.yaml` from the live overlay, and registers the project.
Refuse to clobber an existing project without `--force`.

**Acceptance criteria:**
- [x] Template copy reproduces `profile.yaml`, `modules/`, `files/`, `.gitignore`.
- [x] `profile.name` and the registered project name match the requested name.
- [x] Overlay module is seeded from live state (empty live = empty/absent module).
- [x] Missing template falls back to the from-image reconstruction.

**Verification:**
- [x] `cargo check`

**Dependencies:** Tasks 4-7

**Files likely touched:**
- `src/commands/init.rs`
- `src/commands/overlay.rs`

**Estimated scope:** M

## Checkpoint: Phase 3
- [x] `cargo check` clean
- [x] init flow reasoned for template-present and template-absent
- [ ] Review with human before Phase 4

## Phase 4: Docs

### Task 9: README overlay section
**Description:** Document non-destructive default, auto-snapshot, and the
explicit-remove flow (`overlay sync`), replacing the "run import first" guidance.

**Acceptance criteria:**
- [x] README describes the new build/update behavior.
- [x] Remove-a-package flow is documented without ambiguity.

**Verification:**
- [x] Proofread against `tasks/plan.md`.

**Dependencies:** Tasks 1-4

**Files likely touched:**
- `README.md`

**Estimated scope:** S

### Task 10: README kineticos init flow
**Description:** Document bare `init` on kineticos, `--from-image`,
`--template`, and the `~/.config/kineticos/` contract.

**Acceptance criteria:**
- [x] README covers both fork models (layer vs copy-upstream).
- [x] Installer population of `~/.config/kineticos` marked as future work.

**Verification:**
- [x] Proofread against `tasks/plan.md`.

**Dependencies:** Tasks 5-8

**Files likely touched:**
- `README.md`

**Estimated scope:** S

## Phase 5: revised init (baked template + explicit modes)

Supersedes the template-location parts of Phase 3.

### Task 11: Generator `include_project` bakes the template
**Description:** When `profile.yaml` sets `include_project: true`, `gen` emits a
step copying the project tree into `/usr/share/<name>/project/` inside the
image (whole tree minus VCS/generated/transient dirs).

**Acceptance criteria:**
- [x] Baked path contains `profile.yaml`, `modules/`, `files/`, `dotfiles/`.
- [x] `.git`, `.dcli-bootc`, `output/`, `target/`, `build_files/` excluded.
- [x] No-op when the option is absent.

**Verification:** `cargo check`

**Dependencies:** None

**Files likely touched:** `src/generator.rs`, `src/config.rs`

**Estimated scope:** M

### Task 12: Template resolution order
**Description:** `init` resolves the template from `--template`, then
`/usr/share/kineticos/project`, then `~/.config/kineticos`.

**Acceptance criteria:**
- [x] `/usr/share/kineticos/project` is preferred over `~/.config/kineticos`.
- [x] Missing everywhere falls back to `--from-image` reconstruction.

**Verification:** `cargo check`

**Dependencies:** Task 11

**Files likely touched:** `src/project.rs`, `src/commands/init.rs`

**Estimated scope:** S

### Task 13: Extend/Fork prompt
**Description:** Bare `init` on kineticos asks the user to choose Extend or
Fork, explaining the upstream-tracking difference. Non-interactive default is
Extend.

**Acceptance criteria:**
- [x] Prompt appears only on kineticos TTY without `--extend`/`--fork`.
- [x] Non-interactive invocations never block on a prompt.

**Verification:** `cargo check`

**Dependencies:** Task 12

**Files likely touched:** `src/commands/init.rs`

**Estimated scope:** S

### Task 14: `--extend` / `--fork` flags
**Description:** Explicit modes; `--from-image` remains for non-kineticos hosts.

**Acceptance criteria:**
- [x] `--extend` = layer on the running image; `--fork` = copy the template.
- [x] Combining incompatible modes errors clearly.

**Verification:** `cargo check`

**Dependencies:** Task 13

**Files likely touched:** `src/main.rs`, `src/commands/init.rs`

**Estimated scope:** S

### Task 15: Docs
**Description:** Document the baked template, the prompt, and both modes.

**Acceptance criteria:**
- [x] README overlay/init sections describe the `/usr` template and modes.

**Verification:** Proofread against `tasks/plan.md`.

**Dependencies:** Tasks 11-14

**Files likely touched:** `README.md`

**Estimated scope:** S

## Checkpoint: Complete
- [x] All acceptance criteria met
- [x] `cargo check` clean
- [ ] Ready for review
