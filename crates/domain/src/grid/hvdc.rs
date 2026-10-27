//! Scheduled DC transfer. Direction is always the player's stored from/to order.
use super::LineStatus;
use crate::ids::{LinkId, NodeId};
use crate::units::{MegawattHours, Megawatts};

/// Stable station identity within a project; independent of city/plant node IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConverterEnd {
    From,
    To,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConverterStationId {
    pub link: LinkId,
    pub end: ConverterEnd,
}

/// A converter station at a local AC bus. Both stations are built, paid for and cancelled
/// with their HVDC project. Their bus attachments never merge the two AC grids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConverterStation {
    pub id: ConverterStationId,
    pub grid_node: NodeId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DcLink {
    pub id: LinkId,
    pub from: ConverterStation,
    pub to: ConverterStation,
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
            from: ConverterStation {
                id: ConverterStationId {
                    link: id,
                    end: ConverterEnd::From,
                },
                grid_node: from,
            },
            to: ConverterStation {
                id: ConverterStationId {
                    link: id,
                    end: ConverterEnd::To,
                },
                grid_node: to,
            },
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
    pub fn stations(&self) -> [ConverterStation; 2] {
        [self.from, self.to]
    }
    pub fn from_grid(&self) -> NodeId {
        self.from.grid_node
    }
    pub fn to_grid(&self) -> NodeId {
        self.to.grid_node
    }
    pub fn target(&self, hour: usize) -> f64 {
        self.override_mw.unwrap_or(self.schedule[hour.min(23)])
    }
}
// Retain the original portable contract: station identity is derived from (link, end),
// and the two persisted endpoint IDs now describe local AC bus attachments. Every old
// link therefore acquires explicit stations on read, without changing IDs or source bytes.
impl crate::state::State for DcLink {
    fn write(&self, out: &mut Vec<u8>) {
        self.write_schema(out, crate::state::CURRENT_STATE_SCHEMA);
    }
    fn write_schema(&self, out: &mut Vec<u8>, schema: u32) {
        self.id.write_schema(out, schema);
        self.from.grid_node.write_schema(out, schema);
        self.to.grid_node.write_schema(out, schema);
        self.status.write_schema(out, schema);
        self.length_cells.write_schema(out, schema);
        self.schedule.write_schema(out, schema);
        self.override_mw.write_schema(out, schema);
        self.flow.write_schema(out, schema);
        self.blocked.write_schema(out, schema);
        self.runback.write_schema(out, schema);
        self.sent.write_schema(out, schema);
        self.loss.write_schema(out, schema);
        self.review_year.write_schema(out, schema);
        self.runback_terminal.write_schema(out, schema);
    }
    fn read(input: &mut crate::state::StateReader<'_>) -> Result<Self, crate::state::StateError> {
        use crate::state::State;
        let id = State::read(input)?;
        let from = State::read(input)?;
        let to = State::read(input)?;
        Ok(Self {
            id,
            from: ConverterStation {
                id: ConverterStationId {
                    link: id,
                    end: ConverterEnd::From,
                },
                grid_node: from,
            },
            to: ConverterStation {
                id: ConverterStationId {
                    link: id,
                    end: ConverterEnd::To,
                },
                grid_node: to,
            },
            status: State::read(input)?,
            length_cells: State::read(input)?,
            schedule: State::read(input)?,
            override_mw: State::read(input)?,
            flow: State::read(input)?,
            blocked: State::read(input)?,
            runback: State::read(input)?,
            sent: State::read(input)?,
            loss: State::read(input)?,
            review_year: State::read(input)?,
            runback_terminal: State::read(input)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::State;

    // Frozen pre-station field contract, deliberately independent of DcLink's writer.
    #[derive(Debug, Clone)]
    struct LegacyLink {
        id: LinkId,
        from: NodeId,
        to: NodeId,
        status: LineStatus,
        length_cells: f64,
        schedule: [f64; 24],
        override_mw: Option<f64>,
        flow: Megawatts,
        blocked: bool,
        runback: bool,
        sent: MegawattHours,
        loss: Megawatts,
        review_year: i32,
        runback_terminal: Option<NodeId>,
    }
    crate::state_record!(LegacyLink {
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

    proptest::proptest! {
        #[test]
        fn legacy_station_migration_preserves_every_link_field_and_wire_byte(
            id in 0u32..10_000, from in 0u32..10_000, to in 10_000u32..20_000,
            mw in -200.0f64..200.0, months in 0u32..24, blocked in proptest::bool::ANY,
            runback in proptest::bool::ANY,
        ) {
            let old = LegacyLink { id: LinkId(id), from: NodeId(from), to: NodeId(to),
                status: if months == 0 { LineStatus::InService } else { LineStatus::UnderConstruction { months_left: months } },
                length_cells: 12.5, schedule: [mw; 24], override_mw: Some(-mw), flow: Megawatts::new(mw),
                blocked, runback, sent: MegawattHours::new(17.0), loss: Megawatts::new(3.0), review_year: 2031,
                runback_terminal: runback.then_some(NodeId(from)),
            };
            for schema in [4, 5] {
                let bytes = old.state_bytes_schema(schema);
                let new = DcLink::from_state_bytes_schema(&bytes, schema).unwrap();
                proptest::prop_assert_eq!(new.from.id, ConverterStationId { link: old.id, end: ConverterEnd::From });
                proptest::prop_assert_eq!(new.to.id, ConverterStationId { link: old.id, end: ConverterEnd::To });
                proptest::prop_assert_eq!((new.from.grid_node, new.to.grid_node), (old.from, old.to));
                proptest::prop_assert_eq!(new.state_bytes_schema(schema), bytes);
                proptest::prop_assert_eq!(new.schedule, old.schedule);
                proptest::prop_assert_eq!(new.runback_terminal, old.runback_terminal);
            }
        }
    }

    #[test]
    fn legacy_link_endpoints_become_distinct_converter_stations_without_changing_wire_bytes() {
        let link = DcLink::new(LinkId(9), NodeId(1), NodeId(2), LineStatus::InService, 10.0);
        let stations = link.stations();
        assert_ne!(stations[0].id, stations[1].id);
        assert_eq!(
            [stations[0].grid_node, stations[1].grid_node],
            [NodeId(1), NodeId(2)]
        );
        for schema in [4, 5] {
            let bytes = link.state_bytes_schema(schema);
            let restored = DcLink::from_state_bytes_schema(&bytes, schema).unwrap();
            assert_eq!(restored, link);
            assert_eq!(restored.state_bytes_schema(schema), bytes);
        }
    }
}
