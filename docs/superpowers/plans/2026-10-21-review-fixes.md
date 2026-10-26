# Review fixes: disabled controls, autosave warning, directory sync

**Completion reconciliation (2026-10-26):** Implemented.

- [x] Disabled controls and the active map layer have distinct visuals, covered by the [contrast tests](../../../crates/game/tests/game/contrast.rs) and [map legend tests](../../../crates/game/tests/game/map_legend.rs).
- [x] Failed autosaves keep warning until confirmed saving succeeds, covered by the [save/resume tests](../../../crates/game/tests/game/save_resume.rs).
- [x] Directory synchronization and uncertain-publication retry behavior are covered by the [save-store tests](../../../crates/infrastructure/tests/save_store.rs) and [preferences tests](../../../crates/infrastructure/tests/client_preferences.rs).

The original execution recipe is retained below as plain steps. This reconciliation records
landed deliverables, without claiming a fresh run of each historical RED test, command or commit
sequence. Native checks without a complete receipt remain unchecked.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. The step sequence below is the historical execution recipe.

**Goal:** Fix the three Important findings from the review of commits d7eef29..261e40e.
- Disabled controls must look disabled.
- A failed autosave must keep warning the player.
- Saves must sync only the directories that need it.

**Architecture:** Each fix stays inside the layer it touches:
- disabled controls: `game` HUD theme and the button helper;
- the autosave warning: `game` persistence state and the top bar;
- directory sync: the `infrastructure` save store and preferences writer.

No domain or application changes. `SIM_RULES_VERSION` is unchanged.

**Tech Stack:** unchanged (Rust 1.98, Bevy 0.20, bevy_egui 0.43.1 / egui 0.36.2; no new crates).

**Spec:** `docs/superpowers/specs/2026-10-18-megawatt-mogul-design.md`:
- §17, save and resume, especially §17.4 (one persistence operation) and §17.5 (autosave);
- the HUD sections on the minimum window, 1024×640.

## Global Constraints

- The dependency rule and the game crate rules in AGENTS.md hold:
  - presenter pattern;
  - `dispatch` is the only writer;
  - resources `dispatch` writes are read through `ActionSink` getters;
  - one test binary;
  - new text goes in `.ftl` and in `keys.rs`.
- Text meets 4.5:1 contrast (the `contrast.rs` rule), disabled text included.
- Everything fits at 1024×640.
- TDD:
  - show RED for every behaviour change;
  - `cargo nextest run --workspace`, clippy `-D warnings` and `cargo fmt --all --check` are green
    before every commit;
  - never `--no-verify`.
- Every I/O test uses its own `TempDir`. No fixed paths, no environment variables.

## Review Focus

1. **A test that passes for the wrong reason.** The old contrast test looped over
   `[1.0, disabled_alpha]` with `disabled_alpha == 1.0`. The new tests must compare a disabled
   control with an enabled one, not a value with itself.
2. **The warning and the "(saved)" label must not contradict each other.** While an autosave
   failure is outstanding, the Save button must not say "(saved)".
3. **Retry after an uncertain publication.** It must still sync the directories the first attempt
   created, even though they now exist.

---

### Task 1: Disabled controls look disabled (game)

**Files:**
- Modify: `crates/game/src/hud/theme.rs` (`disabled_alpha`), `crates/game/src/hud/action.rs`
  (`button`), and `crates/game/src/hud/map.rs` (the layer row's active indicator)
- Test: `crates/game/tests/game/contrast.rs`, plus a real-pass test

**Finding:** `theme.rs:77` sets `v.disabled_alpha = 1.0`.
- In egui 0.36.2, a disabled `Button` still senses clicks, so it paints with the same `inactive`
  visuals as an enabled idle one.
- `Ui::disable()`'s `multiply_opacity(disabled_alpha)` was the only difference, so disabled and
  enabled controls now look the same.
- The map-layer row shows the active layer by disabling its button (`map.rs:~748`), so the player
  can't tell which layer is on.
- The same goes for the following, which all look clickable:
  - *New game* with a bad seed;
  - refused build entries;
  - *Start the day* with unsaved edits;
  - *Apply*/*Revert* with no changes.
- `contrast.rs` loops `for alpha in [1.0, v.disabled_alpha]`, which is now `[1.0, 1.0]`.

**Ruling:**
- Disabled controls get a distinct look, and their text still meets 4.5:1 against what is actually
  under it. Pick one approach and record why:
  - a `disabled_alpha < 1` with a text and surface pair chosen to pass at that alpha;
  - or the shared `button` helper drawing disabled buttons with their own visuals, such as a panel
    fill, no stroke and readable weak text.
- The active map layer is shown as **selected**: `Button::selected(true)`, or the existing toggle
  helper. Don't show it as disabled. Clicking the active layer stays harmless.

- **Step 1: Write the failing tests.**
  - `a_disabled_button_paints_differently_from_an_enabled_one`. Use the real pass. Use the painted
    shapes, the fill and text colour of two buttons, one enabled and one disabled, laid out the
    same way. Assert they differ and that the disabled text still meets 4.5:1 against its actual
    background.
  - `the_active_map_layer_is_shown_as_selected`. In the real pass, the active layer's button
    paints differently from the other layer buttons, and switching layers moves that look to the
    new active one.
  - In `contrast.rs`:
    - check disabled text with the real disabled rendering, not `disabled_alpha` compared with
      itself;
    - keep the existing audit's checked-count floors.
- **Step 2: Run them; see them fail.**
- **Step 3: Implement.**
- **Step 4: Run everything.**
- **Step 5: Commit**: `fix(game): disabled controls look disabled; the active map layer is selected`.

---

### Task 2: A failed autosave keeps warning (game)

**Files:**
- Modify: `crates/game/src/sim/persistence.rs` (`ContinueWithoutSaving`, the state)
- Modify: `crates/game/src/hud/top_bar.rs` (the warning and the Save label)
- Modify: `assets/locales/en-US/saves.ftl`, `crates/game/src/l10n/keys.rs`
- Test: `crates/game/tests/game/save_resume.rs`, plus inline presenter tests

**Finding:**
- `persistence.rs:374-381`: `ContinueWithoutSaving` clears both `error` and `task`, and nothing
  records that the latest month-close autosave failed.
- The top bar reads only `last_summary`, so it keeps saying "Save (saved)" for an older boundary.
- Spec §17.5 says a failed autosave "shows a persistent warning", and that only a later successful
  autosave clears it.

**Ruling:**
- Keep the failure in persistence state that survives `ContinueWithoutSaving`, for example
  `autosave_failed: Option<CheckpointSummary>` (the boundary that failed).
- Clear it only when a later autosave is confirmed, or when a manual save is confirmed whose
  boundary is the same or later.
- Show it in the top bar in the same way as `recording_stopped`:
  - truncated so it fits at 1024×640 next to Menu, Milestones and Save;
  - with the full text in a tooltip;
  - naming the month whose autosave failed.
- While the warning is set, the Save button's label must not say "(saved)".
- A new game or a load clears it.
- Starting the warning, and clearing it, are writes made through the existing persistence path,
  not new `dispatch` writes, unless a new `UiAction` is genuinely needed. If one is, glue-test it.

- **Step 1: Write the failing tests.**
  - `a_failed_autosave_keeps_warning_after_continue_without_saving`. Inject an autosave failure
    with the store's fault hooks, then click *Continue without saving*:
    - the warning is in the top bar model, and in the real pass, with the failed month;
    - the Save label is not "(saved)";
    - after playing on, the warning is still shown.
  - `a_later_confirmed_autosave_clears_the_warning`, and the same with a manual save.
  - `a_new_game_or_load_clears_the_warning`.
  - At 1024×640, with both the recording-stopped banner and this warning shown, Menu, Milestones
    and Save are inside the screen and clickable. Use the real pass and the existing top-bar fit
    pattern.
- **Step 2: Run them; see them fail.**
- **Step 3: Implement.**
- **Step 4: Run everything.**
- **Step 5: Commit**: `fix(game): a failed autosave keeps warning until a save succeeds`.

---

### Task 3: Sync only the directories that need it (infrastructure)

**Files:**
- Modify: `crates/infrastructure/src/save/file_store.rs` (`make_directory`, `sync_directory`, the
  publish path) and `crates/infrastructure/src/client_preferences.rs`
- Test: `crates/infrastructure/tests/save_store.rs` and `client_preferences.rs`, plus unit tests in
  `file_store.rs`

**Finding:**
- `make_directory` (`file_store.rs:128-152`) adds every ancestor of the save root, up to `/`, to
  the sync list, so every save fsyncs all of them. Any failure becomes `DurabilityUnconfirmed`, and
  Retry repeats the same syncs.
- On macOS each sync is an `F_FULLFSYNC`, so a save flushes the drive cache about seven times.
- On Windows the directory is opened read-only with backup semantics, so `FlushFileBuffers` (which
  needs `GENERIC_WRITE`) should fail on every save. That would make every save "durability
  unconfirmed", and "Save and return to menu" could never finish. (This is inferred: CI runs
  Ubuntu only.)
- Preferences use the same path.

**Ruling:**
- Sync only:
  - the destination's parent, which is the save root;
  - the parent of each directory that this call created, including any a failed
    `create_dir_all` left behind.
- A retry after `DurabilityUnconfirmed` must still sync the directories an earlier attempt created.
  Record them on the store, for example as a set of pending directories that is cleared once each
  sync succeeds, or carry them through the uncertain-revision state. Pick one and say why.
- Windows: don't attempt a directory sync. `FlushFileBuffers` on a directory needs
  `GENERIC_WRITE`, which is why the path doesn't attempt it. Document in `sync_directory` what a
  receipt does and doesn't mean there:
  - On NTFS the rename is journaled, so a crash leaves either the previous or the new complete
    file.
  - The rename reaches disk when NTFS commits its log. A receipt on Windows means the contents
    were flushed and publication was atomic, not that the directory entry is synced.
  - Non-journaled volumes such as FAT32 or exFAT get neither guarantee.

  A cfg-gated unit test asserts that the Windows path returns `Ok` without opening the directory.
- Preferences reuse the narrowed helper.

- **Step 1: Write the failing tests.** Make the set of directories synced observable to the
  test, for example through an injectable sync function or the existing fault hooks, rather than
  real fsync counts.
  - `a_save_into_an_existing_root_syncs_only_the_root`: none of its ancestors.
  - `a_save_that_creates_nested_directories_syncs_each_new_directorys_parent`.
  - `a_retry_after_an_uncertain_publish_syncs_the_directories_the_first_attempt_created`: inject a
    `SyncDirectory` fault on the first attempt.
  - `a_sync_failure_on_the_root_is_still_durability_unconfirmed`, so the error path stays honest.
  - The preferences writer syncs only its own directory.
- **Step 2: Run them; see them fail.**
- **Step 3: Implement.**
- **Step 4: Run everything.**
- **Step 5: Commit**: `fix(infrastructure): sync only the directories a save created`.
