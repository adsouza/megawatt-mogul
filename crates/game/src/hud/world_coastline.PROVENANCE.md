# World context land

Source: Natural Earth `ne_110m_land.geojson`, upstream commit
`ca96624a56bd078437bca8184e78163e5039ad19`.
https://github.com/nvkelso/natural-earth-vector/blob/ca96624a56bd078437bca8184e78163e5039ad19/geojson/ne_110m_land.geojson

Retrieved 2026-10-24. Source SHA-256: `9e0729ee253ca7d7a5c4ae9395fb1902264c5377c52e224d13dd85010e2835d9`.
Natural Earth data are public domain: https://www.naturalearthdata.com/about/terms-of-use/ .

The coordinate rings were simplified with Douglas–Peucker at 0.35 degrees in
longitude/latitude space and rounded to five decimal places. Rings entirely
south of 60°S were omitted. 109 rings / 1966 vertices remain.
At runtime the map clips to 60°S–75°N and projects to spherical Equal Earth.
This coarse asset supplies geographic context only; region source polygons supply
all selectable land, exclusions and hit tests. It is not the research map's
1:10m detailed boundary layer and makes no claim about electrical coverage.

Projection algorithm reference: PROJ spherical Equal Earth, upstream commit
`5fa31c29f0a806568ed7bc76c1a557604db4a804`,
https://github.com/OSGeo/PROJ/blob/5fa31c29f0a806568ed7bc76c1a557604db4a804/src/projections/eqearth.cpp .
Original projection: Šavrič, Patterson & Jenny (2018), DOI
https://doi.org/10.1080/13658816.2018.1504949 .
