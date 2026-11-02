//! Looking ahead at demand growth: the factor a future month will play at (spec §3.4).
//!
//! Growth steps once at every month close where every city is connected
//! ([`GameSession::close_month`] step 9), so a projection for a later month counts the closes
//! still to come before it at which the region is, or by construction already ordered will be,
//! complete.

use std::collections::BTreeSet;

use super::{DayPhase, GameSession, SessionStatus};
use crate::demand::DemandGrowth;
use crate::grid::{LineStatus, Network};
use crate::time::SimMonth;

impl GameSession {
    /// The demand growth `month` will be played at: today's factor, stepped once for each close
    /// still to come before `month` at which the region will be complete with what is already
    /// ordered. A month already playing (or before it) plays at today's factor.
    pub fn growth_for(&self, month: SimMonth) -> DemandGrowth {
        self.growth_on(self.power.network(), month)
    }

    /// The first close still to come at which demand grows: the next close if every city is
    /// connected now, otherwise the close at which lines and links already under construction
    /// connect the last of them. `None` at a zero growth rate, once the game has ended, or when
    /// what is ordered never connects every city.
    pub fn first_growth_close(&self) -> Option<SimMonth> {
        self.first_growth_close_on(self.power.network())
    }

    /// Whether this close, and so every close after it, grows demand: the game goes on, the
    /// region grows, and every city is connected (completion is permanent).
    pub(super) fn growing(&self) -> bool {
        self.status == SessionStatus::Active
            && self.region.demand_growth_per_year() != 0.0
            && self.region_complete()
    }

    /// [`GameSession::growth_for`] on `network` (today's, or a proposal that adds to it) and
    /// the construction it has ordered.
    pub(super) fn growth_on(&self, network: &Network, month: SimMonth) -> DemandGrowth {
        let current = self.power.demand_growth();
        match self.first_growth_close_on(network) {
            Some(first) if month > first => current.ahead(
                self.region.monthly_demand_growth(),
                month.months_since(first).unsigned_abs(),
            ),
            _ => current,
        }
    }

    fn first_growth_close_on(&self, network: &Network) -> Option<SimMonth> {
        if self.status != SessionStatus::Active || self.region.demand_growth_per_year() == 0.0 {
            return None;
        }
        // Like the outlooks' projected systems, a connected city counts as served.
        let complete = |network: &Network| {
            self.power
                .cities()
                .values()
                .all(|city| network.is_connected(city.node()))
        };
        // The live network is judged exactly as the close's growth step judges it
        // ([`GameSession::region_complete`]: a city counts once its area is enrolled, not merely
        // reached by a link). A proposal has no activation state of its own, so it keeps the
        // physical-connectivity rule.
        let complete_now = if network == self.power.network() {
            self.region_complete()
        } else {
            complete(network)
        };
        // Connectivity only changes when something under construction commissions.
        let closes = if complete_now {
            0
        } else {
            construction_months(network)
                .into_iter()
                .find(|months| complete(&network.projected_after(*months)))?
        };
        // The region is complete after `closes` more closes, and the last of them steps; a region
        // complete now steps at the next close.
        self.first_pending_close()
            .checked_add_months(closes.saturating_sub(1))
            .ok()
    }

    /// The month whose close is next to run. In Review the clock still shows the month just
    /// closed, whose growth step is already in today's factor.
    fn first_pending_close(&self) -> SimMonth {
        let month = self.clock.month();
        if self.phase == DayPhase::Review {
            month.next()
        } else {
            month
        }
    }
}

/// The distinct numbers of closes after which something under construction commissions.
fn construction_months(network: &Network) -> BTreeSet<u32> {
    network
        .lines()
        .map(|line| line.status)
        .chain(network.links().map(|link| link.status))
        .filter_map(|status| match status {
            LineStatus::UnderConstruction { months_left } => Some(months_left),
            LineStatus::InService => None,
        })
        .collect()
}
