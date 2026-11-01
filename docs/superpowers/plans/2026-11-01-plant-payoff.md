# Plant payoff comparison

Add a read-only modal opened by **Compare plant payoff** above the right-hand build cards.
Compare the current region's catalog at the current tariff. A selected cell supplies its economic
zone's capital and build time; without one, use the regional default catalog. Include technologies
available to this session, even if the currently selected cell cannot host them.

Plot cumulative cash against years since ordering: actual monthly construction instalments first,
then constant monthly revenue less fuel and fixed O&M. Omit batteries: storage does not generate
power. Include a zero
line, commissioning markers, break-even markers, a togglable technology key, and a numerical table
with construction time, capital, monthly revenue/cost/net and total years to recover the investment.
Use the existing currency and plant colors. Limit the plotted horizon to 100 years and explicitly
label the limit; longer payoffs remain visible in the table.

This is a nameplate utilization comparison, using an average month from a 365-day year, a fixed
tariff and costs, unlimited customers and no financing, startup, network, regulatory or replacement
costs. Solar/wind/hydro ignore resource limits. Geothermal assumes successful exploration and a
full-strength field; preserve its staged spending. These assumptions appear in the dialog;
zero/negative net income is labeled as never repaying capital.

Warn on CCGT construction cards when full-utilization monthly income at the current tariff is
negative, naming the monthly loss and price. Derive this from the same read model, rather than
hard-coding Brazil and Southern China. Keep valid construction enabled and siting failures separate.

Keep calculations in an application read model, localized presentation in the game presenter,
and opening/closing state in UiAction/dispatch. Escape closes only the active modal; prompts take
precedence. Pause while viewing and block play shortcuts. Reset modal state with the session.

Validation: focused application formula/property tests; actual build-button, modal close/Escape,
currency, regional terms and no-simulation-mutation coverage through the existing egui harness;
localization coverage, clippy, formatting and a 120-frame windowed launch smoke. Do not run the
full local suite without express user permission.
