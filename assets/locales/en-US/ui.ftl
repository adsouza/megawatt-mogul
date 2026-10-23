# Interface text: screens, panel labels and the display names of typed values.
#
# Every literal key the HUD uses is listed in `crates/game/src/l10n/keys.rs` (ALL_UI_KEYS), and a
# coverage test checks that each one exists here. Values that the code formats (money, power,
# percentages, clock times) arrive as ready-made text, so messages never reformat them.

## Names of typed values

phase-planning = Planning
phase-operating = Operating
phase-review = Review

season-winter = Winter
season-spring = Spring
season-summer = Summer
season-autumn = Autumn

plant-kind-nuclear = Nuclear plant
plant-kind-coal = Coal plant
plant-kind-ccgt = Gas plant (CCGT)
plant-kind-peaker = Gas peaker
plant-kind-solar = Solar farm
plant-kind-hydro = Hydro plant

plant-status-under-construction = { $months ->
    [one] Under construction: 1 month left
   *[other] Under construction: { $months } months left
}
plant-status-offline = Off
plant-status-starting = Starting: { $time } left
plant-status-online = Online
plant-status-stopping = Stopping

game-over-bankrupt = Your company went bankrupt: cash stayed negative at too many month-ends.
game-over-license-revoked = The regulator revoked your operating license after too many strikes.

name-plant = { $kind } { $id }
name-plant-unknown = Plant #{ $id }
name-city-unknown = City #{ $id }
name-line = Line { $id }

milestone-serve-peak = Serve a monthly peak of { $gigawatts } GW
milestone-full-season-without-strike = Get through a full season without a strike
milestone-connect-cities = Connect { $count } cities
milestone-progress-peak = Best monthly peak so far: { $best } of { $goal }
milestone-progress-strike-free = { $months } of { $needed } strike-free months this season
milestone-progress-cities = { $connected } of { $needed } cities connected

## Shell: boot, menu and region select

app-title = Megawatt Mogul
boot-error-title = The game could not start
ui-quit = Quit
ui-back = Back
ui-continue = Continue
ui-close = Close
menu-eyebrow = THE POWER COMPANY SIMULATOR
menu-subtitle = Build your grid. Keep the lights on.
menu-continue = Resume game
menu-continue-details = { $name } · { $region } · { $month } · { $phase } · { $time }
menu-new-game = New game
menu-seed = Seed
menu-seed-invalid = The seed must be a whole number, such as 42.
region-select-title = Choose your region
region-select-empty = No regions were found. Check that the game's assets folder is complete.
region-select-unavailable = Unavailable

## Shell: top bar, toasts and game over

topbar-cash = Cash
topbar-reliability = Reliability
topbar-satisfaction = Satisfaction
topbar-strikes = Strikes
topbar-strikes-value = { $strikes }/{ $limit }
topbar-menu = Menu
leave-game-question = Leave this game? It can't be resumed yet.
leave-game-leave = Leave
leave-game-stay = Stay
topbar-milestones = Milestones
recording-stopped = Recording stopped: { $reason }
topbar-warnings = { $count ->
    [one] ⚠ { $count } warning
   *[other] ⚠ { $count } warnings
}
toast-repeated = { $text } (×{ $count })
game-over-title = Game over
game-over-final-cash = Final cash
game-over-reliability = Reliability
game-over-satisfaction = Satisfaction
game-over-see-month = See the month
game-over-main-menu = Main menu

## Planning: map, inspector, build menu

map-label = Map
map-layer-terrain = Terrain
map-layer-irradiance = Sunshine
map-solar-scale = Solar potential
map-solar-low = 0%
map-solar-mid = 50%
map-solar-high = 100%
map-solar-help = Site solar potential: purple/blue is lower, green/yellow is higher. The same 0–100% scale applies in every region. This is a game site rating, not today's weather or the percentage of sunny days; season, time of day and clouds also affect solar output.
map-layer-cooling-water = Cooling water
map-layer-hydro-potential = Hydro potential
map-layer-suitability = Suitability: { $kind }
legend-label = Map legend
# The layer-button row's toggle that shows or hides the map legend.
legend-toggle = Legend
legend-city-connected = Connected city
legend-city-unconnected = Unconnected city
legend-plant-under-construction = Plant under construction
legend-line-under-construction = Line under construction
# Beside a plant or line under construction on the map: its months left, kept short.
map-months-left = { $months } mo
# One letter for each kind of plant on the map.
plant-glyph-nuclear = N
plant-glyph-coal = C
plant-glyph-ccgt = G
plant-glyph-peaker = P
plant-glyph-solar = S
plant-glyph-hydro = H
inspector-title = Cell ({ $x }, { $y })
inspector-land = Land
inspector-water = Water
inspector-height = Height
inspector-irradiance = Sunshine
inspector-cooling-water = Cooling water
inspector-hydro-potential = Hydro potential
inspector-can-build = Can be built here
inspector-site-ok = ✓
inspector-select-cell = Select a cell to see what can be built there
build-title = Build
build-plant = Build plant
build-plant-heading = Plant at ({ $x }, { $y })
build-plant-select-cell = Select a cell on the map to build a plant there.
build-capacity = Capacity
build-capital = Capital cost
build-months = Build time
build-months-value = { $months ->
    [one] 1 month
   *[other] { $months } months
}
build-marginal-cost = Fuel cost
build-line = Build line
build-line-heading = Transmission line
build-line-hint-from = Choose where the line starts.
build-line-hint-to = Starting at { $node }. Choose where it ends.
build-line-from = Line from { $node }
build-line-to = Line to { $node }
build-line-cancel = Cancel drawing
line-in-service = In service
line-cancel-construction = Cancel construction
line-cancel-question = Cancel construction of { $line }? Already-paid costs will not be refunded. No further instalments will be charged.
line-cancel-confirm = Cancel this line
line-cancel-keep = Keep building
error-unknown-line = { $line } no longer exists.
error-line-not-under-construction = { $line } is already in service and cannot be cancelled.
event-line-construction-cancelled = Construction of { $line } cancelled.
build-line-cells = Length
build-cells-value = { $cells ->
    [one] 1 cell
   *[other] { $cells } cells
}
name-node-unknown = Grid point { $id }

## Planning: tariff, commitments and forecast

tariff-title = Tariff
tariff-current = Current tariff
tariff-allowed-return = Allowed return
tariff-review-heading = Projected rate review
tariff-projected-return = Projected allowed return
tariff-reserve-margin = Reserve margin
tariff-held-capital = Capital held out of the rate base
tariff-capital-release = Capital released back
tariff-capital-at-risk = Capital at risk of write-off
tariff-review-months = { $months ->
    [0] The review runs when this month closes.
    [one] The December review is 1 month away.
   *[other] The December review is { $months } months away.
}
commitment-title = Commitments
commitment-forecast = Forecast demand
commitment-committed = Committed output
commitment-empty = No plants are ready to commit yet.
commitment-cell = { $plant } at { $time }: { $state }
commitment-state-on = committed
commitment-state-off = off
commitment-hour-header = Show { $time } in the grid
commitment-apply = Apply
commitment-revert = Revert
commitment-all-day = All day
commitment-off = Off
commitment-agc = AGC
commitment-agc-tip = Automatic generation control: the plant follows the load by itself. After a system-wide trip, it automatically restarts if it was running or starting, is connected, and can finish starting by midnight. Start-up time and costs still apply.
commitment-target = Initial target
commitment-target-set = Set
commitment-target-range = { $min }–{ $max } MW
commitment-hydro-budget = Water for the day: { $energy }
commitment-unsaved = Apply or revert your changes before starting the day.
forecast-title = Forecast
forecast-band = Forecast range
forecast-solar = Expected solar
forecast-net-supply = Net supply
forecast-axis-hour = Hour of day
forecast-axis-power = Power (MW)
warnings-title = Warnings
warnings-none = No warnings: the commitments cover the forecast.
warning-shortfall = { $time }: short by { $amount }. Expect a rotating outage.
warning-min-gen = { $time }: committed minimum exceeds load by { $amount }.
warning-hydro-dry = { $plant } has no water to spare today.
warning-show-hour = Show hour
warnings-more = +{ $count } more
warnings-less = Hide
start-operating = Start the day
shortfall-start-question = The forecast is short by up to { $amount } (worst at { $time }). Expect rotating outages if you start the day now.
shortfall-start-anyway = Start anyway
shortfall-start-keep-planning = Keep planning
# The Construction tab (spec §16.9), and the card of a selected plant under construction.
construction-title = Construction
construction-title-count = Construction ({ $count })
construction-empty = Nothing under construction.
construction-months-left = { $total ->
    [one] { $left } of 1 month left
   *[other] { $left } of { $total } months left
}
construction-in-service = In service from { $month }
# A line under construction on the tab: its name and the two ends it joins.
construction-line-name = { $line }: { $from } to { $to }
construction-card-status = Under construction

## Operating

speed-pause = Pause
speed-1x = 1×
speed-2x = 2×
speed-4x = 4×
speed-8x = 8×
speed-16x = 16×
speed-32x = 32×
frequency-title = Frequency
frequency-imbalance = Imbalance
grid-mode-normal = Normal
grid-mode-blackout = Blackout
outage-rotating = Rotating outage: { $share } of demand
outage-target = ordered: { $share }
shed-share = Protection has shed { $share } of the load
plant-list-title = Plants
plant-list-agc = AGC
plant-select-hint = Select a plant in the list or on the map.
plant-output = Output
plant-output-capacity = Output / capacity
plant-output-capacity-value = { $output } / { $capacity }
plant-grid-connection = Grid connection
plant-grid-connected = Connected
plant-grid-disconnected = Not connected
plant-min-output = Minimum output
plant-initial-target = Initial target
plant-startup-time = Start-up time
plant-startup-cost = Start-up cost
plant-fixed-om = Fixed O&M / month
plant-specifications = Specifications
plant-hydro-available = Water available today
plant-target = Target
plant-target-slider = Set target
plant-target-none = none
plant-ramp = Ramp rate
plant-marginal-cost = Marginal cost
plant-energy-today = Energy today
# The plant's weighted energy and its revenue over every month closed so far.
plant-closed-months = Closed months
plant-closed-months-value = { $energy }, { $revenue }
plant-hydro-budget = Water left today
plant-start = Start
plant-stop = Stop
# The city card (spec §16.8), in Planning's inspector and Operating's side panel.
city-connected = Connected
city-not-connected = Not connected
city-connect-hint = Build a line to it from the build menu to connect it.
# Instead of the hint while a line to the city is under construction: the month it enters service.
city-line-under-construction = A line to it is under construction: connected from { $month }
city-base-load = Base load
city-peak = Peak today
# What an unconnected city would add to demand, at the day's peak, once a line reaches it.
city-would-add = Would add at peak
city-peak-value = { $power } at { $time }
city-demand-now = Demand now
city-served-now = Served now
# The share of the city's demand a rotating outage or protection's shed cuts now.
city-cut = Cut
operating-tab-supply = Supply
supply-history-title = History
supply-history-month = Month
supply-history-note = The complete played day representing this month.
supply-history-empty = Complete a day to see its supply and demand history.
review-supply = Supply demand retrospective
review-summary = Review summary
supply-chart-title = Supply and demand
supply-chart-demand = Demand
supply-chart-served = Served
supply-chart-forecast = Forecast
supply-chart-marginal-cost = System marginal cost
supply-chart-axis-cost = Marginal cost ($/MWh)
ramp-warnings-title = Ramp warnings
ramp-warnings-none = No ramp warnings.
ramp-warning-row = { $time }: { $warning }
hydro-title = Hydro
hydro-level = Hydro covers everything above { $level }
hydro-no-level = Hydro has no water to spare today
hydro-frozen = Hours already played are frozen
hydro-none = No hydro plant is scheduled today.
hydro-row = { $plant }: { $level }
incidents-title = Incidents
incidents-none = No incidents today.
incident-row = { $time }: { $incident }

## Review

review-title = Month in review
review-pnl = Profit and loss
review-days-note = Each played day stands for { $days } days of the month.
review-revenue = Revenue
review-fuel = Fuel
review-start-up = Start-ups
review-fixed-om = Fixed operations and maintenance
review-construction = Construction
review-net = Net
review-merit-order = Merit order used
review-merit-plant = Plant
review-merit-kind = Type
review-merit-cost = Marginal cost
review-merit-energy = Energy
review-reliability = Reliability
review-served = Served
review-unserved-planned = Unserved (planned rotating outages)
review-unserved-unplanned = Unserved (protection shedding)
review-out-of-band = Time out of frequency band
review-incidents = Incidents
review-incidents-none = No incidents this month.
review-lessons = Lessons
review-regulatory = Regulator
review-regulatory-none = The regulator took no action this month.
review-rate-review = Rate review
review-rate-review-result = Your tariff is now { $tariff } and your allowed return is { $allowed_return }.
milestones-title = Milestones
milestones-reached = Reached
milestones-in-progress = In progress

# Region overview: map and accessible region cards share the same start action.
region-select-subtitle = Choose a grid. Build your power company. Keep the lights on.
region-select-map-title = North America
region-select-map-note = Hover to explore · Click a coloured region to begin
region-select-home = Start in { $city } · { $cities } cities to serve
region-select-east-description = Dense cities, big demand, and a legacy coal and nuclear fleet. Keep the industrial heartland humming.
region-select-west-description = Mountain crossings, desert sunshine, and Pacific Northwest hydro. Master the evening demand surge.

# Tabletop diorama
world-reset = Reset view (R)
world-overlay-hide = Hide overlay (O)
world-overlay-show = Show overlay (O)
world-effects-off = Disable effects
world-effects-on = Enable effects
world-effects-help = Soft glow around bright objects and gentle camera-focus blur. Your choice is remembered.
world-help = Drag to orbit · Shift + drag or right/middle drag to pan · Scroll to zoom at the pointer · R resets · O toggles overlay. Keyboard shortcuts work while pointing at the map.
world-copper-plate = Pillars: output / capacity · Shared city supply · Arrows: illustrative supply routes, not measured line flow
world-dev-title = Diorama inspector
world-dev-boundary = Visual tuning only. Simulation tuning lives in region data and applies to new games.
world-dev-entities = Render entities

world-help-label = Controls
world-control-room = Control room

world-layer-label = Map layer

# Saved-game names
save-rename = Rename
save-rename-title = Rename saved game
save-rename-body = Rename “{ $name }”. Saved progress and the save date stay the same.

# Construction is paid in monthly instalments, not upfront.
build-monthly-payment = Per month (approx.)

# Daily-cycle battery storage
plant-kind-battery = Battery storage
plant-glyph-battery = B
battery-mode-disabled = Disabled
battery-mode-disconnected = Not connected
battery-mode-holding = Holding charge
battery-mode-charging = Charging
battery-mode-discharging = Discharging
battery-mode-empty = Empty
battery-mode-full = Full
battery-mode-suspended = Grid suspended
battery-enable = Enable storage
battery-disable = Disable storage
battery-grid-charging-hint = Charges from the grid in daylight; thermal generation may supply it. Holds energy for the evening.
battery-stored = Stored energy
battery-charging = Charging now
battery-discharging = Discharging now
battery-input = Charge input
battery-delivered = Delivered energy
battery-losses = Conversion losses
battery-charge-loss = Charging losses
battery-discharge-loss = Discharge losses
battery-charge-rating = Max charging power
battery-discharge-rating = Max discharge power
battery-energy-rating = Energy capacity
battery-round-trip = Round-trip efficiency
battery-charge-window = Charging window
battery-evening-window = Discharge window
battery-window-value = { $start }–{ $end }
battery-cycle = Daily cycle
battery-cycle-hint = Empty at dawn; daylight charge, evening discharge, remainder cleared at midnight.
battery-total-load = Demand + battery charging
battery-stored-axis = Stored energy (MWh)
battery-midnight-marker = Unused energy cleared at midnight
battery-preview-charge = Expected charging (MW)
battery-preview-discharge = Expected discharge (MW)
battery-review-heading = Battery storage
battery-review-note = Energy totals cover the whole month. Unused energy is reported, then cleared at midnight each day; charging earns no customer revenue.
battery-unused = Unused at midnight (month)
battery-unused-daily = Unused at midnight (per day)
battery-fleet = All batteries

supply-chart-combine = Combine graphs
supply-chart-unit-power = MW
supply-chart-unit-cost = $/MWh
supply-chart-unit-energy = MWh
