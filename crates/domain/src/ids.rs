//! Identifiers and grid coordinates.

macro_rules! numeric_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub u32);

        impl From<u32> for $name {
            fn from(value: u32) -> Self {
                Self(value)
            }
        }
    };
}

numeric_id!(
    /// Identifies a power plant.
    PlantId
);
numeric_id!(
    /// Identifies a city (a demand centre).
    CityId
);
numeric_id!(
    /// Identifies a transmission node.
    NodeId
);
numeric_id!(
    /// Identifies a transmission line.
    LineId
);

numeric_id!(
    /// Identifies a direct-current interconnector.
    LinkId
);

/// Identifies a selectable region (data-driven, so a string rather than an enum).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RegionId(String);

impl RegionId {
    pub fn new(id: &str) -> Self {
        Self(id.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A cell on the map grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CellCoord {
    pub x: u16,
    pub y: u16,
}

/// Hands out numeric ids. One shared counter serves every id type, so ids are
/// unique across kinds as well as within one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdAllocator {
    next: u32,
}

impl IdAllocator {
    #[allow(clippy::should_implement_trait)]
    pub fn next<T: From<u32>>(&mut self) -> T {
        let id = T::from(self.next);
        self.next += 1;
        id
    }
}

// Portable state schema 1: explicit field order and enum tags.
crate::state_record!(IdAllocator { next });
crate::state_record!(CellCoord { x, y });

crate::state_tuple!(PlantId);
crate::state_tuple!(CityId);
crate::state_tuple!(NodeId);
crate::state_tuple!(LineId);
crate::state_tuple!(LinkId);
crate::state_tuple!(RegionId);

impl IdAllocator {
    pub(crate) fn next_value(&self) -> u32 {
        self.next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocator_is_shared_and_monotonic() {
        let mut a = IdAllocator::default();
        let p: PlantId = a.next();
        let n: NodeId = a.next();
        assert_eq!((p.0, n.0), (0, 1));
    }

    #[test]
    fn region_id_round_trips_its_string() {
        assert_eq!(RegionId::new("eastern").as_str(), "eastern");
    }

    #[test]
    fn cell_coords_order_by_x_then_y() {
        assert!(CellCoord { x: 1, y: 9 } < CellCoord { x: 2, y: 0 });
    }
}
