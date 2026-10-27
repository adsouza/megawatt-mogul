//! Transmission topology: nodes, lines and which nodes are energised.
//!
//! A node is *connected* when a path of in-service lines links it to the
//! root (the player's home city). Connectivity is recomputed after every
//! change, with ordered collections throughout so results are deterministic.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::error::DomainError;
use crate::ids::{CellCoord, CityId, LineId, LinkId, NodeId, PlantId};

/// What a network node represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Plant(PlantId),
    City(CityId),
}

/// A point in the network, placed on a map cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Node {
    pub kind: NodeKind,
    pub cell: CellCoord,
}

/// Whether a line carries power yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineStatus {
    UnderConstruction { months_left: u32 },
    InService,
}

/// A transmission line between two nodes. Direction carries no meaning.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line {
    pub id: LineId,
    pub from: NodeId,
    pub to: NodeId,
    pub status: LineStatus,
    pub length_cells: f64,
}

/// Euclidean distance between two cells, in cells.
pub fn distance_cells(a: CellCoord, b: CellCoord) -> f64 {
    let dx = f64::from(a.x) - f64::from(b.x);
    let dy = f64::from(a.y) - f64::from(b.y);
    dx.hypot(dy)
}

/// Nodes and lines, plus the set of nodes reachable from the root over
/// in-service lines.
#[derive(Debug, Clone, PartialEq)]
pub struct Network {
    root: NodeId,
    nodes: BTreeMap<NodeId, Node>,
    lines: BTreeMap<LineId, Line>,
    connected: BTreeSet<NodeId>,
    links: BTreeMap<LinkId, super::DcLink>,
}

impl Network {
    /// A network holding only its root node, which is always connected.
    pub fn new(root: NodeId, node: Node) -> Self {
        Self {
            root,
            nodes: BTreeMap::from([(root, node)]),
            lines: BTreeMap::new(),
            links: BTreeMap::new(),
            connected: BTreeSet::from([root]),
        }
    }

    /// Adds an isolated node. Its id must be new.
    pub fn add_node(&mut self, id: NodeId, node: Node) -> Result<(), DomainError> {
        if self.nodes.contains_key(&id) {
            return Err(DomainError::DuplicateNode { node: id });
        }
        self.nodes.insert(id, node);
        Ok(())
    }

    /// Whether any existing or ordered path joins two known nodes, including islands.
    /// This need test is separate from `add_line`, so archived cycles remain valid.
    pub fn joined(&self, a: NodeId, b: NodeId) -> bool {
        if !self.nodes.contains_key(&a) || !self.nodes.contains_key(&b) {
            return false;
        }
        let mut adjacency: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
        for line in self.lines.values() {
            adjacency.entry(line.from).or_default().push(line.to);
            adjacency.entry(line.to).or_default().push(line.from);
        }
        let mut seen = BTreeSet::from([a]);
        let mut queue = VecDeque::from([a]);
        while let Some(node) = queue.pop_front() {
            if node == b {
                return true;
            }
            for &next in adjacency.get(&node).into_iter().flatten() {
                if seen.insert(next) {
                    queue.push_back(next);
                }
            }
        }
        false
    }

    /// Whether a line could be added between `from` and `to`: two distinct,
    /// existing nodes that are not directly linked (in either direction, in any
    /// status). The checks [`Network::add_line`] makes about the endpoints, so
    /// a quote and the order itself cannot disagree. Pure.
    pub fn check_line(&self, from: NodeId, to: NodeId) -> Result<(), DomainError> {
        if from == to {
            return Err(DomainError::LineToSelf { node: from });
        }
        for node in [from, to] {
            if !self.nodes.contains_key(&node) {
                return Err(DomainError::UnknownNode { node });
            }
        }
        if self
            .lines
            .values()
            .any(|l| (l.from, l.to) == (from, to) || (l.from, l.to) == (to, from))
        {
            return Err(DomainError::DuplicateLine { from, to });
        }
        Ok(())
    }

    /// Adds a line between two existing, distinct nodes that are not already
    /// joined (in either direction, in any status). Length is derived from the
    /// endpoints' cells.
    pub fn add_line(
        &mut self,
        id: LineId,
        from: NodeId,
        to: NodeId,
        status: LineStatus,
    ) -> Result<(), DomainError> {
        self.check_line(from, to)?;
        if self.lines.contains_key(&id) {
            return Err(DomainError::DuplicateLineId { line: id });
        }
        let from_cell = self.nodes[&from].cell;
        let to_cell = self.nodes[&to].cell;
        let line = Line {
            id,
            from,
            to,
            status,
            length_cells: distance_cells(from_cell, to_cell),
        };
        self.lines.insert(id, line);
        self.recompute_connectivity();
        Ok(())
    }

    /// Advances every line under construction by one month. Returns the lines
    /// that entered service, in id order.
    pub fn advance_construction(&mut self) -> Vec<LineId> {
        let mut commissioned = Vec::new();
        for line in self.lines.values_mut() {
            if let LineStatus::UnderConstruction { months_left } = line.status {
                let months_left = months_left.saturating_sub(1);
                if months_left == 0 {
                    line.status = LineStatus::InService;
                    commissioned.push(line.id);
                } else {
                    line.status = LineStatus::UnderConstruction { months_left };
                }
            }
        }
        if !commissioned.is_empty() {
            self.recompute_connectivity();
        }
        commissioned
    }

    /// Removes only an unfinished line; all validation precedes mutation.
    pub fn cancel_construction(&mut self, line: LineId) -> Result<(), DomainError> {
        let existing = self
            .lines
            .get(&line)
            .ok_or(DomainError::UnknownLine { line })?;
        if !matches!(existing.status, LineStatus::UnderConstruction { .. }) {
            return Err(DomainError::LineNotUnderConstruction { line });
        }
        self.lines.remove(&line);
        self.recompute_connectivity();
        Ok(())
    }

    pub fn root(&self) -> NodeId {
        self.root
    }

    pub fn is_connected(&self, node: NodeId) -> bool {
        self.connected.contains(&node)
    }

    /// Earliest number of month closes after which a complete path reaches `node`,
    /// assuming current construction orders finish. A path waits for its slowest line;
    /// competing paths use the earliest such date. `None` means no ordered path exists.
    pub fn months_until_connected(&self, node: NodeId) -> Option<u32> {
        let mut earliest = BTreeMap::from([(self.root, 0)]);
        let mut pending = BTreeSet::from([(0, self.root)]);
        while let Some((months, from)) = pending.pop_first() {
            if from == node {
                return Some(months);
            }
            for (a, b, status) in self
                .lines
                .values()
                .map(|l| (l.from, l.to, l.status))
                .chain(
                    self.links
                        .values()
                        .map(|l| (l.from_grid(), l.to_grid(), l.status)),
                )
                .filter(|(a, b, _)| *a == from || *b == from)
            {
                let to = if a == from { b } else { a };
                let ready = match status {
                    LineStatus::InService => 0,
                    LineStatus::UnderConstruction { months_left } => months_left.max(1),
                };
                let candidate = months.max(ready);
                if earliest.get(&to).is_none_or(|&old| candidate < old) {
                    if let Some(old) = earliest.insert(to, candidate) {
                        pending.remove(&(old, to));
                    }
                    pending.insert((candidate, to));
                }
            }
        }
        None
    }

    pub fn connected_nodes(&self) -> &BTreeSet<NodeId> {
        &self.connected
    }

    /// Construction-only projection for expansion forecasts. No live topology changes.
    pub(crate) fn projected_after(&self, months: u32) -> Self {
        let mut next = self.clone();
        let advance = |status: &mut LineStatus| {
            if let LineStatus::UnderConstruction { months_left } = status {
                *status = if *months_left <= months {
                    LineStatus::InService
                } else {
                    LineStatus::UnderConstruction {
                        months_left: *months_left - months,
                    }
                };
            }
        };
        for line in next.lines.values_mut() {
            advance(&mut line.status);
        }
        for link in next.links.values_mut() {
            advance(&mut link.status);
            link.override_mw = None;
        }
        next.recompute_connectivity();
        next
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    /// Nodes with their ids, in id order.
    pub fn nodes(&self) -> impl Iterator<Item = (NodeId, &Node)> {
        self.nodes.iter().map(|(id, node)| (*id, node))
    }

    pub fn line(&self, id: LineId) -> Option<&Line> {
        self.lines.get(&id)
    }

    /// Lines in id order.
    pub fn lines(&self) -> impl Iterator<Item = &Line> {
        self.lines.values()
    }

    pub fn links(&self) -> impl Iterator<Item = &super::DcLink> {
        self.links.values()
    }
    pub fn links_mut(&mut self) -> impl Iterator<Item = &mut super::DcLink> {
        self.links.values_mut()
    }
    pub fn link(&self, id: LinkId) -> Option<&super::DcLink> {
        self.links.get(&id)
    }
    /// Explicit project-owned stations, including stations still under construction.
    pub fn converter_stations(&self) -> impl Iterator<Item = super::ConverterStation> + '_ {
        self.links.values().flat_map(|link| link.stations())
    }
    pub fn converter_station(
        &self,
        id: super::ConverterStationId,
    ) -> Option<super::ConverterStation> {
        self.links.get(&id.link).map(|link| match id.end {
            super::ConverterEnd::From => link.from,
            super::ConverterEnd::To => link.to,
        })
    }
    pub fn link_mut(&mut self, id: LinkId) -> Option<&mut super::DcLink> {
        self.links.get_mut(&id)
    }
    pub fn add_link(
        &mut self,
        id: LinkId,
        from: NodeId,
        to: NodeId,
        status: LineStatus,
    ) -> Result<(), DomainError> {
        for node in [from, to] {
            if self.node(node).is_none() {
                return Err(DomainError::UnknownNode { node });
            }
        }
        if from == to {
            return Err(DomainError::LineToSelf { node: from });
        }
        if self.links.contains_key(&id) {
            return Err(DomainError::ConstructionOverflow);
        }
        let length_cells = distance_cells(self.nodes[&from].cell, self.nodes[&to].cell);
        self.links
            .insert(id, super::DcLink::new(id, from, to, status, length_cells));
        self.recompute_connectivity();
        Ok(())
    }
    pub fn cancel_link(&mut self, link: LinkId) -> Result<(), DomainError> {
        let current = self.link(link).ok_or(DomainError::UnknownLink { link })?;
        if !matches!(current.status, LineStatus::UnderConstruction { .. }) {
            return Err(DomainError::LinkNotUnderConstruction { link });
        }
        self.links.remove(&link);
        self.recompute_connectivity();
        Ok(())
    }
    pub fn advance_links(&mut self) -> Vec<LinkId> {
        let mut commissioned = Vec::new();
        for link in self.links.values_mut() {
            if let LineStatus::UnderConstruction { months_left } = &mut link.status {
                *months_left = months_left.saturating_sub(1);
                if *months_left == 0 {
                    link.status = LineStatus::InService;
                    commissioned.push(link.id);
                }
            }
        }
        self.recompute_connectivity();
        commissioned
    }
    /// Energised AC components; DC joins service territory but never synchronises it.
    pub fn islands(&self) -> BTreeMap<NodeId, BTreeSet<NodeId>> {
        let mut unseen = self.connected.clone();
        let mut result = BTreeMap::new();
        while let Some(root) = unseen.pop_first() {
            let mut nodes = BTreeSet::from([root]);
            let mut queue = VecDeque::from([root]);
            while let Some(node) = queue.pop_front() {
                for line in self
                    .lines
                    .values()
                    .filter(|l| l.status == LineStatus::InService)
                {
                    let next = if line.from == node {
                        line.to
                    } else if line.to == node {
                        line.from
                    } else {
                        continue;
                    };
                    if unseen.remove(&next) {
                        nodes.insert(next);
                        queue.push_back(next);
                    }
                }
            }
            result.insert(root, nodes);
        }
        result
    }
    pub(crate) fn scope_connected(&mut self, nodes: BTreeSet<NodeId>) -> BTreeSet<NodeId> {
        std::mem::replace(&mut self.connected, nodes)
    }

    fn recompute_connectivity(&mut self) {
        let mut adjacency: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
        for line in self
            .lines
            .values()
            .filter(|l| l.status == LineStatus::InService)
        {
            adjacency.entry(line.from).or_default().push(line.to);
            adjacency.entry(line.to).or_default().push(line.from);
        }
        for link in self
            .links
            .values()
            .filter(|l| l.status == LineStatus::InService)
        {
            adjacency
                .entry(link.from_grid())
                .or_default()
                .push(link.to_grid());
            adjacency
                .entry(link.to_grid())
                .or_default()
                .push(link.from_grid());
        }
        let mut reached = BTreeSet::from([self.root]);
        let mut queue = VecDeque::from([self.root]);
        while let Some(node) = queue.pop_front() {
            for &next in adjacency.get(&node).into_iter().flatten() {
                if reached.insert(next) {
                    queue.push_back(next);
                }
            }
        }
        self.connected = reached;
    }
}

// Portable state schema 1: explicit field order and enum tags.
crate::state_record!(Node { kind, cell });
impl crate::state::State for NodeKind {
    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::Plant(v0) => {
                crate::state::State::write(&0u8, out);
                crate::state::State::write(v0, out);
            }
            Self::City(v0) => {
                crate::state::State::write(&1u8, out);
                crate::state::State::write(v0, out);
            }
        }
    }
    fn read(input: &mut crate::state::StateReader<'_>) -> Result<Self, crate::state::StateError> {
        Ok(match <u8 as crate::state::State>::read(input)? {
            0 => Self::Plant(crate::state::State::read(input)?),
            1 => Self::City(crate::state::State::read(input)?),
            _ => return Err(crate::state::StateError::Invalid),
        })
    }
}
crate::state_record!(Line {
    id,
    from,
    to,
    status,
    length_cells
});
impl crate::state::State for LineStatus {
    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Self::UnderConstruction { months_left } => {
                crate::state::State::write(&0u8, out);
                crate::state::State::write(months_left, out);
            }
            Self::InService => {
                crate::state::State::write(&1u8, out);
            }
        }
    }
    fn read(input: &mut crate::state::StateReader<'_>) -> Result<Self, crate::state::StateError> {
        Ok(match <u8 as crate::state::State>::read(input)? {
            0 => Self::UnderConstruction {
                months_left: crate::state::State::read(input)?,
            },
            1 => Self::InService,
            _ => return Err(crate::state::StateError::Invalid),
        })
    }
}
crate::state_record!(Network {
    root,
    nodes,
    lines,
    connected; since4 { links: Default::default() }
});

impl Network {
    pub(crate) fn valid_snapshot(&self, anchor: NodeId) -> bool {
        if self.root != anchor || !self.nodes.contains_key(&anchor) {
            return false;
        }
        for (id, line) in &self.lines {
            if *id != line.id
                || line.from == line.to
                || !self.nodes.contains_key(&line.from)
                || !self.nodes.contains_key(&line.to)
                || line.length_cells < 0.0
            {
                return false;
            }
        }
        for (id, link) in &self.links {
            if *id != link.id
                || link.from.id
                    != (super::ConverterStationId {
                        link: *id,
                        end: super::ConverterEnd::From,
                    })
                || link.to.id
                    != (super::ConverterStationId {
                        link: *id,
                        end: super::ConverterEnd::To,
                    })
                || link.from_grid() == link.to_grid()
                || !self.nodes.contains_key(&link.from_grid())
                || !self.nodes.contains_key(&link.to_grid())
                || link.length_cells < 0.0
                || !link.length_cells.is_finite()
                || (link.length_cells
                    - distance_cells(
                        self.nodes[&link.from_grid()].cell,
                        self.nodes[&link.to_grid()].cell,
                    ))
                .abs()
                    > 1e-9
                || matches!(
                    link.status,
                    LineStatus::UnderConstruction { months_left: 0 }
                )
            {
                return false;
            }
        }
        let mut network = self.clone();
        network.recompute_connectivity();
        network.connected == self.connected
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn cell(x: u16, y: u16) -> CellCoord {
        CellCoord { x, y }
    }

    fn city_node(x: u16, y: u16) -> Node {
        Node {
            kind: NodeKind::City(CityId(u32::from(x))),
            cell: cell(x, y),
        }
    }

    fn net() -> Network {
        Network::new(NodeId(0), city_node(0, 0))
    }

    #[test]
    fn root_is_connected_alone() {
        let n = net();
        assert_eq!(
            n.connected_nodes().iter().copied().collect::<Vec<_>>(),
            vec![NodeId(0)]
        );
        assert!(n.is_connected(NodeId(0)));
        assert_eq!(n.node(NodeId(0)), Some(&city_node(0, 0)));
    }

    #[test]
    fn joined_includes_ordered_islands_and_only_known_nodes_without_mutating() {
        let mut network = net();
        for id in 1..=3 {
            network
                .add_node(NodeId(id), city_node(id as u16, 0))
                .unwrap();
        }
        network
            .add_line(
                LineId(1),
                NodeId(1),
                NodeId(2),
                LineStatus::UnderConstruction { months_left: 2 },
            )
            .unwrap();
        network
            .add_line(LineId(2), NodeId(2), NodeId(3), LineStatus::InService)
            .unwrap();
        let before = network.clone();
        assert!(network.joined(NodeId(1), NodeId(3)));
        assert!(network.joined(NodeId(3), NodeId(1)));
        assert!(network.joined(NodeId(1), NodeId(1)));
        assert!(!network.is_connected(NodeId(3)));
        assert!(!network.joined(NodeId(0), NodeId(3)));
        assert!(!network.joined(NodeId(99), NodeId(99)));
        assert!(!network.joined(NodeId(1), NodeId(99)));
        assert_eq!(network, before);
        network.cancel_construction(LineId(1)).unwrap();
        assert!(!network.joined(NodeId(1), NodeId(3)));
        assert!(network.joined(NodeId(2), NodeId(3)));
    }

    #[test]
    fn connection_wait_uses_the_earliest_complete_route_and_tracks_cancellation() {
        let mut network = net();
        for node in 1..=5 {
            network
                .add_node(NodeId(node), city_node(node as u16, 0))
                .unwrap();
        }
        for (id, from, to, status) in [
            (1, 0, 1, LineStatus::UnderConstruction { months_left: 5 }),
            (2, 1, 2, LineStatus::UnderConstruction { months_left: 1 }),
            (3, 2, 3, LineStatus::InService),
            (4, 0, 4, LineStatus::UnderConstruction { months_left: 3 }),
            (5, 4, 2, LineStatus::UnderConstruction { months_left: 2 }),
            (6, 3, 1, LineStatus::UnderConstruction { months_left: 4 }),
        ] {
            network
                .add_line(LineId(id), NodeId(from), NodeId(to), status)
                .unwrap();
        }
        assert_eq!(network.months_until_connected(NodeId(0)), Some(0));
        assert_eq!(network.months_until_connected(NodeId(3)), Some(3));
        assert_eq!(network.months_until_connected(NodeId(5)), None);
        assert_eq!(network.months_until_connected(NodeId(99)), None);
        network.cancel_construction(LineId(4)).unwrap();
        assert_eq!(network.months_until_connected(NodeId(3)), Some(5));
        network.advance_construction();
        network.advance_construction();
        assert_eq!(network.months_until_connected(NodeId(3)), Some(3));
    }

    proptest! {
        #[test]
        fn connection_wait_predicts_actual_reachability_after_month_closes(
            edges in prop::collection::vec((0u8..7, any::<bool>()), 15)
        ) {
            let mut network = net();
            for node in 1..6 { network.add_node(NodeId(node), city_node(node as u16, 0)).unwrap(); }
            let mut index = 0;
            for from in 0..6 { for to in from+1..6 {
                let (code, reverse) = edges[index];
                index += 1;
                if code == 0 { continue; }
                let status = if code == 1 { LineStatus::InService } else { LineStatus::UnderConstruction { months_left: u32::from(code - 2) } };
                let (from, to) = if reverse { (to, from) } else { (from, to) };
                network.add_line(LineId(index as u32), NodeId(from), NodeId(to), status).unwrap();
            } }
            let before = network.clone();
            for target in 0..6 {
                let predicted = network.months_until_connected(NodeId(target));
                let mut actual = network.clone();
                let expected = (0..=5).find(|&closes| {
                    if closes > 0 { actual.advance_construction(); }
                    actual.is_connected(NodeId(target))
                });
                prop_assert_eq!(predicted, expected);
            }
            prop_assert_eq!(network, before);
        }
    }

    #[test]
    fn line_under_construction_does_not_connect() {
        let mut n = net();
        n.add_node(NodeId(1), city_node(3, 4)).unwrap();
        n.add_line(
            LineId(10),
            NodeId(0),
            NodeId(1),
            LineStatus::UnderConstruction { months_left: 2 },
        )
        .unwrap();
        assert!(!n.is_connected(NodeId(1)));
        assert!(n.advance_construction().is_empty());
        assert!(!n.is_connected(NodeId(1)));
        assert_eq!(n.advance_construction(), vec![LineId(10)]);
        assert!(n.is_connected(NodeId(1)));
        assert!(n.advance_construction().is_empty());
    }

    #[test]
    fn connectivity_is_transitive() {
        let mut n = net();
        n.add_node(NodeId(1), city_node(1, 0)).unwrap();
        n.add_node(NodeId(2), city_node(2, 0)).unwrap();
        n.add_line(LineId(10), NodeId(1), NodeId(2), LineStatus::InService)
            .unwrap();
        assert!(!n.is_connected(NodeId(2)));
        n.add_line(LineId(11), NodeId(0), NodeId(1), LineStatus::InService)
            .unwrap();
        assert!(n.is_connected(NodeId(2)));
    }

    #[test]
    fn duplicate_reverse_line_rejected() {
        let mut n = net();
        n.add_node(NodeId(1), city_node(1, 0)).unwrap();
        n.add_line(LineId(10), NodeId(0), NodeId(1), LineStatus::InService)
            .unwrap();
        let err = n
            .add_line(
                LineId(11),
                NodeId(1),
                NodeId(0),
                LineStatus::UnderConstruction { months_left: 1 },
            )
            .unwrap_err();
        assert_eq!(
            err,
            DomainError::DuplicateLine {
                from: NodeId(1),
                to: NodeId(0)
            }
        );
        assert_eq!(n.lines().count(), 1);
    }

    #[test]
    fn self_and_unknown_lines_rejected() {
        let mut n = net();
        assert_eq!(
            n.add_line(LineId(10), NodeId(0), NodeId(0), LineStatus::InService),
            Err(DomainError::LineToSelf { node: NodeId(0) })
        );
        assert_eq!(
            n.add_line(LineId(10), NodeId(0), NodeId(7), LineStatus::InService),
            Err(DomainError::UnknownNode { node: NodeId(7) })
        );
        assert_eq!(
            n.add_line(LineId(10), NodeId(8), NodeId(0), LineStatus::InService),
            Err(DomainError::UnknownNode { node: NodeId(8) })
        );
        assert_eq!(n.lines().count(), 0);
    }

    #[test]
    fn duplicate_ids_rejected_without_mutation() {
        let mut n = net();
        assert_eq!(
            n.add_node(NodeId(0), city_node(9, 9)),
            Err(DomainError::DuplicateNode { node: NodeId(0) })
        );
        assert_eq!(n.node(NodeId(0)), Some(&city_node(0, 0)));
        n.add_node(NodeId(1), city_node(1, 0)).unwrap();
        n.add_node(NodeId(2), city_node(2, 0)).unwrap();
        n.add_line(LineId(10), NodeId(0), NodeId(1), LineStatus::InService)
            .unwrap();
        assert_eq!(
            n.add_line(LineId(10), NodeId(0), NodeId(2), LineStatus::InService),
            Err(DomainError::DuplicateLineId { line: LineId(10) })
        );
        assert_eq!(n.lines().count(), 1);
        assert!(!n.is_connected(NodeId(2)));
    }

    #[test]
    fn line_length_is_euclidean_distance_between_endpoints() {
        let mut n = net();
        n.add_node(NodeId(1), city_node(3, 4)).unwrap();
        n.add_line(LineId(10), NodeId(0), NodeId(1), LineStatus::InService)
            .unwrap();
        let line = n.lines().next().unwrap();
        assert_eq!(line.length_cells, 5.0);
        assert_eq!(distance_cells(cell(0, 0), cell(3, 4)), 5.0);
    }

    #[test]
    fn lines_iterate_in_id_order() {
        let mut n = net();
        for i in 1..4 {
            n.add_node(NodeId(i), city_node(i as u16, 0)).unwrap();
        }
        n.add_line(LineId(30), NodeId(0), NodeId(1), LineStatus::InService)
            .unwrap();
        n.add_line(LineId(20), NodeId(1), NodeId(2), LineStatus::InService)
            .unwrap();
        n.add_line(LineId(25), NodeId(2), NodeId(3), LineStatus::InService)
            .unwrap();
        let ids: Vec<_> = n.lines().map(|l| l.id).collect();
        assert_eq!(ids, vec![LineId(20), LineId(25), LineId(30)]);
    }

    fn find(parent: &mut [usize], x: usize) -> usize {
        if parent[x] != x {
            let root = find(parent, parent[x]);
            parent[x] = root;
        }
        parent[x]
    }

    proptest! {
        #[test]
        fn connectivity_matches_union_find(
            edges in prop::collection::vec((0u32..12, 0u32..12), 0..30)
        ) {
            let mut n = net();
            for i in 1..12u32 {
                n.add_node(NodeId(i), city_node(i as u16, 0)).unwrap();
            }
            let mut parent: Vec<usize> = (0..12).collect();
            let mut seen = std::collections::BTreeSet::new();
            for (i, &(a, b)) in edges.iter().enumerate() {
                if a == b || !seen.insert((a.min(b), a.max(b))) {
                    continue;
                }
                n.add_line(LineId(i as u32), NodeId(a), NodeId(b), LineStatus::InService)
                    .unwrap();
                let (ra, rb) = (find(&mut parent, a as usize), find(&mut parent, b as usize));
                parent[ra] = rb;
            }
            let root = find(&mut parent, 0);
            let expected: std::collections::BTreeSet<NodeId> = (0..12u32)
                .filter(|&i| find(&mut parent, i as usize) == root)
                .map(NodeId)
                .collect();
            prop_assert_eq!(n.connected_nodes(), &expected);
        }
    }
}

#[cfg(test)]
mod island_topology_tests {
    use super::*;
    use crate::ids::LinkId;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn islands_match_independent_components_of_the_energised_graph(
            edges in prop::collection::vec((0usize..8, 0usize..8, any::<bool>(), any::<bool>()), 0..40)
        ) {
            let node = |i| Node {
                kind: NodeKind::City(CityId(i as u32)),
                cell: CellCoord { x: i as u16, y: 0 },
            };
            let mut network = Network::new(NodeId(0), node(0));
            for i in 1..8 {
                network.add_node(NodeId(i), node(i as usize)).unwrap();
            }
            // Transitive closure is independent of the production breadth-first search.
            let mut ac = [[false; 8]; 8];
            let mut energised = ac;
            for i in 0..8 {
                ac[i][i] = true;
                energised[i][i] = true;
            }
            for (id, (a, b, dc, live)) in edges.into_iter().enumerate() {
                let status = if live { LineStatus::InService } else {
                    LineStatus::UnderConstruction { months_left: 1 }
                };
                let accepted = if dc {
                    network.add_link(LinkId(id as u32), NodeId(a as u32), NodeId(b as u32), status)
                } else {
                    network.add_line(LineId(id as u32), NodeId(a as u32), NodeId(b as u32), status)
                }.is_ok();
                if accepted && live {
                    energised[a][b] = true;
                    energised[b][a] = true;
                    if !dc {
                        ac[a][b] = true;
                        ac[b][a] = true;
                    }
                }
            }
            for k in 0..8 {
                for a in 0..8 {
                    for b in 0..8 {
                        ac[a][b] |= ac[a][k] && ac[k][b];
                        energised[a][b] |= energised[a][k] && energised[k][b];
                    }
                }
            }
            let mut expected = BTreeMap::<NodeId, BTreeSet<NodeId>>::new();
            for i in 0..8 {
                if energised[0][i] {
                    let root = (0..8).find(|&j| ac[i][j]).unwrap();
                    expected.entry(NodeId(root as u32)).or_default().insert(NodeId(i as u32));
                }
            }
            let stations: Vec<_> = network.converter_stations().collect();
            let ids: BTreeSet<_> = stations.iter().map(|s| s.id).collect();
            prop_assert_eq!(stations.len(), network.links().count() * 2);
            prop_assert_eq!(ids.len(), stations.len());
            prop_assert!(stations.iter().all(|s| network.node(s.grid_node).is_some()));
            prop_assert_eq!(network.islands(), expected);
        }
    }

    #[test]
    fn dc_energises_without_synchronising_or_satisfying_the_ac_need_test() {
        let mut network = Network::new(
            NodeId(0),
            Node {
                kind: NodeKind::City(CityId(0)),
                cell: CellCoord { x: 0, y: 0 },
            },
        );
        network
            .add_node(
                NodeId(1),
                Node {
                    kind: NodeKind::City(CityId(1)),
                    cell: CellCoord { x: 4, y: 0 },
                },
            )
            .unwrap();
        network
            .add_link(LinkId(2), NodeId(0), NodeId(1), LineStatus::InService)
            .unwrap();
        assert!(network.is_connected(NodeId(1)));
        assert!(!network.joined(NodeId(0), NodeId(1)));
        assert_eq!(
            network.islands(),
            BTreeMap::from([
                (NodeId(0), BTreeSet::from([NodeId(0)])),
                (NodeId(1), BTreeSet::from([NodeId(1)])),
            ])
        );
    }
}
