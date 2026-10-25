//! Offline authoring only: emit a v4 irradiance block from immutable legacy potential.
//! Usage: author_irradiance_grid <legacy.ron> <schema4.ron> <potential-scale>
//! Inputs are read-only; stdout is reviewed before insertion into the authored RON.
use std::{fmt::Write as _, io::Write as _};

use domain::ids::{CellCoord, RegionId};
use infrastructure::region::{Field, IrradianceV4, RegionFileV4, load_region_bytes, map_region_v4};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        return Err(
            "usage: author_irradiance_grid <legacy.ron> <schema4.ron> <potential-scale>".into(),
        );
    }
    let scale: f64 = args[2].parse()?;
    if !scale.is_finite() || scale <= 0.0 {
        return Err("potential scale must be finite and positive".into());
    }
    let mut file: RegionFileV4 = ron::from_str(&std::fs::read_to_string(&args[1])?)?;
    let legacy = load_region_bytes(std::fs::read(&args[0])?, &RegionId::new(&file.id))?;
    file.irradiance = IrradianceV4::Gaussian(Field {
        base: 1.0,
        blobs: Vec::new(),
    });
    let unit = map_region_v4(file.clone())?;
    let before = &legacy.definition;
    let (width, height) = (unit.terrain().width(), unit.terrain().height());
    if unit.geo() != before.geo()
        || (width, height) != (before.terrain().width(), before.terrain().height())
    {
        return Err("source and destination geography must match".into());
    }
    if !matches!(
        before.solar().model,
        domain::region::SolarModel::DaylightTable(_)
    ) {
        return Err("source must retain legacy daylight-table semantics".into());
    }
    let mut values = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let cell = CellCoord { x, y };
            if unit.terrain().get(cell).unwrap().land != before.terrain().get(cell).unwrap().land {
                return Err("source and destination playable cells must match".into());
            }
            // A unit-irradiance v4 region exposes the actual domain row factor.
            let factor = unit.solar_potential(cell).value();
            let value = before.solar_potential(cell).value() * scale / factor;
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(
                    format!("scale {scale} cannot fit cell {cell:?}: irradiance={value}").into(),
                );
            }
            values.push(value);
        }
    }
    file.irradiance = IrradianceV4::Cells(values.clone());
    // Round-trip the authored f64 values through the real RON loader before emitting.
    let loaded = load_region_bytes(
        ron::to_string(&file)?.into_bytes(),
        &RegionId::new(&file.id),
    )?;
    let mut ranked = Vec::new();
    let mut maximum_error = 0.0_f64;
    for y in 0..height {
        for x in 0..width {
            let cell = CellCoord { x, y };
            let old = before.solar_potential(cell).value();
            let new = loaded.definition.solar_potential(cell).value();
            maximum_error = maximum_error.max((new - old * scale).abs());
            if before.terrain().get(cell).unwrap().land {
                ranked.push((old, new));
            }
        }
    }
    if maximum_error > f64::EPSILON * scale.max(1.0) * 4.0 {
        return Err(format!("authored target error too large: {maximum_error}").into());
    }
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
    let groups: Vec<_> = ranked.chunk_by(|a, b| a.0 == b.0).collect();
    let extrema: Vec<_> = groups
        .iter()
        .map(|g| {
            g.iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), v| {
                    (min.min(v.1), max.max(v.1))
                })
        })
        .collect();
    if extrema.windows(2).any(|g| g[0].1 >= g[1].0) {
        return Err("authored grid does not preserve every strict playable-site order".into());
    }
    let tie_spread = extrema.iter().map(|g| g.1 - g.0).fold(0.0_f64, f64::max);
    eprintln!(
        "{}: {} cells, {} playable, {} rank groups, scale {scale}, maximum target error {maximum_error:e}, maximum old-tie spread {tie_spread:e}; all strict ranks preserved",
        file.id,
        values.len(),
        ranked.len(),
        groups.len()
    );
    let fingerprint: String = legacy
        .fingerprint
        .as_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let mut output = format!(
        "    // Offline author_irradiance_grid: immutable {}-v3.ron\n    // Source SHA256: {fingerprint}\n    // Annual potential target = legacy irradiance * {scale}; inverse latitude factor\n    // is baked into site quality. No runtime calibration. Row-major, north first.\n    irradiance: (cells: [\n",
        file.id
    );
    for (y, row) in values.chunks(usize::from(width)).enumerate() {
        writeln!(output, "        // Row {y}")?;
        for chunk in row.chunks(5) {
            output.push_str("        ");
            for v in chunk {
                write!(output, "{v:?}, ")?;
            }
            output.pop();
            output.push('\n');
        }
    }
    output.push_str("    ]),\n");
    std::io::stdout().write_all(output.as_bytes())?;
    Ok(())
}
