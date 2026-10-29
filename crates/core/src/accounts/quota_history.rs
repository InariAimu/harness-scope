//! Derived measurement boundaries; original quota observations stay unchanged.
use rust_decimal::Decimal;

// Keep in sync with QUOTA_ADJUSTMENT_PERCENT_POINTS in src/lib/providerHistory.ts.
pub(super) const ADJUSTMENT_PERCENT_POINTS: u32 = 20;

pub(super) fn is_adjustment(previous_used: Decimal, used: Decimal) -> bool {
    previous_used - used >= Decimal::from(ADJUSTMENT_PERCENT_POINTS)
}

// Codex can report the same deadline one second apart on successive refreshes.
pub(super) fn same_codex_reset(a: i64, b: i64) -> bool {
    a.abs_diff(b) <= 1
}
