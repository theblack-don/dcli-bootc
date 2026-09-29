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
- [ ] Returns true on a kineticos-like host, false otherwise.
- [ ] Failure to read os-release does not panic.

**Verification:**
- [ ] `cargo check`

**Dependencies:** None

**Files likely touched:**
- `src/project.rs` or `src/config.rs`

**Estimated scope:** XS

### Task 6: CLI for `init --from-image` / `--template`
**Description:** Add the flags to the `Init` subcommand and pass them through.

**Acceptance criteria:**
- [ ] Flags parse and reach `commands::init::run`.

**Verification:**
- [ ] `cargo check`

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
- [ ] `base_image` equals the booted image ref when available.
- [ ] Falls back to the default base when `bootc status` is unavailable.

**Verification:**
- [ ] `cargo check`

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
- [ ] Template copy reproduces `profile.yaml`, `modules/`, `files/`, `.gitignore`.
- [ ] `profile.name` and the registered project name match the requested name.
- [ ] Overlay module is seeded from live state (empty live = empty/absent module).
- [ ] Missing template falls back to the from-image reconstruction.

**Verification:**
- [ ] `cargo check`

**Dependencies:** Tasks 4-7

**Files likely touched:**
- `src/commands/init.rs`
- `src/commands/overlay.rs`

**Estimated scope:** M

## Checkpoint: Phase 3
- [ ] `cargo check` clean
- [ ] init flow reasoned for template-present and template-absent
- [ ] Review with human before Phase 4

## Phase 4: Docs

### Task 9: README overlay section
**Description:** Document non-destructive default, auto-snapshot, and the
explicit-remove flow (`overlay sync`), replacing the "run import first" guidance.

**Acceptance criteria:**
- [ ] README describes the new build/update behavior.
- [ ] Remove-a-package flow is documented without ambiguity.

**Verification:**
- [ ] Proofread against `tasks/plan.md`.

**Dependencies:** Tasks 1-4

**Files likely touched:**
- `README.md`

**Estimated scope:** S

### Task 10: README kineticos init flow
**Description:** Document bare `init` on kineticos, `--from-image`,
`--template`, and the `~/.config/kineticos/` contract.

**Acceptance criteria:**
- [ ] README covers both fork models (layer vs copy-upstream).
- [ ] Installer population of `~/.config/kineticos` marked as future work.

**Verification:**
- [ ] Proofread against `tasks/plan.md`.

**Dependencies:** Tasks 5-8

**Files likely touched:**
- `README.md`

**Estimated scope:** S

## Checkpoint: Complete
- [ ] All acceptance criteria met
- [ ] `cargo check` clean
- [ ] Ready for review
