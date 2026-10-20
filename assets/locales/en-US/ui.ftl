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

## Shell: boot, menu and region select

app-title = Megawatt Mogul
boot-error-title = The game could not start
ui-quit = Quit
ui-back = Back
ui-continue = Continue
ui-close = Close
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
toast-repeated = { $text } (×{ $count })
game-over-title = Game over
game-over-final-cash = Final cash
game-over-reliability = Reliability
game-over-satisfaction = Satisfaction
game-over-main-menu = Main menu

## Planning: map, inspector, build menu

map-label = Map
map-layer-terrain = Terrain
map-layer-irradiance = Sunshine
map-layer-cooling-water = Cooling water
map-layer-hydro-potential = Hydro potential
map-layer-suitability = Suitability: { $kind }
legend-label = Map legend
legend-city-connected = Connected city
legend-city-unconnected = Unconnected city
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
build-line-cancel = Cancel line
build-line-cells = Length
build-cells-value = { $cells ->
    [one] 1 cell
   *[other] { $cells } cells
}
name-substation = Substation
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
commitment-agc-tip = Automatic generation control: the plant follows the load by itself.
commitment-target = Initial target
commitment-target-set = Set
commitment-target-range = { $min }–{ $max } MW
commitment-hydro-budget = Water for the day: { $energy }
commitment-unsaved = Apply or revert your changes before starting the day.
forecast-title = Forecast
forecast-band = Forecast range
forecast-solar = Expected solar
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

## Operating

speed-pause = Pause
speed-1x = 1×
speed-2x = 2×
speed-4x = 4×
speed-8x = 8×
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
plant-target = Target
plant-target-slider = Set target
plant-target-none = none
plant-ramp = Ramp rate
plant-marginal-cost = Marginal cost
plant-energy-today = Energy so far
plant-revenue-today = Revenue so far
plant-hydro-budget = Water left today
plant-agc = Automatic generation control (AGC)
plant-start = Start
plant-stop = Stop
operating-tab-supply = Supply
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
review-reliability = Reliability
review-served = Served
review-unserved-planned = Unserved (planned rotating outages)
review-unserved-unplanned = Unserved (protection shedding)
review-out-of-band = Time out of frequency band
review-incidents = Incidents
review-lessons = Lessons
review-regulatory = Regulator
review-rate-review = Rate review
milestones-title = Milestones
milestones-reached = Reached
milestones-in-progress = In progress
