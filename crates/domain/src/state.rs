//! Portable state schema primitives. Pure mementos: no serde, files, wall clock or Debug output.
//! Field order and enum tags in the explicit implementations are a persisted contract. Changing
//! that contract requires a schema migration, independently of simulation rules and Rust layout.
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ops::RangeInclusive;

/// Schema emitted by new portable state captures.
pub const CURRENT_STATE_SCHEMA: u32 = 6;
/// Existing portable collection bound, also used for construction payment queues.
pub const MAX_COLLECTION_LEN: usize = 1_000_000;
/// Portable state schemas accepted by every reader.
pub const SUPPORTED_STATE_SCHEMAS: RangeInclusive<u32> = 1..=6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StateError {
    #[error("truncated state")]
    Truncated,
    #[error("invalid state value")]
    Invalid,
    #[error("state limit exceeded")]
    Limit,
    #[error("unsupported state schema")]
    Version,
}
pub struct StateReader<'a> {
    bytes: &'a [u8],
    at: usize,
    pub schema: u32,
    collection_depth: u8,
}
impl<'a> StateReader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            at: 0,
            schema: CURRENT_STATE_SCHEMA,
            collection_depth: 0,
        }
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8], StateError> {
        let end = self.at.checked_add(n).ok_or(StateError::Limit)?;
        let out = self.bytes.get(self.at..end).ok_or(StateError::Truncated)?;
        self.at = end;
        Ok(out)
    }
    pub fn finish(self) -> Result<(), StateError> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err(StateError::Invalid)
        }
    }
    fn length(&mut self) -> Result<usize, StateError> {
        let n = u32::read(self)? as usize;
        if n > MAX_COLLECTION_LEN || n > self.bytes.len().saturating_sub(self.at) {
            return Err(StateError::Limit);
        }
        Ok(n)
    }
}
pub trait State: Sized {
    fn write(&self, out: &mut Vec<u8>);
    fn read(input: &mut StateReader<'_>) -> Result<Self, StateError>;
    fn write_schema(&self, out: &mut Vec<u8>, _schema: u32) {
        self.write(out);
    }
    fn state_bytes_schema(&self, schema: u32) -> Vec<u8> {
        let mut out = Vec::new();
        self.write_schema(&mut out, schema);
        out
    }
    fn from_state_bytes_schema(bytes: &[u8], schema: u32) -> Result<Self, StateError> {
        if !SUPPORTED_STATE_SCHEMAS.contains(&schema) {
            return Err(StateError::Version);
        }
        let mut input = StateReader::new(bytes);
        input.schema = schema;
        let state = Self::read(&mut input)?;
        input.finish()?;
        Ok(state)
    }
    fn state_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.write(&mut out);
        out
    }
    fn from_state_bytes(bytes: &[u8]) -> Result<Self, StateError> {
        let mut input = StateReader::new(bytes);
        let state = Self::read(&mut input)?;
        input.finish()?;
        Ok(state)
    }
}
macro_rules! numbers { ($($ty:ty),*) => { $(impl State for $ty {
    fn write(&self, out: &mut Vec<u8>) { out.extend(self.to_le_bytes()); }
    fn read(input: &mut StateReader<'_>) -> Result<Self, StateError> {
        let bytes = input.take(std::mem::size_of::<Self>())?;
        Ok(Self::from_le_bytes(bytes.try_into().map_err(|_| StateError::Truncated)?))
    }
})* }; }
numbers!(u8, u16, u32, u64, u128, i32, i64);
impl State for f64 {
    fn write(&self, out: &mut Vec<u8>) {
        self.to_bits().write(out);
    }
    fn read(input: &mut StateReader<'_>) -> Result<Self, StateError> {
        let n = Self::from_bits(u64::read(input)?);
        if n.is_finite() {
            Ok(n)
        } else {
            Err(StateError::Invalid)
        }
    }
}
impl State for bool {
    fn write(&self, out: &mut Vec<u8>) {
        u8::from(*self).write(out);
    }
    fn read(input: &mut StateReader<'_>) -> Result<Self, StateError> {
        match u8::read(input)? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(StateError::Invalid),
        }
    }
}
impl<T: State> State for Option<T> {
    fn write(&self, out: &mut Vec<u8>) {
        self.is_some().write(out);
        if let Some(v) = self {
            v.write(out);
        }
    }
    fn write_schema(&self, out: &mut Vec<u8>, schema: u32) {
        self.is_some().write_schema(out, schema);
        if let Some(v) = self {
            v.write_schema(out, schema);
        }
    }
    fn read(input: &mut StateReader<'_>) -> Result<Self, StateError> {
        if bool::read(input)? {
            Ok(Some(T::read(input)?))
        } else {
            Ok(None)
        }
    }
}
impl<T: State> State for Box<T> {
    fn write(&self, out: &mut Vec<u8>) {
        self.as_ref().write(out);
    }
    fn write_schema(&self, out: &mut Vec<u8>, schema: u32) {
        self.as_ref().write_schema(out, schema);
    }
    fn read(input: &mut StateReader<'_>) -> Result<Self, StateError> {
        Ok(Box::new(T::read(input)?))
    }
}
impl<T: State> State for Vec<T> {
    fn write(&self, out: &mut Vec<u8>) {
        (self.len() as u32).write(out);
        for v in self {
            v.write(out);
        }
    }
    fn write_schema(&self, out: &mut Vec<u8>, schema: u32) {
        (self.len() as u32).write_schema(out, schema);
        for v in self {
            v.write_schema(out, schema);
        }
    }
    fn read(input: &mut StateReader<'_>) -> Result<Self, StateError> {
        let n = input.length()?;
        if input.collection_depth >= 32 {
            return Err(StateError::Limit);
        }
        input.collection_depth += 1;
        let result = (0..n).map(|_| T::read(input)).collect();
        input.collection_depth -= 1;
        result
    }
}
impl<T: State> State for VecDeque<T> {
    fn write(&self, out: &mut Vec<u8>) {
        (self.len() as u32).write(out);
        for v in self {
            v.write(out);
        }
    }
    fn write_schema(&self, out: &mut Vec<u8>, schema: u32) {
        (self.len() as u32).write_schema(out, schema);
        for v in self {
            v.write_schema(out, schema);
        }
    }
    fn read(input: &mut StateReader<'_>) -> Result<Self, StateError> {
        Ok(Vec::read(input)?.into())
    }
}
impl<K: State + Ord, V: State> State for BTreeMap<K, V> {
    fn write(&self, out: &mut Vec<u8>) {
        (self.len() as u32).write(out);
        for (k, v) in self {
            k.write(out);
            v.write(out);
        }
    }
    fn write_schema(&self, out: &mut Vec<u8>, schema: u32) {
        (self.len() as u32).write_schema(out, schema);
        for (k, v) in self {
            k.write_schema(out, schema);
            v.write_schema(out, schema);
        }
    }
    fn read(input: &mut StateReader<'_>) -> Result<Self, StateError> {
        let n = input.length()?;
        let mut map = Self::new();
        for _ in 0..n {
            let k = K::read(input)?;
            if map.last_key_value().is_some_and(|(last, _)| last >= &k) {
                return Err(StateError::Invalid);
            }
            map.insert(k, V::read(input)?);
        }
        Ok(map)
    }
}
impl<T: State + Ord> State for BTreeSet<T> {
    fn write(&self, out: &mut Vec<u8>) {
        (self.len() as u32).write(out);
        for v in self {
            v.write(out);
        }
    }
    fn write_schema(&self, out: &mut Vec<u8>, schema: u32) {
        (self.len() as u32).write_schema(out, schema);
        for v in self {
            v.write_schema(out, schema);
        }
    }
    fn read(input: &mut StateReader<'_>) -> Result<Self, StateError> {
        let n = input.length()?;
        let mut set = Self::new();
        for _ in 0..n {
            let v = T::read(input)?;
            if set.last().is_some_and(|last| last >= &v) {
                return Err(StateError::Invalid);
            }
            set.insert(v);
        }
        Ok(set)
    }
}
impl<T: State, const N: usize> State for [T; N] {
    fn write(&self, out: &mut Vec<u8>) {
        for v in self {
            v.write(out);
        }
    }
    fn write_schema(&self, out: &mut Vec<u8>, schema: u32) {
        for v in self {
            v.write_schema(out, schema);
        }
    }
    fn read(input: &mut StateReader<'_>) -> Result<Self, StateError> {
        (0..N)
            .map(|_| T::read(input))
            .collect::<Result<Vec<_>, _>>()?
            .try_into()
            .map_err(|_| StateError::Invalid)
    }
}
impl<A: State, B: State> State for (A, B) {
    fn write(&self, out: &mut Vec<u8>) {
        self.0.write(out);
        self.1.write(out);
    }
    fn write_schema(&self, out: &mut Vec<u8>, schema: u32) {
        self.0.write_schema(out, schema);
        self.1.write_schema(out, schema);
    }
    fn read(input: &mut StateReader<'_>) -> Result<Self, StateError> {
        Ok((A::read(input)?, B::read(input)?))
    }
}
impl State for String {
    fn write(&self, out: &mut Vec<u8>) {
        (self.len() as u32).write(out);
        out.extend(self.as_bytes());
    }
    fn read(input: &mut StateReader<'_>) -> Result<Self, StateError> {
        let n = input.length()?;
        String::from_utf8(input.take(n)?.to_vec()).map_err(|_| StateError::Invalid)
    }
}
/// Explicit schema-1 field order; adding a Rust field must require an explicit default/migration.
#[macro_export]
macro_rules! state_record {
    ($ty:ty { $($field:ident),* ; since6 { $($added:ident : $default:expr),* $(,)? } }) => {
        impl $crate::state::State for $ty {
            fn write(&self, out: &mut Vec<u8>) { self.write_schema(out, $crate::state::CURRENT_STATE_SCHEMA); }
            fn write_schema(&self, out: &mut Vec<u8>, schema: u32) {
                $( $crate::state::State::write_schema(&self.$field, out, schema); )*
                if schema >= 6 { $( $crate::state::State::write_schema(&self.$added, out, schema); )* }
            }
            fn read(input: &mut $crate::state::StateReader<'_>) -> Result<Self, $crate::state::StateError> {
                Ok(Self { $( $field: $crate::state::State::read(input)?, )*
                    $( $added: if input.schema >= 6 { $crate::state::State::read(input)? } else { $default }, )* })
            }
        }
    };
    ($ty:ty { $($field:ident),* ; since4 { $($added:ident : $default:expr),* $(,)? } }) => {
        impl $crate::state::State for $ty {
            fn write(&self, out: &mut Vec<u8>) { self.write_schema(out, $crate::state::CURRENT_STATE_SCHEMA); }
            fn write_schema(&self, out: &mut Vec<u8>, schema: u32) {
                $( $crate::state::State::write_schema(&self.$field, out, schema); )*
                if schema >= 4 { $( $crate::state::State::write_schema(&self.$added, out, schema); )* }
            }
            fn read(input: &mut $crate::state::StateReader<'_>) -> Result<Self, $crate::state::StateError> {
                Ok(Self { $( $field: $crate::state::State::read(input)?, )*
                    $( $added: if input.schema >= 4 { $crate::state::State::read(input)? } else { $default }, )* })
            }
        }
    };
    ($ty:ty { $($field:ident),* ; $($added:ident : $default:expr),* $(,)? }) => {
        impl $crate::state::State for $ty {
            fn write(&self, out: &mut Vec<u8>) { self.write_schema(out, $crate::state::CURRENT_STATE_SCHEMA); }
            fn write_schema(&self, out: &mut Vec<u8>, schema: u32) {
                $( $crate::state::State::write_schema(&self.$field, out, schema); )*
                if schema >= 2 { $( $crate::state::State::write_schema(&self.$added, out, schema); )* }
            }
            fn read(input: &mut $crate::state::StateReader<'_>) -> Result<Self, $crate::state::StateError> {
                Ok(Self { $( $field: $crate::state::State::read(input)?, )*
                    $( $added: if input.schema >= 2 { $crate::state::State::read(input)? } else { $default }, )* })
            }
        }
    };
    ($ty:ty { $($field:ident),* $(,)? }) => {
        impl $crate::state::State for $ty {
            fn write(&self, out: &mut Vec<u8>) { self.write_schema(out, $crate::state::CURRENT_STATE_SCHEMA); }
            fn write_schema(&self, out: &mut Vec<u8>, schema: u32) { $( $crate::state::State::write_schema(&self.$field, out, schema); )* }
            fn read(input: &mut $crate::state::StateReader<'_>) -> Result<Self, $crate::state::StateError> {
                Ok(Self { $( $field: $crate::state::State::read(input)?, )* })
            }
        }
    };
}
#[macro_export]
macro_rules! state_tuple {
    ($ty:ty) => {
        impl $crate::state::State for $ty {
            fn write(&self, out: &mut Vec<u8>) {
                $crate::state::State::write(&self.0, out);
            }
            fn read(
                input: &mut $crate::state::StateReader<'_>,
            ) -> Result<Self, $crate::state::StateError> {
                Ok(Self($crate::state::State::read(input)?))
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn every_gate_accepts_exactly_the_supported_schemas() {
        assert_eq!(CURRENT_STATE_SCHEMA, 6);
        for schema in [1, 2, 3, 4, 5, 6] {
            assert!(SUPPORTED_STATE_SCHEMAS.contains(&schema));
            assert_eq!(u8::from_state_bytes_schema(&[7], schema), Ok(7));
        }
        for schema in [0, 7, u32::MAX] {
            assert!(!SUPPORTED_STATE_SCHEMAS.contains(&schema));
            assert_eq!(
                u8::from_state_bytes_schema(&[7], schema),
                Err(StateError::Version)
            );
        }
    }

    proptest! {
        #[test]
        fn state_schema_gate_accepts_only_one_through_four(schema in any::<u32>()) {
            let expected = match schema {
                1..=4 => Ok(7),
                _ => Err(StateError::Version),
            };
            prop_assert_eq!(u8::from_state_bytes_schema(&[7], schema), expected);
        }
    }
}
