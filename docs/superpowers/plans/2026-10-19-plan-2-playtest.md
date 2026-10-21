# Plan 2 playtest checklist

Plan 2's headless tests cover the presenters, the glue around `dispatch`, the real egui pass and a
whole month through the app. They can't watch a real window. This checklist is the part a person
has to look at. Run each step in order and tick it when the expected result matches. If a step
fails, note the step number and what you saw.

Use a debug build (`cargo run -p game`) with a window at the minimum size, 1024×640, unless a step
says otherwise. The window opens larger on a large monitor (step 7.1): drag it down to its minimum
first. Steps that need a fresh game say so; the others follow on from the step before.

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
  bottom panel with *Start the day*. The commitment grid opens already filled with the previous
  operator's schedule: every row all day, with *AGC* on.

## 2. Planning

- [ ] **2.1** Click each map layer button in turn: *Terrain*, *Sunshine*, *Cooling water*,
  *Hydro potential* and each *Suitability: …*.
  **Expected:** the map recolours for each, and the button of the layer on show is disabled. Plant
  letters (N, C, G, P, S, H) appear on the map only in a larger window.
- [ ] **2.1a** Look at the map and at the end of the layer-button row: a *Legend* toggle.
  **Expected:** in a fresh game the legend is open (the toggle on) in a corner of the map that
  covers no city or plant, usually the top right; at 1280×720 it shows in Eastern Planning, for
  instance. Only when every corner would hide a mark is it collapsed (at 1024×640 in Planning,
  for instance). It lists the six plant kinds by colour swatch, connected and unconnected cities,
  and the two under-construction marks. A click on the legend selects
  nothing. Click *Legend*: it closes; click again: it opens, in the clear corner or else the top
  right. Drag every panel to its maximum: the legend drops to two columns and stays inside the
  map. Start the day: the legend stays as you left it in Operating and the next month's planning.
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
- [ ] **2.5** On the Nuclear row of *Commitments* (all day at the start), click hours 12–23 off
  and then *Apply*.
  **Expected:** a toast says the gap *is too short* and how long the plant needs *to ramp down*
  and to start again; the row goes back to its committed pattern.
- [ ] **2.6** Look over the grid as it opened: every row *All day*, with *AGC* on.
  **Expected:** *Warnings* shows no *short by* line. (In spring and autumn a minimum-generation
  line, *committed minimum exceeds load*, can appear; otherwise it reads *No warnings: the
  commitments cover the forecast.*)
- [ ] **2.7** Click *Off* on two of the largest rows (coal, say).
  **Expected:** warnings like *HH:MM: short by … MW. Expect a rotating outage.*, one line plus
  *+N more* if there are several. *Show hour* tints that hour's column in the grid.
- [ ] **2.8** With those rows still *Off*, click *Start the day*.
  **Expected:** the day doesn't begin. A prompt asks first: *The forecast is short by up to …
  (worst at HH:MM). Expect rotating outages if you start the day now.*, with *Start anyway* and
  *Keep planning*. The hour and the amount are the first warning's. Click *Keep planning*: the prompt closes and the
  game is still in Planning. Click *Start the day* again, then click outside the prompt (on the
  dimmed map): it closes the same way. Put the rows back to *All day* with AGC.
- [ ] **2.9** Open the *Forecast* and *Tariff* tabs, then back to *Commitments*.
  **Expected:** the forecast chart (forecast range, expected solar, committed output) and the
  tariff with the projected rate review. The map doesn't move when a tab changes.
- [ ] **2.10** Toggle one hour on a row and, without applying, look at *Start the day*.
  **Expected:** it is disabled with *Apply or revert your changes before starting the day.*
  Click *Revert*.

## 3. Operating

- [ ] **3.1** Click *Start the day*.
  **Expected:** with no shortfall, no prompt: the day begins at once. The speed bar (time of
  day, *Pause*, *1×* to *8×*, with *1×* on); on the right, from the top, the frequency gauge with
  *Normal*, the selected plant's details (*Select a plant in the list or on the map.* until one
  is), and the *Plants* list filling the rest of the panel down to its bottom, with at least three
  rows in view; and the *Supply* and *Hydro* tabs at the bottom. The clock moves one game minute
  per quarter second.
- [ ] **3.2** Click *8×*, then *Pause*, then press Space twice.
  **Expected:** 8× moves the clock eight times as fast; *Pause* stops it; Space resumes at 8×
  and pauses again.
- [ ] **3.3** Run at 1× or 2× and watch the gauge.
  **Expected:** the needle stays in the green zone around nominal; the imbalance reads a few MW
  either way.
- [ ] **3.4** Select a coal plant in the list, turn its *AGC* off, drag *Set target* and let go.
  **Expected:** the plant's details fill the block above the list, every figure down to *Closed
  months* in view without scrolling, and the list doesn't move when the selection changes (select
  another plant, then click it again to deselect: the list stays put). The slider is enabled only
  with AGC off; nothing changes while dragging; on
  release the plant's target moves to the value, and its output ramps toward it at the ramp rate.
  Turn AGC back on.
- [ ] **3.5** Open *Hydro* (Western games show more), then *Supply*.
  **Expected:** each scheduled hydro plant's level (*Hydro covers everything above …*) and the
  note that hours already played are frozen; the supply chart with demand, served, forecast and
  the system marginal cost on its right axis.
- [ ] **3.6** Fresh game (Menu → Leave → New game, Eastern, seed 7). Click *Off* on most rows so
  the warnings show a large shortfall. Click *Start the day*: the prompt names the hour and the
  shortfall of the first warning. Click *Start anyway*, then *8×*.
  **Expected:** *Start anyway* begins the day, and the prompt is gone. Then a toast *Rotating
  outage ordered from HH:MM: …% of demand is cut in turn.*; the gauge shows *Rotating outage: …%
  of demand*; the incident appears under *Incidents*. Expect no blackout; if one happens, note
  the time.

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

- [ ] **6.1** Fresh game, Eastern, any seed. Commit nothing: click *Off* on every row (the grid
  opens with every row all day), then *Start the day*, *Start anyway* in the prompt, then *8×*;
  run to the close, click *Continue*, and repeat. The rows stay off from month to month, so each
  month's *Start the day* asks again.
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

## 7. Window and panel sizing

- [ ] **7.1** On a monitor larger than 1280×800 (logical), run `cargo run -p game`.
  **Expected:** the window opens centred on the primary monitor at about 80% of its width and
  height (on a 2560×1440 monitor, about 2048×1152), not at 1280×720. On a 1280×800 monitor or
  smaller it opens at 1024×640, or the whole monitor if that is smaller. Resize it by hand: it
  keeps your size, and it can't go below 1024×640.
- [ ] **7.2** Start a game (Planning). Hover the inner edge of the inspector, the build menu and
  the bottom panel in turn, then drag each edge toward the map.
  **Expected:** the cursor shows a resize arrow on each edge. Each panel follows the drag and the
  map shrinks to make room. Each stops at a maximum: a side panel at 40% of the window's width, the
  bottom panel at half its height, and short of that wherever the map would drop below 320×200.
  With both side panels dragged out, the map keeps its room and neither panel squeezes the other.
- [ ] **7.3** Drag each edge back past where it started.
  **Expected:** each panel stops at its opening size (inspector 260, build 300, bottom 240) and
  goes no smaller; the map returns to where it was.
- [ ] **7.4** Grow the bottom panel and a side panel, then select a cell, switch the bottom tabs,
  and start the day; in Operating, drag the side panel's and the bottom panel's edges the same way
  (side 300 to 40% of the width, bottom 200 to half the height). Select a plant and switch
  *Supply*/*Hydro*. Run the day to the close and click *Continue*.
  **Expected:** the map moves only while you drag. Selections, tabs and the phase changes leave
  every panel's size alone; back in Planning, and in the next day's Operating, each panel is as
  you left it. The charts, the commitment grid and the plant list use the extra room: a taller
  window, or a shorter bottom panel, gives the *Plants* list more rows while the plant's details
  keep their height.
- [ ] **7.5** At 1024×640 in Operating, select a hydro plant (Western) and drag the bottom panel's
  edge up as far as it goes.
  **Expected:** it stops after only a few pixels. The side panel still shows the gauge, every
  figure of the plant down to *Water left today*, and at least three rows of the *Plants* list.
  In a taller window the bottom panel can grow further, and the side panel keeps that minimum.

## 8. Selecting a city

- [ ] **8.1** In Planning, click a connected city (a filled circle) on the map, then an
  unconnected one (a hollow ring).
  **Expected:** the inspector shows the city's name, *Connected* or *Not connected*, its *Base
  load* and *Peak today* (*… MW at HH:00*). The unconnected city shows *Would add at peak* instead,
  and the hint *Build a line to it from the build menu to connect it.* Clicking a cell brings the
  cell card back.
- [ ] **8.2** Start the day and click a connected city on the map, then an unconnected one, then
  a plant in the *Plants* list.
  **Expected:** the block above the list shows the city's card: its name, connection, base load
  and peak, and for the connected city *Demand now* and *Served now*, which move with the day. The
  list doesn't move. Clicking the plant brings its details back. In the outage of step 3.6, a
  connected city also shows *Cut*, the share of its demand cut, the same for every connected city.

## 9. Construction progress

- [ ] **9.1** In Planning, order a plant (a gas peaker builds in 12 months) and a line from an
  unconnected city to its nearest node. Look at the bottom panel's tabs.
  **Expected:** the fourth tab reads *Construction (2)*. Before ordering it read *Construction*,
  and opening it then showed *Nothing under construction.* Click the city the line goes to: its
  card no longer asks for a line but says *A line to it is under construction: connected from
  Month Year*, the line's month on the tab.
- [ ] **9.2** Open *Construction*.
  **Expected:** one row per project, the one in service soonest first: its name (a line also
  names its two ends), a progress bar, *N of M months left*, and *In service from Month Year*.
  At 1024×640 the rows fit, and *Start the day* stays in view.
- [ ] **9.3** Grow the window until the map's plant letters show, and look at the new plant and
  line.
  **Expected:** a short label such as *12 mo* beside the faded square and above the dashed line's
  middle. In a small window, where the letters are hidden, the labels are too. Clicking on a
  label selects whatever is under it, not the project. The legend lists *Plant under
  construction* (a faded square) and *Line under construction* (a dashed line); if the new plant
  sits under the legend's corner, the legend moves to another clear corner.
- [ ] **9.4** Click the new plant's faded square on the map, then start the day and click it
  again.
  **Expected:** in Planning the inspector shows its construction card: its name, *Under
  construction*, *Capacity*, *N of M months left* with a bar, and *In service from Month Year*.
  In Operating the same card fills the block above the *Plants* list, which doesn't move.
- [ ] **9.5** Play months until the project is due.
  **Expected:** each month the months left go down by one and the in-service month stays the
  same. The commissioning toast arrives at the close of the month before the predicted month, in
  its Review; in the predicted month the project is gone from the tab, the line is solid, and the
  plant is in service (in the commitment grid, off and ready to start).

## Known visual risks to eyeball

These came out of the reviews and couldn't be checked headlessly. Look at each while running the
steps above.

- [ ] The ✓ glyph in the inspector renders in the default fonts (not a box).
- [ ] Panel widths look right, and the map's cells meet without seams.
- [ ] By default the legend (about 358×92 pt; two columns, about 260×132, with every panel at its
  maximum) opens in the first corner that covers no city or plant, and is collapsed when none is
  clear: in the shipped regions at 1024×640 in Planning (both regions) and Western Operating, and
  at 1280×720 in Western Planning. Everywhere else checked it opens in the top right. So every
  city can be clicked by default. Opened by the player with no clear corner, it goes in the top
  right and hides cities until it is closed. Judge whether a legend that moves between corners as
  plants are built is distracting, and whether the collapsed cases at 1024×640 leave a new player
  without a key.
- [ ] Map markers and the legend are legible.
- [ ] The supply chart's plot legends fit at about 700 px wide, and the right cost axis's ticks
  (not round numbers) read acceptably.
- [ ] The frequency dial looks right.
- [ ] Tab to a button and press Space: it doesn't both pause and press the button.
- [ ] In Review, map labels can run past the right edge (x up to about 1067 at width 1024).
- [ ] Toasts draw above the Review modal (`Order::Tooltip`); they never cover *Continue*.
- [ ] The Review modal stays under the Milestones window after you click it.
- [ ] Toasts float undimmed above the leave prompt and the game-over modal.
- [ ] The shortfall prompt's question reads cleanly at 1024×640 (one line, or a tidy wrap), with
  both buttons under it.
