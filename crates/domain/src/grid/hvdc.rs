//! Scheduled DC transfer. Direction is always the player's stored from/to order.
use super::LineStatus;
use crate::ids::{LinkId, NodeId};
use crate::units::{MegawattHours, Megawatts};

#[derive(Debug, Clone, PartialEq)]
pub struct DcLink {
    pub id: LinkId,
    pub from: NodeId,
    pub to: NodeId,
    pub status: LineStatus,
    pub length_cells: f64,
    pub schedule: [f64; 24],
    pub override_mw: Option<f64>,
    pub flow: Megawatts,
    pub blocked: bool,
    pub runback: bool,
    /// Converter terminal that triggered runback; retained even at zero flow.
    pub runback_terminal: Option<NodeId>,
    pub sent: MegawattHours,
    pub loss: Megawatts,
    pub review_year: i32,
}
impl DcLink {
    pub(super) fn new(
        id: LinkId,
        from: NodeId,
        to: NodeId,
        status: LineStatus,
        length_cells: f64,
    ) -> Self {
        Self {
            id,
            from,
            to,
            status,
            length_cells,
            schedule: [0.0; 24],
            override_mw: None,
            flow: Megawatts::ZERO,
            blocked: false,
            runback: false,
            runback_terminal: None,
            sent: MegawattHours::ZERO,
            loss: Megawatts::ZERO,
            review_year: 0,
        }
    }
    pub fn target(&self, hour: usize) -> f64 {
        self.override_mw.unwrap_or(self.schedule[hour.min(23)])
    }
}
crate::state_record!(DcLink {
    id,
    from,
    to,
    status,
    length_cells,
    schedule,
    override_mw,
    flow,
    blocked,
    runback,
    sent,
    loss,
    review_year,
    runback_terminal
});
