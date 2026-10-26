# Things that just happened: toasts for domain events, the incident ticker, strike reasons and
# the ramp warning. Events carry ids and typed values only; the words are all here.

## Toasts

event-blackout = Blackout! Protection tripped every plant to save the grid.
event-blackout-restored = The blackout is over: the last of the load is reconnected.
event-rotating-outage-ordered = Rotating outage ordered from { $time }: { $share } of demand is cut in turn.
event-rotating-outage-lifted = The rotating outage is lifted.
event-strike-issued = The regulator issued strike { $strikes }. { $reason }
event-frequency-notice = { $month }: frequency was out of band too often. This is a formal notice, not a strike, but another failure within a year earns a strike.
event-strike-expired =
    A strike lapsed after clean months. You now have { $strikes ->
        [0] no strikes
        [one] one strike
       *[other] { $strikes } strikes
    }.
event-license-revoked = The regulator has revoked your operating license.
event-bankrupt = Bankrupt: cash stayed negative at too many month-ends.
event-plant-tripped = { $plant } tripped offline. Other plants must cover its output until it is restarted.
event-over-frequency-trip = { $plant } was tripped by over-frequency protection: generation outran demand.
event-hydro-budget-exhausted = { $plant } has used up its water for the day and stops generating.
event-city-connected = { $city } is now connected and buying power.
event-plant-commissioned = { $plant } finished construction and operates automatically.
event-plant-commissioned-agc = { $plant } finished construction. All-day operation and AGC are enabled. You can change these in Planning.
event-line-commissioned = { $line } is in service.
event-rate-review-completed = Rate review complete: your tariff is now { $tariff } and your allowed return is { $allowed_return }.

## Strike reasons

strike-reason-unserved-energy = Too much energy went unserved this month.
strike-reason-frequency-performance = Frequency stayed out of its band too often this month.

## Incidents (the ticker and the review timeline)

incident-load-shed = Protection shed { $mw } of load to hold the frequency up.
incident-blackout = Blackout: protection tripped every plant.
incident-blackout-restored = Power restored after the blackout.
incident-over-frequency-trip = Over-frequency protection tripped { $plant }.
incident-rotating-outage = Rotating outage ordered: { $share } of demand cut in turn.

## Warnings

ramp-warning = Net load is about to rise by { $change } within { $minutes } minutes, but the fleet can only add { $capability } in that time. Start fast plants now.
