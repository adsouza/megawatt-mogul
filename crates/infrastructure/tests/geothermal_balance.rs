//! Independent lifetime-cost oracle for A19. Costs are cents; energy is MWh.
//! A good site is the highest potential land cell within eight grid cells of a city
//! in the same country mask/electrical area. Italy mask: 7..19E, 36..47N;
//! western Turkey: 26..31E, 36..42N. These masks isolate the authored fields, not borders.
//! Compare each alternative at the same line distance; line quotes use actual regional
//! route costs. Siting-minimum quality is synthetic, so no exact raster value is required.
use application::{RegionCatalog, RegionLoadError};
use domain::{
    grid::distance_cells,
    ids::{CellCoord, RegionId},
    region::{PlantKind, RegionDefinition, SitingRule},
    units::Ratio,
};
use infrastructure::region::RonRegionCatalog;
fn loaded(id: &str) -> Result<std::sync::Arc<RegionDefinition>, RegionLoadError> {
    RonRegionCatalog::new(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/regions"),
    )
    .load(&RegionId::new(id))
    .map(|r| r.definition)
}
/// Oracle-only finance arithmetic; inputs and result are cents, annual_return is a fraction.
fn construction_carry(
    capital: f64,
    months: u32,
    exploration: u32,
    share: f64,
    annual_return: f64,
) -> f64 {
    (0..months)
        .map(|m| {
            let payment = if m < exploration {
                capital * share / f64::from(exploration)
            } else {
                capital * (1.0 - share) / f64::from(months - exploration)
            };
            payment * (f64::from(months - m) - 0.5) * annual_return / 12.0
        })
        .sum()
}
#[test]
fn geothermal_economic_oracle_hand_calculated_staged_finance_and_units() {
    // $120 paid $30/$90 over two months at 12%: $30×1.5×1% + $90×0.5×1% = $0.90.
    assert!((construction_carry(12000.0, 2, 1, 0.25, 0.12) - 90.0).abs() < 1e-10);
    // Equal $60 instalments cost $1.20, so staging reduces tied-up construction capital.
    assert!((construction_carry(12000.0, 2, 0, 0.0, 0.12) - 120.0).abs() < 1e-10);
    // 100 MW × 50% field × 90% duty × 24 h = 1,080 MWh; at $5/MWh, cost is 540,000 cents.
    let mwh = 100.0 * 0.5 * 0.9 * 24.0;
    assert_eq!(mwh, 1080.0);
    assert_eq!(mwh * 5.0 * 100.0, 540000.0);
}
fn geo_cost(
    r: &RegionDefinition,
    at: CellCoord,
    city: CellCoord,
    potential: Ratio,
) -> (f64, f64, usize) {
    let spec = r.plant_spec_at(PlantKind::Geothermal, at).unwrap();
    let p = r.geothermal().unwrap();
    let SitingRule::GeothermalSite { min } = spec.siting else {
        panic!("geothermal siting")
    };
    let (odds, initial) = p.site(potential, min);
    let line = r.line_terms(at, city, distance_cells(at, city)).unwrap();
    let line_cost = line.capital.cents() as f64;
    let capital = spec.capital_cost.cents() as f64;
    let risk = capital * p.exploration_share.value() * (1.0 - odds.value()) / odds.value();
    let mut cost = risk;
    let mut energy = 0.0;
    let mut components = vec![(capital, spec.book_life_months)];
    let mut line_book = line_cost;
    let mut field = initial.value();
    let mut rounds = 0u32;
    let mut crossed = 0u16;
    let mut drilling: Option<(u32, f64)> = None;
    // Finance each staged instalment from its mid-month payment until commissioning.
    cost += construction_carry(
        capital,
        spec.build_months,
        p.exploration_months,
        p.exploration_share.value(),
        r.regulator().base_return.value(),
    );
    for month in 0..spec.book_life_months {
        let hours = f64::from(
            domain::time::SimMonth::new(2027 + (month / 12) as i32, (month % 12 + 1) as u8)
                .unwrap()
                .days_in_month(),
        ) * 24.0;
        let mwh = spec.capacity.value() * field * 0.9 * hours;
        energy += mwh;
        cost += mwh * spec.marginal_cost.value() * 100.0 + spec.fixed_om_per_month.cents() as f64;
        for (book, left) in &mut components {
            if *left > 0 {
                let depreciation = *book / f64::from(*left);
                cost += depreciation
                    + (*book - depreciation / 2.0) * r.regulator().base_return.value() / 12.0;
                *book -= depreciation;
                *left -= 1;
            }
        }
        let line_life = r.lines().book_life_months;
        if month < line_life {
            let d = line_cost / f64::from(line_life);
            cost += d + (line_book - d / 2.0) * r.regulator().base_return.value() / 12.0;
            line_book -= d;
        }
        field *= 1.0 - p.monthly_decline.value();
        if let Some((left, ceiling)) = drilling.as_mut() {
            *left -= 1;
            if *left == 0 {
                field = field.max(*ceiling);
                rounds += 1;
                // Effective local plant terms determine the same local makeup multiplier.
                let capital = p.makeup_capital.cents() as f64 * spec.capital_cost.cents() as f64
                    / r.spec(PlantKind::Geothermal).capital_cost.cents() as f64;
                let life = spec.book_life_months - month;
                let d = capital / f64::from(life);
                cost += d + (capital - d / 2.0) * r.regulator().base_return.value() / 12.0;
                components.push((capital - d, life - 1));
                drilling = None;
            }
        }
        // Independent threshold arithmetic: do not call the simulation's decline/notice code.
        let mut notice = false;
        for n in 0..9u8 {
            let threshold = f64::from(90 - 10 * n) / 100.0;
            if field / initial.value() > threshold {
                crossed &= !(1 << n);
            }
            if field / initial.value() < threshold && crossed & (1 << n) == 0 {
                crossed |= 1 << n;
                notice = true;
            }
        }
        let ceiling = initial.value() * (1.0 - (f64::from(rounds) + 1.0) * p.ceiling_step.value());
        if notice
            && drilling.is_none()
            && ceiling - field >= p.minimum_gain.value()
            && month + p.makeup_months < spec.book_life_months
        {
            drilling = Some((p.makeup_months, ceiling));
        }
    }
    // Recover any remaining connection capital at horizon; compare all kinds identically.
    cost += line_book.max(0.0);
    (cost / 100.0 / energy, energy, rounds as usize)
}
fn other_cost(r: &RegionDefinition, kind: PlantKind, at: CellCoord, city: CellCoord) -> f64 {
    let s = r.plant_spec_at(kind, at).unwrap();
    let line = r.line_terms(at, city, distance_cells(at, city)).unwrap();
    let years = f64::from(s.book_life_months) / 12.0;
    let energy = s.capacity.value() * 0.9 * 365.2425 * 24.0 * years;
    let capital = s.capital_cost.cents() as f64;
    let line = line.capital.cents() as f64;
    let cost = capital
        + line
        + (capital + line) * r.regulator().base_return.value() * years / 2.0
        + construction_carry(
            capital,
            s.build_months,
            0,
            0.0,
            r.regulator().base_return.value(),
        )
        + s.fixed_om_per_month.cents() as f64 * f64::from(s.book_life_months)
        + energy * s.marginal_cost.value() * 100.0;
    cost / 100.0 / energy
}
fn compare(id: &str, mask: impl Fn(f64, f64) -> bool) {
    let r = loaded(id).unwrap();
    let mut sites = Vec::new();
    for y in 0..r.terrain().height() {
        for x in 0..r.terrain().width() {
            let at = CellCoord { x, y };
            let (lat, lon) = r.cell_lat_lon(at);
            if !mask(lon, lat) || r.site_check(PlantKind::Geothermal, at).is_err() {
                continue;
            }
            for city in r.cities() {
                let (lat, lon) = r.cell_lat_lon(city.cell);
                if mask(lon, lat)
                    && distance_cells(at, city.cell) > 0.0
                    && distance_cells(at, city.cell) <= 8.0
                    && r.area_at(at) == r.area_at(city.cell)
                {
                    sites.push((at, city.cell));
                }
            }
        }
    }
    sites.sort_by(|(a, ca), (b, cb)| {
        r.terrain()
            .get(*b)
            .unwrap()
            .geothermal_potential
            .value()
            .total_cmp(&r.terrain().get(*a).unwrap().geothermal_potential.value())
            .then(a.cmp(b))
            .then(ca.cmp(cb))
    });
    let (at, city) = sites
        .first()
        .copied()
        .expect("country needs a good site near a city");
    let potential = r.terrain().get(at).unwrap().geothermal_potential;
    let good = geo_cost(&r, at, city, potential);
    let SitingRule::GeothermalSite { min } = r.spec(PlantKind::Geothermal).siting else {
        panic!()
    };
    let marginal = geo_cost(&r, at, city, min);
    let ccgt = other_cost(&r, PlantKind::Ccgt, at, city);
    eprintln!(
        "{id} site={at:?} city={city:?} potential={:.4} good={:.4} marginal={:.4} gas={ccgt:.4} lifetime_MWh={:.0} rounds={}",
        potential.value(),
        good.0,
        marginal.0,
        good.1,
        good.2
    );
    for kind in [PlantKind::Ccgt, PlantKind::Coal, PlantKind::Nuclear] {
        let comparator = other_cost(&r, kind, at, city);
        eprintln!(
            "  {kind:?}={comparator:.4} good_margin={:.4}",
            comparator - good.0
        );
        assert!(
            good.0 < comparator,
            "{id} good geothermal {:.4} must beat {kind:?} {comparator:.4}",
            good.0
        );
    }
    assert!(
        marginal.0 > ccgt,
        "{id} marginal geothermal must lose to CCGT"
    );
}
#[test]
fn geothermal_balance_western() {
    compare("western_interconnection", |_, _| true);
}
#[test]
fn geothermal_balance_italy() {
    compare("continental_europe_maghreb", |lon, lat| {
        (7.0..19.0).contains(&lon) && (36.0..47.0).contains(&lat)
    });
}
#[test]
fn geothermal_balance_turkey() {
    compare("continental_europe_maghreb", |lon, lat| {
        (26.0..31.0).contains(&lon) && (36.0..42.0).contains(&lat)
    });
}
