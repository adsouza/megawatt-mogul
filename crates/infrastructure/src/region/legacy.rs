//! Frozen A12 compatibility defaults for region archives predating schema 2.
//! These values never come from current shipped files; schema 2 must author its own tuning.

pub const A12_STORAGE_ACCREDITATION: [(f64, f64); 4] =
    [(2.0, 0.65), (4.0, 0.85), (6.0, 0.93), (8.0, 0.96)];
pub const A12_SATISFACTION_TARGET: f64 = 90.0;
pub const A12_RETURN_PER_SATISFACTION_POINT: f64 = 0.0005;
