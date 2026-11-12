//! `bullterrier_audio::scale` through the public API: worked rows, refusals, and the properties of the product.

use bullterrier_audio::{Error, scale};
use proptest::prelude::*;

use crate::common::finite;

/// Each product worked out by hand.
#[test]
fn scaling_gives_the_product() {
    let rows = [
        (3.0, 2.0, 6.0),
        (0.5, 0.5, 0.25),
        (-4.0, 0.25, -1.0),
        (7.0, 0.0, 0.0),
        (1.0e200, 1.0e-200, 1.0),
    ];
    for (value, factor, expected) in rows {
        assert_eq!(scale(value, factor), Ok(expected), "{value} * {factor}");
    }
}

#[test]
fn a_zero_result_is_never_negative() {
    for (value, factor) in [(0.0, -2.0), (-3.0, 0.0), (-0.0, 5.0)] {
        assert!(
            scale(value, factor).is_ok_and(f64::is_sign_positive),
            "{value} * {factor}"
        );
    }
}

#[test]
fn a_refusal_names_the_argument_or_the_overflow() {
    let rows = [
        (f64::NAN, 2.0, Error::NotFinite { name: "value" }),
        (2.0, f64::INFINITY, Error::NotFinite { name: "factor" }),
        (f64::NEG_INFINITY, f64::NAN, Error::NotFinite { name: "value" }),
        (1.0e200, 1.0e200, Error::Overflow),
    ];
    for (value, factor, expected) in rows {
        assert_eq!(scale(value, factor), Err(expected), "{value} * {factor}");
    }
}

#[test]
fn a_refusal_says_what_was_wrong() {
    assert_eq!(
        Error::NotFinite { name: "factor" }.to_string(),
        "factor must be a finite number"
    );
    assert!(Error::Overflow.to_string().contains("too large"));
}

proptest! {
    // laws of the product that hold exactly in IEEE arithmetic, so no formula of the code is restated
    #[test]
    fn scaling_commutes_and_flips_its_sign_with_the_factor(value in finite(), factor in finite()) {
        prop_assert_eq!(scale(value, factor), scale(factor, value));
        if let (Ok(up), Ok(down)) = (scale(value, factor), scale(value, -factor)) {
            prop_assert_eq!(up.to_bits(), if down == 0.0 { 0.0 } else { -down }.to_bits());
        }
    }

    #[test]
    fn scaling_by_one_keeps_the_value_and_by_zero_gives_zero(value in finite()) {
        prop_assert_eq!(scale(value, 1.0).map(f64::to_bits), Ok(if value == 0.0 { 0.0 } else { value }.to_bits()));
        prop_assert_eq!(scale(value, 0.0), Ok(0.0));
    }
}
