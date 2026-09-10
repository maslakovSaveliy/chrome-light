//! [`Au`]: the app unit, `1/60` of a CSS pixel — the integer unit every layout computation
//! downstream of `cl-layout` works in.
//!
//! Layout arithmetic is integer (`docs/superpowers/plans/2026-09-07-m1a-static-pipeline.md`,
//! "Global Constraints"): `f32` only ever appears at the boundary with stylo's percentages
//! and, later, `parley`'s shaping output, and is rounded to `Au` immediately. `1/60` matches
//! stylo's own `app_units::Au` (`border-*-width`'s computed type, read in
//! [`crate::style_adapt`]) exactly, so converting a stylo width into ours is a bit-for-bit
//! copy of the inner integer, not a lossy rescale.
//!
//! A document is attacker-controlled input, so every operation here is total: there is no
//! CSS pixel value, however large, non-finite or adversarially chosen, that makes any
//! function in this module panic. Overflow saturates instead of wrapping or aborting.

/// One-sixtieth of a CSS pixel, stored as a signed 32-bit integer.
///
/// `Copy`/`Ord`, so two `Au` values compare and move exactly like the `i32` they wrap.
/// Construct one from a CSS pixel value with [`Au::from_px`]; read it back with
/// [`Au::to_px`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Au(pub i32);

/// App units per CSS pixel. `1/60` is stylo's own `app_units::AU_PER_PX`
/// (`app_units-0.7.8/src/app_unit.rs`): six times the least common multiple of the pixel
/// subdivisions historically used by layout engines (1, 2, 3, 4, 5, 6, 10, 12, 15, 20, 30,
/// 60), so common fractional CSS pixel values (a third of a pixel, a twelfth, …) round-trip
/// exactly instead of drifting under repeated arithmetic.
const AU_PER_PX: f64 = 60.0;

impl Au {
    /// Zero app units.
    pub const ZERO: Au = Au(0);

    /// The largest representable value, `i32::MAX` app units (a little over 35.7 million CSS
    /// pixels). Used as the conventional "unconstrained" sentinel for an available size that
    /// has no upper bound.
    pub const MAX: Au = Au(i32::MAX);

    /// Converts a CSS pixel value to app units, rounding half away from zero.
    ///
    /// `px` is attacker-controlled (it ultimately comes from parsed CSS): `NaN`, infinities
    /// and magnitudes far beyond any real layout are all valid input and never panic.
    /// Rust's float-to-integer `as` cast is itself saturating and NaN-safe (`NaN` becomes
    /// `0`, out-of-range magnitudes clamp to `i32::MIN`/`i32::MAX`) as of Rust 1.45, so no
    /// manual range check is needed beyond that cast.
    #[must_use]
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the f64 -> i32 cast below is Rust's saturating float-to-int conversion: \
                   out-of-range and non-finite inputs clamp to i32::MIN/MAX/0 rather than \
                   wrapping or panicking, which is exactly the saturation this function \
                   promises"
    )]
    pub fn from_px(px: f32) -> Au {
        // `f64` for the multiply: a `f32` product of two `f32`s can round differently than
        // the exact `f64` product, and app units are precise enough (1/60px) that the extra
        // precision keeps `from_px`/`to_px` a closer round trip for ordinary page-sized
        // values.
        let scaled = f64::from(px) * AU_PER_PX;
        Au(scaled.round() as i32)
    }

    /// Converts back to a CSS pixel value.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        reason = "i32 -> f32 loses precision only past 2^24 app units (~279917 CSS px), far \
                   beyond any real layout; there is no panic-free alternative representation"
    )]
    pub fn to_px(self) -> f32 {
        // A plain `f32` literal rather than casting the `f64` `AU_PER_PX` down: 60.0 is
        // exactly representable in both, so this is not a precision loss, just a second
        // constant.
        self.0 as f32 / 60.0
    }

    /// Adds two app-unit values, saturating at [`Au::MAX`]/`i32::MIN` instead of overflowing.
    #[must_use]
    pub fn saturating_add(self, other: Au) -> Au {
        Au(self.0.saturating_add(other.0))
    }

    /// Subtracts `other` from `self`, saturating at [`Au::MAX`]/`i32::MIN` instead of
    /// overflowing.
    #[must_use]
    pub fn saturating_sub(self, other: Au) -> Au {
        Au(self.0.saturating_sub(other.0))
    }

    /// Scales by a floating-point factor (used for e.g. `line-height: <number>`, which
    /// multiplies a font size), rounding half away from zero and saturating on overflow or
    /// a non-finite `scale`.
    #[must_use]
    pub fn mul_by_f32(self, scale: f32) -> Au {
        Au::from_px(self.to_px() * scale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn au_from_px_should_round_half_away_from_zero() {
        assert_eq!(Au::from_px(0.5), Au(30));
        assert_eq!(Au::from_px(-0.5), Au(-30));
    }

    #[test]
    fn au_from_px_should_round_fractional_au_half_away_from_zero() {
        // 1/120 px * 60 = 0.5 Au exactly: the case `0.5px` itself cannot exercise, since
        // 0.5 * 60 is already an integer.
        assert_eq!(Au::from_px(1.0 / 120.0), Au(1));
        assert_eq!(Au::from_px(-1.0 / 120.0), Au(-1));
    }

    #[test]
    fn au_from_px_should_not_panic_on_non_finite_input() {
        assert_eq!(Au::from_px(f32::NAN), Au::ZERO);
        assert_eq!(Au::from_px(f32::INFINITY), Au::MAX);
        assert_eq!(Au::from_px(f32::NEG_INFINITY), Au(i32::MIN));
    }

    #[test]
    fn au_to_px_should_invert_from_px_for_whole_pixels() {
        assert_eq!(Au::from_px(16.0).to_px(), 16.0);
        assert_eq!(Au::from_px(-3.0).to_px(), -3.0);
    }

    #[test]
    fn au_saturating_add_should_not_overflow() {
        assert_eq!(Au::MAX.saturating_add(Au(1)), Au::MAX);
        assert_eq!(Au(i32::MIN).saturating_sub(Au(1)), Au(i32::MIN));
    }

    proptest! {
        /// `Au` arithmetic must never panic, however extreme the inputs: a document's CSS
        /// (and therefore every `Au` this module ever constructs) is attacker-controlled.
        #[test]
        fn au_should_saturate_on_overflow(a: i32, b: i32, px in proptest::num::f32::ANY) {
            let x = Au(a);
            let y = Au(b);
            let _ = x.saturating_add(y);
            let _ = x.saturating_sub(y);
            let _ = Au::from_px(px);
            let _ = x.mul_by_f32(px);
            let _ = x.to_px();
        }
    }
}
