# Solar calibration receipts and authoring (2026-10-26)

All region irradiance values map to annual cloud-adjusted clear-sky fractions. The five Plan 7
regions retain their documented FAO-56 sunshine fits, divided by the common 0.75 clear-sky reference.
North America and SGCC no longer reinterpret arbitrary legacy site indices as sunshine duration.

Their new data comes from NASA POWER SYN1DEG climatology, parameters `ALLSKY_SFC_SW_DWN` and
`CLRSKY_SFC_SW_DWN`, both in kWh/m²/day. The archived JSON receipts include requested coordinates,
all twelve monthly values, annual values, source URLs and the API's reported January 2001–December
2025 climatology period. The annual ratio is all-sky energy divided by clear-sky energy. NASA's
API documentation: https://power.larc.nasa.gov/docs/services/api/temporal/climatology/ .

Run `python3 -I tools/region_authoring/solar_clearness.py` from the repository root to reproduce the
three new sidecars without network access. Inverse-distance-squared interpolation in latitude-adjusted
degrees is a game authoring judgment. Each point snaps to its containing cell so independent source
energy receipts remain exact there. These sparse anchors do not constitute a measured solar atlas.
The frozen pre-calibration NA sidecars move byte-for-byte to infrastructure's global-foundations
fixtures; no legacy golden save, embedded region bundle or historical value is retuned.

Weather variation uses a bounded, annual-mean-preserving linear response (§30), including the actual
mean of the bounded daily distribution. This avoids the earlier multiplicative response's clipping
bias. Forecasts and dispatch share the response and differ only in using expected versus observed
cloudiness. Statistical and property regressions cover nonzero variance, bounded output and annual
mean preservation. Save regressions construct and connect a solar farm in every shipped region,
capture positive solar output and compare continuation across the next weather draw.
