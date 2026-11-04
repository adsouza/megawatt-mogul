# Cash outlook: construction commitments and operating-income scenarios

Add a read-only Cash outlook dialog beside Compare plant payoff in the construction panel.
Place its new button beside Compare plant payoff. Choosing a scenario never places an order.
A chooser compares existing commitments with one prospective plant at the selected cell,
using its actual local quote.

Start at today's cash and plot monthly closing cash over at least twelve months, extending
through scheduled or proposed construction, up to ten years. Review starts with next month's
close, since the current close is already accounted for. Read remaining payment queues directly:
include plants, AC lines, HVDC projects and makeup drilling, without recharging paid instalments.
Use the quote's staged geothermal exploration payments for a proposed geothermal plant.
Scheduled geothermal payments assume exploration succeeds; dry exploration can cancel later costs.

The user chose recent operating results for the income cone. Use up to twelve completed monthly
reports. Remove construction cash flows and reprice energy sold at today's tariff before computing
minimum, arithmetic mean and maximum operating net cash. Repeat these three monthly run rates to form a widening
scenario envelope. These are scenarios, not confidence intervals or guaranteed bounds. Do not
invent a percentage spread. One report produces no spread; no reports shows construction-only
cash and explicitly says an income estimate is not yet available.

Show committed-project cash with mean operating income, the proposed-plant cash with the same
income assumption, the selected scenario's income range, and its construction-only cash path.
Mark zero cash prominently, state the lowest projected mean cash and month, and warn if any
lower scenario falls below zero. Include a scrollable monthly table of payments and cash.
Amounts and axes use the region's currency. Explain that recent results hold operations constant:
growth, seasonal changes, new-plant output/upkeep, reliability and connection
costs for an unconnected proposed plant are not predicted. Display the history sample size.

At each future January, change the income slopes using the actual rate-review formula with
minimum/base/maximum allowed return from regional data. Infer costs and energy sold from the
same history sample. Depreciate a copy of today's eligible capital through future closes;
derive a conditional tariff band, then reprice every historical sample at each scenario tariff.
Keep cash continuous across year-end; only subsequent income changes. Mark each reset and
display its tariff band. In December Review the January tariff has already been awarded;
use that known tariff until the next annual review rather than reopening its uncertainty.
These are conditional scenarios: future commissions, reserve holds
and write-offs can change the actual tariff and are not projected in this first version.

Follow the crystal ball's bordered, scrollable dialog style. Opening pauses playback; Close and
Escape dismiss it, with popups taking Escape first. Prompts and save screens take precedence.
Keep it usable at the minimum window size and maximum UI zoom. Route clicks through dispatch;
panel systems read UI resources through ActionSink. Queries leave session, RNG, command log,
save formats and schemas unchanged. Focused tests cover accounting, historical construction
exclusion, payment remainder/staging, Review boundaries, no-history behavior, ordering of the
income bounds, read-only preview, chooser, Close/Escape and small-window usability.
