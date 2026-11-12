//! Multiplying a value by a factor: the bullterrier-audio feature of the template.

use crate::Error;

/// `value` times `factor`.
///
/// Promises:
/// - The result is the correctly rounded product, and never a negative zero: `scale(0.0, -2.0)` is `0.0`.
/// - Both arguments must be finite, and so must the result: otherwise an [`Error`] names the argument, or the overflow.
///   Nothing panics.
///
/// # Errors
///
/// [`Error::NotFinite`] when `value` or `factor` is NaN or infinite, [`Error::Overflow`] when the product is.
pub fn scale(value: f64, factor: f64) -> Result<f64, Error> {
    for (name, number) in [("value", value), ("factor", factor)] {
        if !number.is_finite() {
            return Err(Error::NotFinite { name });
        }
    }
    let scaled = product(value, factor);
    if scaled.is_finite() {
        Ok(scaled)
    } else {
        Err(Error::Overflow)
    }
}

/// The product without a negative zero. A NaN or an infinity passes through as the IEEE product gives it (`scale`
/// refuses them before it gets here), so the kernel never panics on one.
fn product(value: f64, factor: f64) -> f64 {
    let scaled = value * factor;
    if scaled == 0.0 { 0.0 } else { scaled }
}

// Inline, because no public route reaches this: `scale` refuses a NaN or an infinity before the kernel runs.
#[cfg(test)]
mod tests {
    use super::product;

    #[test]
    fn the_kernel_passes_a_nan_through_and_an_infinity_on_instead_of_panicking() {
        assert!(product(f64::NAN, 2.0).is_nan());
        assert!(product(2.0, f64::NAN).is_nan());
        assert_eq!(product(f64::INFINITY, -2.0), f64::NEG_INFINITY);
        assert!(product(f64::INFINITY, 0.0).is_nan());
    }
}
