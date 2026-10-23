//! Portable state schema primitives. Pure mementos: no serde, files, wall clock or Debug output.
//! Field order and enum tags in the explicit implementations are a persisted contract. Changing
//! that contract requires a schema migration, independently of simulation rules and Rust layout.
use std::collections::{BTreeMap, BTreeSet, VecDeque};

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
}
impl<'a> StateReader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
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
        if n > 1_000_000 || n > self.bytes.len().saturating_sub(self.at) {
            return Err(StateError::Limit);
        }
        Ok(n)
    }
}
pub trait State: Sized {
    fn write(&self, out: &mut Vec<u8>);
    fn read(input: &mut StateReader<'_>) -> Result<Self, StateError>;
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
    fn read(input: &mut StateReader<'_>) -> Result<Self, StateError> {
        let n = input.length()?;
        (0..n).map(|_| T::read(input)).collect()
    }
}
impl<T: State> State for VecDeque<T> {
    fn write(&self, out: &mut Vec<u8>) {
        (self.len() as u32).write(out);
        for v in self {
            v.write(out);
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
    ($ty:ty { $($field:ident),* $(,)? }) => {
        impl $crate::state::State for $ty {
            fn write(&self, out: &mut Vec<u8>) { $( $crate::state::State::write(&self.$field, out); )* }
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
