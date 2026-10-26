//! Player commands. Each checks the game is active, then the phase, then
//! validates completely before mutating, so `Err` leaves the session unchanged.

use super::{DayPhase, GameSession};
#[cfg(test)]
use super::{line_cost, whole_cells};
use crate::company::AssetRef;
use crate::error::DomainError;
use crate::events::DomainEvent;
use crate::generation::{CommitmentSchedule, Plant, PlantStatus};
use crate::grid::{LineStatus, Node, NodeKind, distance_cells};
use crate::ids::{CellCoord, LineId, NodeId, PlantId};
use crate::region::{PlantKind, SiteError};
use crate::time::{SimDuration, TICKS_PER_DAY};
use crate::units::{Megawatts, Money};

const PLANNING_OR_OPERATING: [DayPhase; 2] = [DayPhase::Planning, DayPhase::Operating];

/// What a line would cost and how long it would take to build (see
/// [`GameSession::line_quote`]).
pub type LineQuote = crate::region::LineConstructionTerms;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlantQuote {
    pub capital: Money,
    pub months: u32,
    pub economic_zone: crate::region::EconomicZoneId,
}

impl GameSession {
    /// Pure, phase-independent quote; an order still requires Planning and a valid site.
    pub fn plant_quote(&self, kind: PlantKind, cell: CellCoord) -> Result<PlantQuote, DomainError> {
        self.plant_site_check(kind, cell)
            .map_err(|reason| DomainError::InvalidSite { kind, cell, reason })?;
        let spec = self.region.plant_spec_at(kind, cell)?;
        self.clock.month().checked_add_months(spec.build_months)?;
        Ok(PlantQuote {
            capital: spec.capital_cost,
            months: spec.build_months,
            economic_zone: self.region.economic_zone_at(cell)?,
        })
    }
    /// Original terms of an owned plant, including a plant that is already commissioned.
    pub fn plant_construction(&self, plant: PlantId) -> Option<PlantQuote> {
        let plant = self.power.plant(plant)?;
        Some(PlantQuote {
            capital: plant.spec().capital_cost,
            months: plant.spec().build_months,
            economic_zone: self.region.economic_zone_at(plant.cell()).ok()?,
        })
    }
    /// Original terms of an existing line; no new-order eligibility or phase check.
    pub fn line_construction(&self, line: LineId) -> Option<LineQuote> {
        let line = self.power.network().line(line)?;
        self.region
            .line_terms(
                self.power.network().node(line.from)?.cell,
                self.power.network().node(line.to)?.cell,
                line.length_cells,
            )
            .ok()
    }

    /// The current siting verdict, including cities and plants still under construction.
    /// Region resource suitability alone does not establish that a site is available.
    pub fn plant_site_check(&self, kind: PlantKind, cell: CellCoord) -> Result<(), SiteError> {
        if self
            .power
            .network()
            .nodes()
            .any(|(_, node)| node.cell == cell)
        {
            return Err(SiteError::Occupied);
        }
        self.region.site_check(kind, cell)
    }

    /// Cancels an unfinished line in Planning. Paid instalments remain spent;
    /// future instalments are removed and the line can never commission.
    pub fn cancel_line_construction(
        &mut self,
        line: LineId,
    ) -> Result<Vec<DomainEvent>, DomainError> {
        self.require_phase(&[DayPhase::Planning])?;
        self.power.network_mut().cancel_construction(line)?;
        self.instalments.remove(&AssetRef::Line(line));
        Ok(vec![DomainEvent::LineConstructionCancelled { line }])
    }

    /// Orders a plant of `kind` at `cell` (Planning only). The plant sits
    /// under construction for its build months on a new node with no line,
    /// and its capital cost is queued as equal monthly instalments.
    pub fn build_plant(
        &mut self,
        kind: PlantKind,
        cell: CellCoord,
    ) -> Result<(PlantId, Vec<DomainEvent>), DomainError> {
        self.require_phase(&[DayPhase::Planning])?;
        self.plant_quote(kind, cell)?;
        let spec = self.region.plant_spec_at(kind, cell)?;
        let plant: PlantId = self.ids.next();
        let node: NodeId = self.ids.next();
        self.power
            .network_mut()
            .add_node(
                node,
                Node {
                    kind: NodeKind::Plant(plant),
                    cell,
                },
            )
            .expect("freshly allocated node id is unique");
        let instalments = spec.capital_cost.split_even(spec.build_months);
        self.instalments
            .insert(AssetRef::Plant(plant), instalments.into());
        self.power
            .insert_plant(Plant::new_under_construction(plant, node, cell, spec));
        Ok((
            plant,
            vec![DomainEvent::PlantConstructionStarted { plant, kind, cell }],
        ))
    }

    /// Orders a line between two existing nodes (Planning only), using the route's
    /// resolved construction terms and equal monthly instalments: exactly what
    /// [`GameSession::line_quote`] reports.
    pub fn build_line(
        &mut self,
        from: NodeId,
        to: NodeId,
    ) -> Result<(LineId, Vec<DomainEvent>), DomainError> {
        self.require_phase(&[DayPhase::Planning])?;
        let LineQuote {
            capital, months, ..
        } = self.line_quote(from, to)?;
        // Allocate on a copy so a rejected line leaves the counter untouched.
        let mut ids = self.ids.clone();
        let line: LineId = ids.next();
        self.power.network_mut().add_line(
            line,
            from,
            to,
            LineStatus::UnderConstruction {
                months_left: months,
            },
        )?;
        self.ids = ids;
        self.instalments
            .insert(AssetRef::Line(line), capital.split_even(months).into());
        Ok((line, vec![DomainEvent::LineConstructionStarted { line }]))
    }

    /// What a line between two nodes would cost and how long it would take,
    /// by the rule [`GameSession::build_line`] charges (the one place the
    /// length and cost are worked out), with the errors it would return for
    /// an unknown node, a line to the node itself, or two nodes already
    /// joined by any in-service or ordered path. Pure: no phase check and no state change, so the player can
    /// price a line at any time; only ordering one needs Planning.
    pub fn line_quote(&self, from: NodeId, to: NodeId) -> Result<LineQuote, DomainError> {
        let length = self.line_length(from, to)?;
        let network = self.power.network();
        if self.region.area_at(
            network
                .node(from)
                .ok_or(DomainError::UnknownNode { node: from })?
                .cell,
        ) != self.region.area_at(
            network
                .node(to)
                .ok_or(DomainError::UnknownNode { node: to })?
                .cell,
        ) {
            return Err(DomainError::LineCrossesAreas);
        }
        self.power.network().check_line(from, to)?;
        if self.power.network().joined(from, to) {
            return Err(DomainError::LineNotNeeded { from, to });
        }
        let network = self.power.network();
        let quote = self.region.line_terms(
            network
                .node(from)
                .ok_or(DomainError::UnknownNode { node: from })?
                .cell,
            network
                .node(to)
                .ok_or(DomainError::UnknownNode { node: to })?
                .cell,
            length,
        )?;
        self.clock.month().checked_add_months(quote.months)?;
        Ok(quote)
    }

    /// Distance between two distinct existing nodes, in cells.
    pub(super) fn line_length(&self, from: NodeId, to: NodeId) -> Result<f64, DomainError> {
        if from == to {
            return Err(DomainError::LineToSelf { node: from });
        }
        let network = self.power.network();
        let cell = |node| {
            network
                .node(node)
                .map(|n| n.cell)
                .ok_or(DomainError::UnknownNode { node })
        };
        Ok(distance_cells(cell(from)?, cell(to)?))
    }

    /// Sets a commissioned dispatchable plant's daily pattern (Planning only),
    /// validated against the plant's own spec on the 24-hour circle.
    pub fn commit_plant(
        &mut self,
        plant: PlantId,
        hours: [bool; 24],
    ) -> Result<Vec<DomainEvent>, DomainError> {
        self.require_phase(&[DayPhase::Planning])?;
        let schedule = CommitmentSchedule::try_new(hours, self.operable(plant)?.spec())?;
        self.operable_mut(plant).set_commitment(schedule);
        Ok(Vec::new())
    }

    /// Planning: sets the target the plant assumes when the day begins with it
    /// running. Operating: sets an online plant's dispatch target. Either way
    /// the target must be finite and lie within `[min_output, capacity]`.
    pub fn set_dispatch_target(
        &mut self,
        plant: PlantId,
        mw: Megawatts,
    ) -> Result<Vec<DomainEvent>, DomainError> {
        self.require_phase(&PLANNING_OR_OPERATING)?;
        let target = self.operable(plant)?;
        let before = target.clone();
        if self.phase == DayPhase::Planning {
            target.check_target(mw)?;
            self.operable_mut(plant).set_initial_target(mw);
        } else {
            self.operable_mut(plant).set_target(mw)?;
        }
        // A same-value command is a no-op: it must not touch the storage plan.
        if *self.operable(plant)? != before {
            self.power.generators_changed();
        }
        Ok(Vec::new())
    }

    /// Enable/disable automatic storage without changing its inventory or advancing time.
    pub fn set_battery_enabled(
        &mut self,
        id: PlantId,
        on: bool,
    ) -> Result<Vec<DomainEvent>, DomainError> {
        self.require_phase(&PLANNING_OR_OPERATING)?;
        let plant = self
            .power
            .plant(id)
            .ok_or(DomainError::UnknownPlant { plant: id })?;
        if plant.kind() != PlantKind::Battery {
            return Err(DomainError::PlantNotBattery { plant: id });
        }
        if matches!(plant.status(), PlantStatus::UnderConstruction { .. }) {
            return Err(DomainError::PlantNotCommissioned { plant: id });
        }
        if plant.battery().is_some_and(|b| b.enabled() != on) {
            self.power
                .plant_mut(id)
                .expect("checked plant")
                .battery_mut()
                .expect("battery kind")
                .set_enabled(on);
            self.power.invalidate_battery_plan();
        }
        Ok(Vec::new())
    }

    /// Turns automatic generation control on or off for a plant.
    pub fn set_auto_follow(
        &mut self,
        plant: PlantId,
        on: bool,
    ) -> Result<Vec<DomainEvent>, DomainError> {
        self.require_phase(&PLANNING_OR_OPERATING)?;
        self.operable(plant)?;
        self.power.set_auto_follow(plant, on)?;
        Ok(Vec::new())
    }

    /// Planning: selects the all-day pattern. Operating: starts an offline
    /// plant now, provided its whole start-up finishes by midnight.
    pub fn start_plant(&mut self, plant: PlantId) -> Result<Vec<DomainEvent>, DomainError> {
        self.require_phase(&PLANNING_OR_OPERATING)?;
        let target = self.operable(plant)?;
        if self.phase == DayPhase::Planning {
            self.operable_mut(plant)
                .set_commitment(CommitmentSchedule::all_day());
            return Ok(Vec::new());
        }
        let startup = target.spec().startup;
        let tick = self.clock.tick();
        if target.status() == PlantStatus::Offline
            && tick.saturating_add(startup.ticks()) > TICKS_PER_DAY
        {
            return Err(DomainError::StartUpBeyondDay {
                remaining: SimDuration::from_ticks(TICKS_PER_DAY - tick),
                startup,
            });
        }
        let events = self.operable_mut(plant).start()?;
        self.month_to_date
            .add_command_events(self.clock.tick(), &events);
        self.power.generators_changed();
        Ok(events)
    }

    /// Planning: selects the all-off pattern. Operating: stops a running plant
    /// and suppresses its automatic commitment actions for the rest of the
    /// day, leaving the month's pattern as it was.
    pub fn stop_plant(&mut self, plant: PlantId) -> Result<Vec<DomainEvent>, DomainError> {
        self.require_phase(&PLANNING_OR_OPERATING)?;
        self.operable(plant)?;
        let planning = self.phase == DayPhase::Planning;
        let target = self.operable_mut(plant);
        if planning {
            target.set_commitment(CommitmentSchedule::none());
            return Ok(Vec::new());
        }
        let events = target.stop()?;
        target.suppress_commitment_for_day();
        self.power.generators_changed();
        Ok(events)
    }

    /// The plant `id` if the player can operate it: it exists, is
    /// dispatchable and has been commissioned.
    fn operable(&self, id: PlantId) -> Result<&Plant, DomainError> {
        let plant = self
            .power
            .plant(id)
            .ok_or(DomainError::UnknownPlant { plant: id })?;
        if !plant.kind().is_dispatchable() {
            return Err(DomainError::PlantNotDispatchable { plant: id });
        }
        if matches!(plant.status(), PlantStatus::UnderConstruction { .. }) {
            return Err(DomainError::PlantNotCommissioned { plant: id });
        }
        Ok(plant)
    }

    /// The plant `id`, already checked by [`GameSession::operable`].
    fn operable_mut(&mut self, id: PlantId) -> &mut Plant {
        self.power
            .plant_mut(id)
            .expect("plant checked by `operable`")
    }
}

#[cfg(test)]
mod tests;
