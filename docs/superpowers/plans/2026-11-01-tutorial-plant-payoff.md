# Tutorial: plant investment tradeoffs

Keep the generator build/connect walkthrough and annual outlook lesson in their existing month.
Schedule the payoff chapter for the following Planning month, only after the player has ordered
and connected the tutorial generator and actually opened the annual demand outlook. Dismissing
the crystal-ball spotlight alone does not meet that prerequisite. Retain the run across the
intervening day and Review, then ask the player to open **Compare plant payoff**. A selected land
cell still supplies local construction terms; otherwise the comparison uses regional defaults.

Point out the graph's initial capital spending and construction delay, then the table's fuel,
upkeep, net income and recovery time. Explain that lower operating costs can require more capital
or a longer build, and that full utilization is an optimistic estimate rather than a resource or
demand forecast. Closing the comparison finishes the tutorial.

Use the existing tutorial step table, timed spotlights, visible anchors and dispatch actions.
Opening the real comparison advances its instruction; closing it early skips remaining comparison
notes. Prompts and popups retain priority, and Escape dismisses an active spotlight before the
dialog. Scroll chart, table and Close into view as needed at maximum zoom. Starting a day before
the prerequisites, or skipping the final eligible Planning chapter, retains the early-exit behavior.
The calendar gate and outlook-use marker are transient tutorial state, with no save-schema change
or new simulation commands.

Validate the actual build/connect → annual outlook → next month → payoff flow, all combinations
of missing prerequisites, minimum-window layout at maximum zoom, early closure/reopening,
tutorial-off behavior, existing tutorial invariants and
localization. Run focused tests, clippy, formatting and a windowed launch smoke; do not run the full
local test suite.

Validation completed: 128 focused tutorial, payoff and localization tests passed, including the
real build/connect → annual outlook → next-month comparison workflow and all four combinations
of prerequisite completion. Clippy with warnings denied, formatting, diff checks and the 120-frame
windowed launch smoke passed.

