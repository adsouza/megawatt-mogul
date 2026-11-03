# First-game tutorial and the SGCC unlock: Design

Status: draft for approval, 2026-10-31. Amends the main spec
(`2026-10-18-megawatt-mogul-design.md`), §2.6 (regions), §8.4 (HUD by phase) and §28.10 (the
world picker).

## 1. Why

A first-time player, here an 11-year-old, picks a region, lands in Planning and doesn't know
where to begin: the map, build menu, inspector, tariff, commitment grid and forecast all compete
for attention, and nothing says what the game is *for*. The tutorial fixes that in two moves:

1. **Establish the goal** before anything else: earn more money by expanding the network to
   connect more demand, while keeping supply balanced against that demand so the lights stay on.
2. **Walk through one concrete plan** on the live game, pointing at the exact piece of UI each
   task uses: build a battery, connect it, start the day, speed up, then read the bill.

SGCC is the most complex region (multi-area, HVDC links), so it unlocks only after the player has
genuinely played four other regions.

## 2. Goals, non-goals, anti-goals

**Goals**

- A new player knows the game's goal within the first minute, and has taken a real action that
  matters (a battery that smooths demand spikes) by the end of the first month.
- Every step is a real game action or a brief, self-ending highlight. **There is no Next button.**
  Progress is inferred from game state.
- Plain, short language that an 11-year-old can read.
- The step list is easy to extend: the user plans to carry the tutorial beyond the first month
  soon.
- SGCC stays locked until four other regions are "played" (§6).

**Non-goals (this version)**

- Guidance beyond building and connecting the first new generator after the first month's Review.
- Resuming a tutorial in a loaded save. The tutorial runs only in the game it started with.
- Voice-over, sound cues or animated characters.
- Translating the new text into other locales (`en-US` only, as today).

**Anti-goals** (a failure even if it technically works)

- Any tutorial effect on the simulation. The tutorial never sends a `PlayerCommand` and never
  writes simulation state, so replays, saves and digests are identical with it on or off. It
  changes only client UI state (tabs, scroll, camera) and client preferences.
- Blocking the player. Spotlights end on any click, Escape or after a few seconds; instructions
  never stop the player from doing something else.
- A tutorial that gets stuck because the player went off-script. It must re-evaluate from state
  every frame (§4.2) rather than assume the previous step happened.
- Patronising walls of text. Each note is one or two short sentences.

## 3. When the tutorial runs

- A new client preference, **`tutorial_pending: bool`**, defaults to `true`. Existing preference
  files lack the field, so every existing player also sees the tutorial once in their next new
  game. That is intended.
- The **new-game screen** (region select, `AppState::RegionSelect`) gets a **"Show tutorial"**
  checkbox beside the seed field, bound to `tutorial_pending`. It reflects the stored value, so
  it is ticked on a first game. It is the only way to turn the tutorial back on once it has
  finished: the main menu has no tutorial button.
- Starting a new game with `tutorial_pending` true starts the tutorial in that game. Loading or
  continuing a save never starts it.
- After the first Review is dismissed, the tutorial continues with the regional generator walkthrough (§4.5).
  After the generator is ordered and connected and the annual outlook is opened, the next
  month's Planning phase offers the plant-payoff comparison. Closing that comparison leads to
  a final renewable-siting spotlight (§4.6). It **finishes** when this spotlight is dismissed or
  times out, or when the player starts a day early while optional guidance is available. That sets
  `tutorial_pending = false` and writes preferences. Leaving the game or
  quitting part-way leaves the flag `true`, so the next new game runs it again.

## 4. The tutorial

### 4.1 Building blocks

Four kinds of presentation, all drawn by the `game` crate in `HudSet::Overlays`:

- **Crawl.** A translucent full-window overlay with perspective text receding up the screen, in
  the style of the Star Wars opening crawl.
- **Spotlight.** The whole window dims except a rounded cut-out around one *anchor* (a widget's
  rect). A short note sits beside the cut-out on an opaque card. It fades in, holds for about
  4 seconds and fades out. **Any click or Escape ends it early.** The click that ends a spotlight
  is consumed, so it never also acts on the game underneath. A spotlight never waits for an
  action. Its clock counts only the time it is actually drawn: if its anchor disappears part-way
  (scrolled out of view, say) or a prompt opens, it pauses and resumes when it is shown again, so
  it never runs out unseen. The crawl's clock works the same way.
- **Instruction.** An undimmed **arrow callout**, which doesn't look like a button: one sentence
  as plain text on an unoutlined, opaque backing in the panel fill (so it still meets §4.4's
  contrast floor), with an accent-coloured arrow (about 2 px wide, 8 px head) from the text to
  the exact control the step asks for, its head on the control. It stays while its step is
  active, never covers its control and takes no clicks, not even on its own text. Placement:
  just right of the control and vertically centred on it, the arrow running from the text's
  left edge to the control's right edge; the text wraps to the room between the arrow's 28 px
  gap and the window's margin, and with less than about 96 px there it goes below the control
  (above, if below doesn't fit), the arrow pointing up (down) at it. An instruction about the
  map, which has no single control, sits inside the map's top edge, centred on it, its arrow
  pointing down into the map; on a map too short for that (the smallest window at the largest
  UI zoom) it goes just below the map, pointing up at it. If its control sits in a scroll area
  and isn't in view, it asks for it to be scrolled in, once for each selection (a new selection
  changes what the build panel shows), as a spotlight does. Until the control is in view, or if
  the player scrolls it away again, the sentence stays on screen without an arrow, on the same
  plain backing at the top centre of the map. While a prompt is open an instruction isn't drawn
  at all.
- **Automatic UI moves.** Switching the Planning tab, scrolling the build panel to a card, and
  framing the camera. Tab switches go through `dispatch` as `UiAction`s like any click (AGENTS.md:
  `dispatch` is the only place a click becomes a resource write); the camera and scroll moves are
  requests the owning systems honour.

**Anchors.** Panels that own a target widget record its screen rect in a per-frame
`TutorialAnchors` resource when they draw it (cash field, the map, battery card and its *Build
plant* button, the build panel's line section and its button that takes a line on from the
player's battery, *Start the day*, the speed controls, the Review Construction row). The overlay pass runs
after the panels in the same frame and reads those rects. A spotlight whose anchor was not drawn this frame doesn't show, and its timer doesn't start,
until the anchor appears. An instruction whose anchor is absent shows its sentence, without an
arrow, at the top centre of the map instead, so guidance never disappears.

### 4.2 Step model

The tutorial is an ordered table of steps in the `game` crate, `TUTORIAL_STEPS`. Each step names:

- the **phase** it belongs to (Planning, Operating, Review);
- its **presentation**: crawl, automatic move, spotlight, instruction, or a spotlight followed by
  an instruction;
- its **anchor** and **text key** (`assets/locales/en-US/ui.ftl`, keys also in
  `l10n/keys.rs`);
- for instructions, a **done** predicate over the game state and client UI state.

A pure function, `tutorial_model(progress, game, ui) -> TutorialView`, picks what to show. The
active step is the first step of the *current* phase that is neither done nor already played (for
one-shot spotlights, crawls and moves). Steps of a phase that has passed are dropped. This makes
the tutorial robust to the player going off-script: if they queue a battery before being asked,
the battery steps simply never show; if they press *Start the day* early, the remaining Planning
steps are skipped and the Operating step plays.

`TutorialProgress` (a Bevy resource) holds only what state can't tell us: which one-shot steps
have played, the current spotlight's timer, and whether the tutorial is running. It lives in
memory for the running game.

Presenter pattern as AGENTS.md requires: `tutorial_model` is pure and unit-tested; a draw
function returns `Vec<UiAction>` (end spotlight, end tutorial, switch tab); a system hands them
to `dispatch`.

### 4.3 The first month, step by step

**Planning, first day**

1. **Frame the starter grid.** On entering Planning, the camera frames the home zone's starter
   grid (its cities, plants and lines; `world/focus.rs` `zone_bounds` computes it). Multi-area
   regions have the domain's island map from the start, but single-area regions (Western,
   Eastern, Brazil SIN, …) get it only at the first month close, so `zone_bounds` falls back to
   the network's energised AC component that holds the anchor node.
2. **Crawl.** The goal, in four or five short lines, roughly: *"You run a power company. Connect
   more cities to sell more electricity and make more money. But every customer you connect needs
   power, every minute. Keep supply equal to demand, or the lights go out."* Click or Escape
   skips it. Its accessible name is the full text.
3. **Switch to Forecast.** The bottom panel switches to the Forecast tab automatically.
4. **Spotlight cash.** The top bar's cash field. Note: *"This is your money. Grow it by selling
   power, without blacking out your customers."* Doesn't wait.
5. **Instruction: pick a site.** After the screen undims: *"Click a square on the map to see what
   you can build there."*, inside the map's top edge with its arrow pointing down into the map
   (§4.1). Done when a map cell is selected, or a battery has been queued.
6. **Battery card.** The build panel scrolls the battery card into view, a spotlight highlights
   it with *"Batteries store power when it's cheap and release it when demand spikes."*, then an
   instruction stays: *"Build a battery here."*, as an arrow callout (§4.1) to the right of the
   battery card's *Build plant* button, its arrow pointing at that button (the card can be
   taller than the build panel's view at large UI zoom, so the button, not the card, is what it
   scrolls into view if needed). Done when the player has queued (or owns) a
   battery that wasn't in the starter fleet (Southern China's starter fleet already has one).
   A plant under construction already has a network node, so it can be a line's endpoint, as
   step 7 needs. If the player deselects the cell, step 5's
   instruction shows again, because step 5 is no longer done.
7. **Connect it.** A spotlight on the build panel's line section with *"A power plant only helps
   once a line connects it to your grid."*, then an instruction: *"Build a line from your battery
   to your grid."*, its arrow pointing at the first *Line from* button that starts at a battery
   the player ordered (ordering the battery selects it, so its button is first). Once the line
   starts at that battery, the arrow moves to the first target's *Build line* button. Each is
   scrolled into view if needed. If the player starts the line somewhere else, there is no
   button to point at and the sentence shows at the map's top. Done when some player-built
   battery has a line in service or under construction that touches its node.
8. **Start the day.** A spotlight on *Start the day*, then an instruction: *"Ready? Start the
   day."*, its arrow pointing at *Start the day*. Done when the phase leaves Planning.

**Operating, first day**

9. **Spotlight the speed controls.** *"Things move slowly at first. Speed up the day here if you
   like."* Doesn't wait for the player to use them.

**Review, first month**

10. **Spotlight Construction.** The Review's Construction row. Construction is paid in **equal
    monthly instalments** (main spec §2.5 and §26), so the first month usually shows only the
    first instalment. Note: *"This is your first payment for the battery and its power line.
    You'll pay a share every month until they're built."* *(Builder's call:)* choose the note from
    the actual orders. If both finished within the month (India's battery builds in one month,
    and a short line can too), say *"…its power line. They're paid off."* If no battery or line
    was queued (the player went off-script), use a general note: *"Construction is what you pay
    each month for things you've ordered."*
11. **Continue** when this Review is dismissed: begin the new-supply walkthrough (§4.5).

### 4.4 Text and contrast

The main spec's 4.5:1 contrast floor (§8.4) applies to every tutorial text, including the crawl:

- Spotlight notes sit on opaque cards using the shared palette; an instruction's text sits on an
  opaque, unoutlined backing in the panel fill, including over the map.
- The crawl's backdrop is translucent, as the user asked, but dark enough that its text meets
  4.5:1 against the backdrop composited over **white**, the worst case for the 3D map beneath.
- The crawl text **clips** at a horizon line rather than fading out, because the main spec
  forbids opacity fades that make visible text unreadable.

### 4.5 After the first Review: build and connect a generator (2026-11-01)

Once the first month's representative day has ended and the player dismisses its Review, guide
increasing supply in the next Planning phase. Never replay the first month's battery or Start
steps, even if the player skipped them.

Recommend **solar in Brazil SIN and Southern China**, where gas costs exceed the starting
tariff; recommend **CCGT in all other regions**. Use that choice throughout the walkthrough:
card reveals, build and connection anchors, text, new-order detection and completion. Starter
plants never count as the player's order, and ordering another kind does not complete the step.

1. Spotlight the map with a short explanation: batteries store electricity; a new power plant
   adds supply.
2. Ask the player to select a map square for the recommended plant. If a cell is already selected,
   use it.
3. Scroll to and spotlight its card. For CCGT, explain that it is a gas plant that can be built
   anywhere and is relatively cheap. For solar, explain that it builds quickly, needs no fuel,
   generates during daylight and lets batteries store some of that energy for later.
   **State its exact build duration**, using the effective
   plant specification for the selected cell (including economic-zone overrides), with the same
   month formatting as the build card. Do not hard-code a region's tuning values.
4. Point at its Build plant button. Keep its exact build duration in the instruction too.
   Re-selecting a site updates the duration. A starter plant never satisfies this step; a new
   player-ordered plant of the recommended kind does, including an order placed ahead of the walkthrough.
5. Spotlight the line section, then point at Line from the new plant and its first enabled Build
   line target that already has a complete existing or ordered path to a service grid. Use the
   same controls and presentation as for the battery. Explain that both plant and line must finish before
   supplying power. Orders under construction count, because they already have network nodes.
6. Once the new plant has a complete path to a service grid, including ordered lines, spotlight
   the top bar's **Save** button. Caution the player to create a **named snapshot** before risky
   moves such as connecting a new city, so they can load it to return to this point if things
   go wrong. A dangling line to an unconnected city does not unlock this caution.
7. After the Save reminder, spotlight the **crystal ball** in the top bar. Explain that it
   compares demand and planned supply over the next twelve months, and that choosing a city
   previews its added demand once the proposed connection is built, before the player places
   the order. Point at the actual icon; opening the outlook remains the player's action.
   In the solar walkthrough, also explain that rated capacity is not available throughout the
   day: solar produces nothing at night. The outlook must be checked before expanding demand.
8. After the player has ordered and connected the generator and opened the annual outlook,
   offer the plant-payoff comparison in the following month's Planning phase. Highlight
   construction cost and duration, then operating returns, and ask the player to close the
   comparison. Closing early skips the remaining comparison notes and proceeds to §4.6.
   The tutorial does not save or open dialogs automatically. Starting a day early while
   optional guidance is available ends it; leaving the game part-way keeps the tutorial owed.

All existing tutorial interaction, contrast, popup precedence and simulation-isolation rules
continue to apply. The plant card, Build plant and connection-button anchors are shared between
chapters; the current chapter chooses the plant kind and new-plant node set.

### 4.6. Renewable siting

After the plant-payoff comparison closes, spotlight the map-layer controls with one final note:
hydro, solar, wind and geothermal yield depends on location. Direct players to compare Hydro
potential, Sunshine, Wind potential and Geothermal potential, then click candidate cells to
inspect their resource readings before building. Favor stronger resources, while explaining
that geothermal exploration can still fail. The note applies in every region, including ones
without usable geothermal resources; it requires no new construction or resource availability.

Anchor the spotlight to the layer-button row or compact layer selector in the world view and
the layer row in the flat map. Use the existing spotlight timing, input handling and dialog
precedence. Only dismissal or timeout completes the tutorial in Planning; closing the payoff
comparison alone does not. Starting the day early still skips the optional remaining guidance.

## 5. Where it fits the existing code

- **Player progress gets its own file, not `preferences.ron`.** `tutorial_pending: bool` (default
  `true`) and `played_regions: BTreeSet<String>` (default empty) live in a new
  `ClientProgress` record, stored as `progress.ron` beside `preferences.ron` (and under its own
  key in browser storage). *(Builder's call.)* Two reasons. Preferences are best-effort: an
  unreadable file is silently replaced with defaults on the next window move, which is fine for
  panel sizes but would wipe unlock progress. And `ClientPreferences` rejects unknown keys, so
  an older or stale-cached build would reset the whole preferences file on seeing new keys.
  Progress follows a stricter rule: **a progress file that exists but fails to read is never
  overwritten.** The game runs with in-memory defaults, logs the error, and leaves the file
  alone. Writes use the same atomic write and directory sync as preferences. Validation bounds
  `played_regions` (count and id length). Tests follow the missing-field rule: delete the key and
  assert the default; an unknown key still fails.
- The tutorial lives in the `game` crate as a `hud/tutorial.rs` module plus a `TutorialPlugin`,
  with tests as modules of `crates/game/tests/game.rs` under `tests/game/`.
- The region-played tracker (§6) is an application-layer projection beside `MilestoneProjection`.
  Checkpoints restore projections from snapshot bytes, not by replay, so adding it is a **portable
  state schema bump from 7 to 8**. That means changing the `CURRENT_STATE_SCHEMA` and
  `SUPPORTED_STATE_SCHEMAS` constants in `crates/domain/src/state.rs`, adding compatibility
  fixtures for schema-7 saves, and nothing else in the domain.

## 6. "Played" regions and the SGCC lock

**Definition.** A region counts as **played** when, in a game in that region, the player has
connected a city beyond the starting service territory (a city not in the region's
`start.connected_cities`) and then **twelve months have closed** since that connection. A game that
ends (game over) or is abandoned before then doesn't count. Disconnecting the city later doesn't undo
it.

**Tracking.** A new application projection, `ExpansionProjection`, counts the month closes since
the first extra city connected (saturating at 12) and reports `played()`. A `game` system adds the
session's region id to `played_regions` the first time `played()` becomes true, and writes
`progress.ron`. Recording is idempotent and survives deleting saves.

**Saves made before this feature** (schema ≤ 7) have no record of the connection month. *(Builder's call:)* on
restore, seed the projection from the session as it stands. If an extra city is already
connected, the count of month closes starts at zero from the loaded month. It can come late but
never early. Otherwise the count starts normally at the first connection.

**The lock.** A region's file may carry optional presentation metadata, mirroring `atlas`:
`unlock: (regions_played: 4)`. It is optional in the strict region-file parser (as `atlas` is),
and the region listing that the picker reads must expose it. Today `atlas` is read only by
`region_outline`, not by the listing. The domain never sees it. `sgcc.ron` gets `regions_played: 4`. A locked region's picker card is disabled, keeps its
name, description and globe fill (the fill drawn muted), and shows *"Play 4 other regions to
unlock — 2 of 4 so far"*. Only *other* regions count toward the threshold. The lock applies only to
starting a new game: loading or continuing an existing SGCC save is unaffected.

**Existing players start locked too** (the user decided). Everyone's `played_regions` starts empty,
including people who already play SGCC. Their SGCC saves still load.

**The unlock flag.** A hidden developer flag, `--unlock-all-regions`, sits beside
`--exit-after-frames` in `paths.rs`. It inserts an `UnlockAllRegions` resource that makes the
picker ignore every `unlock` threshold for that run. It never writes `played_regions`, so it
leaves no trace once the game is launched without it. Game tests that start SGCC through the
picker insert the same resource. The lock is a picker lock only. A `NewGameRequest` written
directly, as most game tests do, is not gated. Desktop only: the browser build has no command line.

## 7. Testing

- **Presenter:** `tutorial_model` unit tests for each step's selection, including off-script
  orders (battery queued before step 5; *Start the day* pressed at step 5; cell deselected at step
  6) and anchor-absent cases.
- **Property:** for arbitrary sequences of tutorial `UiAction`s, `dispatch` never produces a
  `PlayerCommand`. A replay digest is identical with the tutorial on and off.
- **Glue:** each tutorial `UiAction` goes through the real `dispatch` (memory:
  presenter-dispatch glue tests). Injected actions wait for the gate that enables the control.
- **Fit:** spotlight cards and instructions fit the minimum window at maximum UI zoom.
- **Contrast:** the crawl's text against its backdrop over white; the notes against their cards.
- **Preferences:** round-trip, defaults when keys are absent, validation bounds.
- **Projection:** `ExpansionProjection` property tests: played exactly at the twelfth month close
  after the first extra connection, never before, never without an extra city; snapshot bytes
  round-trip at schema 8, and a schema-7 snapshot restores with a seeded projection.
- **Picker:** SGCC disabled at 3 other played regions, enabled at 4; its own id never counts.
- **Launch smoke:** `cargo run -p game -- --exit-after-frames 120` after adding the plugin.

## 8. Left to the builder

- How the crawl is rendered. Suggested: lay out the text as an egui galley, tessellate it into a
  mesh and apply a perspective transform to its vertices.
- Exact timings (fades about 0.3 s, holds about 4 s, crawl speed slow enough for a young reader).
- Exact wording, within the meaning and length given above.
- The exact camera framing margin.
