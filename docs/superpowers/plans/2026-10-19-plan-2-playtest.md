# Plan 2 playtest checklist

Plan 2's headless tests cover the presenters, the glue around `dispatch`, the real egui pass and a
whole month through the app. They can't watch a real window. This checklist is the part a person
has to look at. Run each step in order and tick it when the expected result matches. If a step
fails, note the step number and what you saw.

Use a debug build (`cargo run -p game`) with a window at the minimum size, 1024×640, unless a step
says otherwise. Steps that need a fresh game say so; the others follow on from the step before.

## 0. Launch smoke

- [ ] **0.1** Run `cargo run -p game -- --exit-after-frames 120`.
  **Expected:** a window titled *Megawatt Mogul* opens on the main menu, draws for about two
  seconds and closes by itself. The command exits with status 0 (`echo $?` prints `0`), and the
  log shows `boot complete` and no `ERROR` or panic.

## 1. Menus

- [ ] **1.1** Run `cargo run -p game`.
  **Expected:** the main menu shows the title, a *Seed* field holding a number, *New game* and
  *Quit*.
- [ ] **1.2** Type `abc` in *Seed*.
  **Expected:** *The seed must be a whole number, such as 42.* appears and *New game* is disabled.
  Put back a number, such as `7`.
- [ ] **1.3** Click *New game*.
  **Expected:** *Choose your region* lists both regions, the Eastern and the Western
  Interconnection, each clickable, and a *Back* button.
- [ ] **1.4** Click *Back*, then *New game* again, then the Eastern Interconnection.
  **Expected:** the game opens in Planning: the top bar (date, cash, reliability, satisfaction,
  strikes `0/3`, *Milestones*, *Menu*), the map, the cell inspector, the build menu and the
  bottom panel with *Start the day*.

## 2. Planning

- [ ] **2.1** Click each map layer button in turn: *Terrain*, *Sunshine*, *Cooling water*,
  *Hydro potential* and each *Suitability: …*.
  **Expected:** the map recolours for each, and the button of the layer on show is disabled. The
  legend lists the six plant kinds by colour swatch, connected and unconnected cities, and the
  substation. Plant letters (N, C, G, P, S, H) appear on the map only in a larger window.
- [ ] **2.2** Click a water cell (on *Terrain*, a blue one).
  **Expected:** the inspector says *Water*, and under *Can be built here* the peaker has a reason,
  not ✓. In the build menu the *Gas peaker* row shows that reason, with *Build plant* disabled.
- [ ] **2.3** Choose *Suitability: Gas peaker* and click a cell it marks as suitable.
  **Expected:** under *Can be built here* the peaker has ✓, and its *Build plant* is enabled.
  Don't click it.
- [ ] **2.4** In *Build line*, select a city or plant on the map, click *Line from …*, select
  another, and look at the quote.
  **Expected:** *Length* in cells, *Capital cost* and *Build time* in months, and an enabled
  *Build line*. An invalid pair (the same point twice, or two points already joined) shows the
  reason instead, with *Build line* disabled. Click *Cancel line*.
- [ ] **2.5** On the Nuclear row of *Commitments* (all off at the start), click hours 0–11 on and
  then *Apply*.
  **Expected:** a toast says the gap *is too short* and how long the plant needs *to ramp down*
  and to start again; the row goes back to its committed pattern.
- [ ] **2.6** Click *All day* and turn *AGC* on for every row.
  **Expected:** *Warnings* shows no *short by* line. (In spring and autumn a minimum-generation
  line, *committed minimum exceeds load*, can appear; otherwise it reads *No warnings: the
  commitments cover the forecast.*)
- [ ] **2.7** Click *Off* on two of the largest rows (coal, say).
  **Expected:** warnings like *HH:MM: short by … MW. Expect a rotating outage.*, one line plus
  *+N more* if there are several. *Show hour* tints that hour's column in the grid. Put the rows
  back to *All day* with AGC.
- [ ] **2.8** Open the *Forecast* and *Tariff* tabs, then back to *Commitments*.
  **Expected:** the forecast chart (forecast range, expected solar, committed output) and the
  tariff with the projected rate review. The map doesn't move when a tab changes.
- [ ] **2.9** Toggle one hour on a row and, without applying, look at *Start the day*.
  **Expected:** it is disabled with *Apply or revert your changes before starting the day.*
  Click *Revert*.

## 3. Operating

- [ ] **3.1** Click *Start the day*.
  **Expected:** the speed bar (time of day, *Pause*, *1×* to *8×*, with *1×* on), the
  frequency gauge on the right with *Normal*, the plant list, and the *Supply* and *Hydro*
  tabs at the bottom. The clock moves one game minute per quarter second.
- [ ] **3.2** Click *8×*, then *Pause*, then press Space twice.
  **Expected:** 8× moves the clock eight times as fast; *Pause* stops it; Space resumes at 8×
  and pauses again.
- [ ] **3.3** Run at 1× or 2× and watch the gauge.
  **Expected:** the needle stays in the green zone around nominal; the imbalance reads a few MW
  either way.
- [ ] **3.4** Select a coal plant in the list, turn its *AGC* off, drag *Set target* and let go.
  **Expected:** the slider is enabled only with AGC off; nothing changes while dragging; on
  release the plant's target moves to the value, and its output ramps toward it at the ramp rate.
  Turn AGC back on.
- [ ] **3.5** Open *Hydro* (Western games show more), then *Supply*.
  **Expected:** each scheduled hydro plant's level (*Hydro covers everything above …*) and the
  note that hours already played are frozen; the supply chart with demand, served, forecast and
  the system marginal cost on its right axis.
- [ ] **3.6** Fresh game (Menu → Leave → New game, Eastern, seed 7). Click *All day* with AGC on
  every row, then *Off* on most of them so the warnings show a large shortfall. Start the day, then
  click *8×*.
  **Expected:** a toast *Rotating outage ordered from HH:MM: …% of demand is cut in turn.*; the
  gauge shows *Rotating outage: …% of demand*; the incident appears under *Incidents*. Expect no
  blackout; if one happens, note the time.

## 4. Review

- [ ] **4.1** Let the day of step 3.6 run to the month close.
  **Expected:** the speed goes to paused and *Month in review* opens over the dimmed map: the
  month, *Profit and loss* (revenue, fuel, start-ups, fixed O&M, construction, net) with the
  days note, *Merit order used*, *Reliability* (planned and protection unserved shown apart),
  *Incidents*, *Lessons* (the rotating-outage lesson), *Regulator* and *Continue*. The top bar
  stays usable above it.
- [ ] **4.2** Click *Milestones*, then click the review.
  **Expected:** the Milestones window stays on top of the review.
- [ ] **4.3** Click *Continue*.
  **Expected:** the next month's Planning; the top bar shows the new month.

## 5. Banners

- [ ] **5.1** Make a 2 MB disk image for the logs and start the game on it:

  ```sh
  hdiutil create -size 2m -fs HFS+ -volname MMLogs /tmp/mmlogs.dmg
  hdiutil attach /tmp/mmlogs.dmg
  cargo run -p game -- --log-dir /Volumes/MMLogs
  ```

  Start a game, then fill the disk from a terminal:
  `dd if=/dev/zero of=/Volumes/MMLogs/filler bs=64k`. Start the day, click *8×* and play on.
  **Expected:** once the log's last block is full (up to a few minutes at 8×), the top bar shows
  *Recording stopped: …* with the reason, and the game keeps running. Quit, then
  `hdiutil detach /Volumes/MMLogs`.
  *(A read-only log directory doesn't do it: the log is already open, and writes through an open
  file still succeed.)*
- [ ] **5.2** Make a directory read-only (`mkdir /tmp/mm-ro && chmod a-w /tmp/mm-ro`) and run
  `cargo run -p game -- --log-dir /tmp/mm-ro`. Click *New game* and a region.
  **Expected:** a toast *The command log could not be written: …*, and the game stays on the
  region picker. Afterwards, `chmod u+w /tmp/mm-ro`.

## 6. Game over

- [ ] **6.1** Fresh game, Eastern, any seed. Commit nothing: click *Start the day*, then *8×*,
  run to the close, click *Continue*, and repeat.
  **Expected:** a strike at each close (the top bar counts `1/3`, `2/3`); at the third close
  *Game over* opens instead of the review, with the reason (the license revoked), final cash,
  reliability and satisfaction, *See the month* and *Main menu*.
- [ ] **6.2** Click *See the month*.
  **Expected:** the final month's *Month in review* opens over the dimmed map, with the lessons and
  the *Regulator* section (strike 3 and its reason, and the revoked licence), and *Back* in place of
  *Continue*. The top bar stays usable above it.
- [ ] **6.3** Click *Back*.
  **Expected:** the *Game over* modal returns, as before. *See the month* and *Back* can be repeated.
- [ ] **6.4** Click *Main menu*.
  **Expected:** the main menu, with a new seed in the field.

## Known visual risks to eyeball

These came out of the reviews and couldn't be checked headlessly. Look at each while running the
steps above.

- [ ] The ✓ glyph in the inspector renders in the default fonts (not a box).
- [ ] Panel widths look right, and the map's cells meet without seams.
- [ ] Western at 1024×640: the legend sits close above Edmonton; check it doesn't hide it.
- [ ] Map markers and the legend are legible.
- [ ] The supply chart's plot legends fit at about 700 px wide, and the right cost axis's ticks
  (not round numbers) read acceptably.
- [ ] The frequency dial looks right.
- [ ] Tab to a button and press Space: it doesn't both pause and press the button.
- [ ] In Review, map labels can run past the right edge (x up to about 1067 at width 1024).
- [ ] Toasts draw above the Review modal (`Order::Tooltip`); they never cover *Continue*.
- [ ] The Review modal stays under the Milestones window after you click it.
- [ ] Toasts float undimmed above the leave prompt and the game-over modal.
