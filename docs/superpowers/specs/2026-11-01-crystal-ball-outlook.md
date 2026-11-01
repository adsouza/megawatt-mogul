# Crystal ball: twelve-month capacity outlook

Add a crystal-ball icon to the top bar, with an accessible name and tooltip. It opens a
read-only planning dialog and pauses Operating. Close and Escape dismiss it; open popups take
Escape first. The dialog includes a city chooser initialized from the selected map city when
that city has no existing or ordered grid path. Only those cities appear as preview options;
already connected or scheduled cities are part of the baseline and are excluded from the chooser.

Show twelve representative months starting with the current month (next month in Review).
Plot and tabulate the peak of the summed seasonal demand profiles, existing connected capacity,
capacity arriving from scheduled builds, and accredited dependable capacity. Include all ordered
network construction: a plant contributes only once both it and its grid path are ready; city
loads arrive only once their path is ready. Keep demand on minute resolution to retain local-time
and sub-hour peaks. Existing commitments and dispatch targets do not limit installed capacity.

For the selected-city what-if, assume a direct connection from the closest eligible node of the
currently active grid, by map distance and then node ID. Use actual local line construction terms
(or HVDC terms across electrical areas). Show the source and estimated completion month. Demand
changes only after construction completes. Existing ordered paths are already in the baseline;
never double-count them. A route can also reach a preconnected component. Reuse the existing
prospective-service assessment for a conservative view of demand a completed connection can enroll.

The dialog compares baseline and proposed demand. Supply values follow the selected scenario,
including any existing plants reached by that connection. Explain that dependable capacity uses
regional capacity credits, while installed MW are ratings: neither guarantees hourly balance,
resources, reliability, or transfers between independent grids. Future construction assumes it
completes as scheduled. Forecast queries never change simulation, RNG, commands, saves or schemas.

Use a pure domain query and a game presenter/draw/dispatch split. Cache the query until the
session or chosen city changes. Keep the dialog usable at minimum window size and maximum UI zoom
with scrolling. Test completion boundaries, closest-node routing, seasonal peaks, no double count,
read-only determinism, icon/chooser/Close/Escape interaction and small-window layout. Use focused
checks; no full local suite.
