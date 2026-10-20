# Why something did not work, and what to do about it. One message per error variant; a
# `CommandError` becomes a toast with exactly one of these (spec §8.4).

## Domain errors

error-invalid-ratio = A share must be between 0% and 100%.
error-invalid-month = Month { $month } is not a month of the year.
error-plant-not-dispatchable = { $plant } cannot be dispatched: its output follows the weather, not your orders.
error-plant-not-offline = { $plant } must be offline first. Stop it, then try again.
error-plant-not-online = { $plant } is not online, so it has no output to set. Start it first.
error-plant-not-running = { $plant } is neither starting nor online.
error-non-finite-megawatts = The target for { $plant } is not a number.
error-below-minimum-stable-output = { $plant } cannot run below { $min }, its minimum stable output. Set a higher target or stop the plant.
error-above-capacity = { $plant } cannot produce more than { $max }, its capacity.
error-commitment-infeasible = That gap of { $gap } is too short. The plant needs { $shutdown } to ramp down and { $startup } to start again.
error-duplicate-node = Grid point { $node } already exists.
error-unknown-node = Grid point { $node } does not exist.
error-line-to-self = A line cannot start and end at the same place.
error-duplicate-line = { $from } and { $to } are already connected.
error-duplicate-line-id = { $line } already exists.
error-unknown-plant = There is no plant #{ $plant }.
error-wrong-phase = You can't do that during { $phase }.
error-game-over = The game is over. { $reason }
error-invalid-site = { $kind } can't be built there. { $reason }
error-plant-not-commissioned = { $plant } is still under construction.
error-start-up-beyond-day = That start-up takes { $startup }, but only { $remaining } is left in the day. Start it earlier.

## Why a cell cannot host a plant

site-out-of-bounds = That spot is outside the map.
site-not-land = That spot is water, and plants need land.
site-insufficient-cooling-water = Cooling water here is { $have }, but this plant needs at least { $need }. Look near a river, lake or coast.
site-no-hydro-site = There is no river here to dam. Hydro needs a cell with hydro potential.

## Regions that could not be loaded

error-region-not-found = Region { $region } was not found.
error-region-io = Region { $region } could not be read: { $message }
error-region-parse = Region { $region } is not valid region data: { $message }
error-region-invalid = Region { $region } has a problem. { $detail }
error-region-id-mismatch = The file for region { $requested } says it is region { $found }, so it was not loaded.

## What is wrong inside a region file

region-error-grid-size-mismatch = The map has { $actual } cells but should have { $expected }.
region-error-missing-plant-spec = The region has no entry for { $kind }.
region-error-duplicate-plant-spec = The region has more than one entry for { $kind }.
region-error-city-out-of-bounds = City #{ $city } is outside the map.
region-error-city-not-on-land = City #{ $city } is not on land.
region-error-duplicate-city = City #{ $city } is listed twice.
region-error-unknown-start-city = The starting grid connects city #{ $city }, which does not exist.
region-error-invalid-start-site = The starting plant ({ $kind }) cannot be built where the region puts it. { $reason }
region-error-substation-not-on-land = The starting substation is not on land.
region-error-invalid-shape = The value for { $field } has the wrong shape.
region-error-invalid-band = The value for { $field } has an invalid band.

## The command log

error-command-log = The command log could not be written: { $message }
