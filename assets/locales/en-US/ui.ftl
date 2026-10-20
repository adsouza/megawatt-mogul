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
region-select-title = Choose your region
region-select-empty = No regions were found. Check that the game's assets folder is complete.
region-select-unavailable = Unavailable

## Shell: top bar, toasts and game over

topbar-cash = Cash
topbar-reliability = Reliability
topbar-satisfaction = Satisfaction
topbar-strikes = Strikes
topbar-menu = Menu
topbar-milestones = Milestones
recording-stopped = Recording stopped: { $reason }
toast-repeated = { $text } (×{ $count })
game-over-title = Game over
game-over-final-cash = Final cash
game-over-reliability = Reliability
game-over-satisfaction = Satisfaction
game-over-main-menu = Main menu

## Planning: map, inspector, build menu

map-layer-terrain = Terrain
map-layer-irradiance = Sunshine
map-layer-cooling-water = Cooling water
map-layer-hydro-potential = Hydro potential
map-layer-suitability = Suitability
inspector-title = Cell
inspector-land = Land
inspector-water = Water
inspector-height = Height
inspector-irradiance = Sunshine
inspector-cooling-water = Cooling water
inspector-hydro-potential = Hydro potential
inspector-can-build = Can be built here
build-title = Build
build-plant = Build plant
build-line = Build line
build-line-from = Line from
build-line-to = Line to
build-capacity = Capacity
build-capital = Capital cost
build-months = Build time
build-marginal-cost = Fuel cost

## Planning: tariff, commitments and forecast

tariff-title = Tariff
tariff-current = Current tariff
tariff-projected-return = Projected allowed return
tariff-reserve-margin = Reserve margin
tariff-held-capital = Capital held out of the rate base
commitment-title = Commitments
commitment-forecast = Forecast demand
commitment-committed = Committed output
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
plant-output = Output
plant-target = Target
plant-ramp = Ramp rate
plant-marginal-cost = Marginal cost
plant-energy-today = Energy so far
plant-revenue-today = Revenue so far
plant-agc = Automatic generation control (AGC)
plant-start = Start
plant-stop = Stop
supply-chart-title = Supply and demand
supply-chart-demand = Demand
supply-chart-served = Served
supply-chart-forecast = Forecast
supply-chart-marginal-cost = System marginal cost
ramp-warnings-title = Ramp warnings
hydro-title = Hydro
hydro-level = Hydro covers everything above { $level }
hydro-no-level = Hydro has no water to spare today
hydro-frozen = Hours already played are frozen
incidents-title = Incidents

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
