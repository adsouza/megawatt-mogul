# What the closed month has to teach: what happened, then what to do about it. Parameters arrive
# as ready-made text (energy, money, prices, shares, clock times).

lesson-expensive-peaking = Gas peakers supplied { $energy } this month and burned { $cost } in fuel. A combined-cycle plant makes the same energy at { $alternative }, so cover the predictable evening demand with cheaper plants and keep peakers for the last few hours.
lesson-rotating-outages = Demand outran your fleet, so the grid cut { $energy } on purpose, up to { $peak_share } of demand at the worst hour. Build or commit more capacity before the evening peak.
lesson-unserved-energy = Protection shed { $energy } of demand, first at { $time }. That happens when generation falls short faster than plants can ramp, so keep spare fast-ramping capacity and turn on AGC.
lesson-overgeneration = Generation ran ahead of demand by { $energy }. Slow plants cannot back off quickly, so commit less baseload on days when demand is low.
lesson-overgeneration-trips =
    Generation ran ahead of demand by { $energy }, and over-frequency protection tripped { $trips ->
        [one] one plant
       *[other] { $trips } plants
    }. Slow plants cannot back off quickly, so commit less baseload on days when demand is low.
lesson-frequency-performance = Frequency was outside its safe band for { $out_of_band } of the month. Keep supply matched to demand every minute: set targets from the forecast and leave AGC on.
lesson-frequency-notice = Frequency was outside its safe band for { $out_of_band } of the month, and the regulator sent a formal notice. Keep supply matched to demand every minute: set targets from the forecast and leave AGC on.
lesson-solar-curtailed = { $energy } of solar output was thrown away because the grid had nowhere to put it. At midday, cheap solar can exceed demand, so back off thermal plants or connect more cities to use it.
lesson-start-up-costs = Starting plants cost { $total }, { $share } of operating cost. Plants that stop and start every day pay that bill each time, so keep slow plants running through the day.
lesson-rate-review = The annual rate review set your tariff at { $tariff } and your allowed return at { $allowed_return }. The regulator lets you earn that return only on capital it counts as useful.
lesson-all-clear = Nothing went noticeably wrong this month. Keep matching supply to demand, and plan for the next peak.

lesson-renewable-curtailed = { $energy } of renewable output was curtailed because the grid had nowhere to put it. Reduce inflexible generation, connect demand, or absorb surplus in batteries before 18:00.
