//! Read-only views of the session for projections and display.

use super::{DayPhase, GameSession};
use crate::demand::{ForecastPoint, hourly_forecast};
use crate::generation::{Plant, PlantStatus};
use crate::grid::{GridMode, TickSummary};
use crate::ids::{CellCoord, CityId, NodeId, PlantId};
use crate::region::PlantKind;
use crate::regulation::Licence;
use crate::time::SimMonth;
use crate::units::{Hertz, MegawattHours, Megawatts, Money, Price, Ratio};

/// One plant as the player sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct PlantView {
    pub id: PlantId,
    pub kind: PlantKind,
    pub cell: CellCoord,
    pub node: NodeId,
    pub status: PlantStatus,
    pub output: Megawatts,
    pub auto_follow: bool,
    /// Whether the plant's node is energised from the substation.
    pub connected: bool,
    pub commitment: [bool; 24],
    pub hydro_budget: Option<MegawattHours>,
}

/// One city as the player sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct CityView {
    pub id: CityId,
    pub name_key: String,
    pub cell: CellCoord,
    pub node: NodeId,
    /// Whether the city is energised and therefore a customer.
    pub connected: bool,
}

/// The grid's state at the current instant.
#[derive(Debug, Clone, PartialEq)]
pub struct GridSnapshot {
    pub month: SimMonth,
    pub tick: u32,
    pub phase: DayPhase,
    /// Absolute frequency: nominal plus deviation.
    pub frequency: Hertz,
    pub mode: GridMode,
    pub shed: Ratio,
    /// The most recent tick of the current day, if any has run.
    pub last_tick: Option<TickSummary>,
}

/// The company's standing with its customers and regulator.
#[derive(Debug, Clone, PartialEq)]
pub struct CompanyView {
    pub cash: Money,
    pub reliability: Ratio,
    pub satisfaction: f64,
    pub tariff: Price,
    pub allowed_return: Ratio,
    pub strikes: u8,
    pub licence: Licence,
}

impl GameSession {
    /// Every plant, in id order.
    pub fn plant_views(&self) -> Vec<PlantView> {
        self.power
            .plants()
            .values()
            .map(|plant| self.plant_view(plant))
            .collect()
    }

    fn plant_view(&self, plant: &Plant) -> PlantView {
        PlantView {
            id: plant.id(),
            kind: plant.kind(),
            cell: plant.cell(),
            node: plant.node(),
            status: plant.status(),
            output: plant.output(),
            auto_follow: plant.auto_follow(),
            connected: self.power.network().is_connected(plant.node()),
            commitment: plant.commitment().hours(),
            hydro_budget: plant.hydro_budget(),
        }
    }

    /// Every city, in id order.
    pub fn city_views(&self) -> Vec<CityView> {
        self.power
            .cities()
            .values()
            .map(|city| CityView {
                id: city.id(),
                name_key: city.def().name_key.clone(),
                cell: city.def().cell,
                node: city.node(),
                connected: self.power.network().is_connected(city.node()),
            })
            .collect()
    }

    /// The noise-free demand forecast of connected cities for each hour of
    /// the current month's representative day.
    pub fn hourly_forecast(&self) -> [ForecastPoint; 24] {
        let network = self.power.network();
        let connected = self
            .power
            .cities()
            .values()
            .filter(|city| network.is_connected(city.node()));
        hourly_forecast(connected, self.region.demand(), self.clock.month())
    }

    pub fn grid_snapshot(&self) -> GridSnapshot {
        let protection = self.power.protection();
        GridSnapshot {
            month: self.clock.month(),
            tick: self.clock.tick(),
            phase: self.phase,
            frequency: self.region.frequency().nominal + self.power.deviation(),
            mode: protection.mode,
            shed: protection.shed,
            last_tick: self.day_ticks.last().cloned(),
        }
    }

    pub fn company_view(&self) -> CompanyView {
        CompanyView {
            cash: self.company.cash(),
            reliability: self.company.reliability(),
            satisfaction: self.company.satisfaction(),
            tariff: self.regulator.tariff(),
            allowed_return: self.regulator.allowed_return(),
            strikes: self.regulator.strikes(),
            licence: self.regulator.licence(),
        }
    }
}
