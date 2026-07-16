use core::{fmt, str::FromStr};

/// Maximum absolute numerator and denominator accepted by `rational/1`.
pub const RATIONAL_LIMIT: u64 = i64::MAX as u64;
/// Compatibility identifier for exact Mecojoni number arithmetic.
pub const RATIONAL_VERSION: &str = "rational/1";

/// Reduced signed rational used by deterministic numeric expressions.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Rational {
    numerator: i64,
    denominator: u64,
}

impl Rational {
    pub const ZERO: Self = Self {
        numerator: 0,
        denominator: 1,
    };
    pub const ONE: Self = Self {
        numerator: 1,
        denominator: 1,
    };

    /// Creates and reduces a rational within the versioned 63-bit budget.
    ///
    /// # Errors
    ///
    /// Returns [`RationalError::InvalidSyntax`] for a zero denominator and
    /// [`RationalError::Overflow`] when the reduced value exceeds the budget.
    pub fn new(numerator: i64, denominator: u64) -> Result<Self, RationalError> {
        if denominator == 0 {
            return Err(RationalError::InvalidSyntax);
        }
        reduce(i128::from(numerator), u128::from(denominator))
    }

    #[must_use]
    pub const fn numerator(self) -> i64 {
        self.numerator
    }

    #[must_use]
    pub const fn denominator(self) -> u64 {
        self.denominator
    }

    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.numerator == 0
    }

    #[must_use]
    pub const fn is_positive(self) -> bool {
        self.numerator > 0
    }

    /// Adds two exact values within the versioned 63-bit result budget.
    ///
    /// # Errors
    ///
    /// Returns [`RationalError::Overflow`] when the reduced result exceeds the
    /// numerator or denominator budget.
    pub fn checked_add(self, other: Self) -> Result<Self, RationalError> {
        let left = i128::from(self.numerator)
            .checked_mul(i128::from(other.denominator))
            .ok_or(RationalError::Overflow)?;
        let right = i128::from(other.numerator)
            .checked_mul(i128::from(self.denominator))
            .ok_or(RationalError::Overflow)?;
        let numerator = left.checked_add(right).ok_or(RationalError::Overflow)?;
        let denominator = u128::from(self.denominator)
            .checked_mul(u128::from(other.denominator))
            .ok_or(RationalError::Overflow)?;
        reduce(numerator, denominator)
    }

    /// Subtracts two exact values within the versioned 63-bit result budget.
    ///
    /// # Errors
    ///
    /// Returns [`RationalError::Overflow`] when the reduced result exceeds the
    /// numerator or denominator budget.
    pub fn checked_sub(self, other: Self) -> Result<Self, RationalError> {
        let negated = other
            .numerator
            .checked_neg()
            .ok_or(RationalError::Overflow)?;
        self.checked_add(Self {
            numerator: negated,
            denominator: other.denominator,
        })
    }

    /// Multiplies two exact values within the versioned 63-bit result budget.
    ///
    /// # Errors
    ///
    /// Returns [`RationalError::Overflow`] when the reduced result exceeds the
    /// numerator or denominator budget.
    pub fn checked_mul(self, other: Self) -> Result<Self, RationalError> {
        let numerator = i128::from(self.numerator)
            .checked_mul(i128::from(other.numerator))
            .ok_or(RationalError::Overflow)?;
        let denominator = u128::from(self.denominator)
            .checked_mul(u128::from(other.denominator))
            .ok_or(RationalError::Overflow)?;
        reduce(numerator, denominator)
    }
}

impl FromStr for Rational {
    type Err = RationalError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        parse_decimal(source)
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.denominator == 1 {
            write!(formatter, "{}", self.numerator)
        } else {
            write!(formatter, "{}/{}", self.numerator, self.denominator)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RationalError {
    InvalidSyntax,
    ExponentOutOfRange,
    Overflow,
}

impl fmt::Display for RationalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSyntax => formatter.write_str("invalid decimal-rational syntax"),
            Self::ExponentOutOfRange => formatter.write_str("decimal exponent is outside -18..=18"),
            Self::Overflow => formatter.write_str("rational exceeds its 63-bit budget"),
        }
    }
}

fn parse_decimal(source: &str) -> Result<Rational, RationalError> {
    let (negative, unsigned) = source
        .strip_prefix('-')
        .map_or((false, source), |rest| (true, rest));
    if unsigned.is_empty() || unsigned.starts_with('+') {
        return Err(RationalError::InvalidSyntax);
    }

    let (mantissa, exponent) = split_exponent(unsigned)?;
    let (integer, fraction) = mantissa
        .split_once('.')
        .map_or((mantissa, None), |(integer, fraction)| {
            (integer, Some(fraction))
        });
    if integer.is_empty()
        || !integer.bytes().all(|byte| byte.is_ascii_digit())
        || (integer.len() > 1 && integer.starts_with('0'))
    {
        return Err(RationalError::InvalidSyntax);
    }
    let fraction = fraction.unwrap_or("");
    if mantissa.contains('.') && fraction.is_empty() {
        return Err(RationalError::InvalidSyntax);
    }
    if !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(RationalError::InvalidSyntax);
    }
    let digit_count = integer.len() + fraction.len();
    if digit_count > 18 {
        return Err(RationalError::Overflow);
    }

    let mut coefficient = 0_u128;
    for byte in integer.bytes().chain(fraction.bytes()) {
        coefficient = coefficient
            .checked_mul(10)
            .and_then(|value| value.checked_add(u128::from(byte - b'0')))
            .ok_or(RationalError::Overflow)?;
    }
    if coefficient == 0 {
        return Ok(Rational::ZERO);
    }

    let mut power =
        exponent - i32::try_from(fraction.len()).map_err(|_| RationalError::Overflow)?;
    while coefficient % 10 == 0 {
        coefficient /= 10;
        power += 1;
    }

    let (numerator, denominator) = if power >= 0 {
        let factor =
            checked_power_of_ten(u32::try_from(power).map_err(|_| RationalError::Overflow)?)?;
        (
            coefficient
                .checked_mul(factor)
                .ok_or(RationalError::Overflow)?,
            1,
        )
    } else {
        (coefficient, checked_power_of_ten(power.unsigned_abs())?)
    };
    let signed = i128::try_from(numerator).map_err(|_| RationalError::Overflow)?;
    reduce(if negative { -signed } else { signed }, denominator)
}

fn split_exponent(source: &str) -> Result<(&str, i32), RationalError> {
    let marker = source.find(['e', 'E']);
    let Some(index) = marker else {
        return Ok((source, 0));
    };
    if source[index + 1..].contains(['e', 'E']) {
        return Err(RationalError::InvalidSyntax);
    }
    let mantissa = &source[..index];
    let exponent_source = &source[index + 1..];
    if exponent_source.is_empty() {
        return Err(RationalError::InvalidSyntax);
    }
    let (negative, digits) = exponent_source.strip_prefix('-').map_or_else(
        || {
            exponent_source
                .strip_prefix('+')
                .map_or((false, exponent_source), |rest| (false, rest))
        },
        |rest| (true, rest),
    );
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(RationalError::InvalidSyntax);
    }
    let magnitude = digits
        .parse::<i32>()
        .map_err(|_| RationalError::ExponentOutOfRange)?;
    let exponent = if negative { -magnitude } else { magnitude };
    if !(-18..=18).contains(&exponent) {
        return Err(RationalError::ExponentOutOfRange);
    }
    Ok((mantissa, exponent))
}

fn checked_power_of_ten(exponent: u32) -> Result<u128, RationalError> {
    10_u128.checked_pow(exponent).ok_or(RationalError::Overflow)
}

fn reduce(numerator: i128, denominator: u128) -> Result<Rational, RationalError> {
    if numerator == 0 {
        return Ok(Rational::ZERO);
    }
    let magnitude = numerator.unsigned_abs();
    let divisor = gcd(magnitude, denominator);
    let reduced_numerator = magnitude / divisor;
    let reduced_denominator = denominator / divisor;
    if reduced_numerator > u128::from(RATIONAL_LIMIT)
        || reduced_denominator > u128::from(RATIONAL_LIMIT)
    {
        return Err(RationalError::Overflow);
    }
    let magnitude = i64::try_from(reduced_numerator).map_err(|_| RationalError::Overflow)?;
    Ok(Rational {
        numerator: if numerator < 0 { -magnitude } else { magnitude },
        denominator: u64::try_from(reduced_denominator).map_err(|_| RationalError::Overflow)?,
    })
}

const fn gcd(mut left: u128, mut right: u128) -> u128 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;
    use core::str::FromStr;

    use super::{RATIONAL_VERSION, Rational, RationalError};

    #[test]
    fn parses_and_reduces_decimal_and_scientific_forms() {
        assert_eq!(RATIONAL_VERSION, "rational/1");
        assert_eq!(
            Rational::from_str("0.5").expect("decimal"),
            Rational {
                numerator: 1,
                denominator: 2
            }
        );
        assert_eq!(
            Rational::from_str("1.25e2").expect("scientific"),
            Rational {
                numerator: 125,
                denominator: 1
            }
        );
        assert_eq!(
            Rational::from_str("-0.125").expect("negative"),
            Rational {
                numerator: -1,
                denominator: 8
            }
        );
        assert_eq!(
            Rational::from_str("1e-18").expect("small"),
            Rational {
                numerator: 1,
                denominator: 1_000_000_000_000_000_000
            }
        );
    }

    #[test]
    fn arithmetic_is_exact_and_bounded() {
        let half = Rational::from_str("0.5").expect("half");
        let quarter = Rational::from_str("0.25").expect("quarter");

        assert_eq!(half.checked_add(quarter).expect("sum").to_string(), "3/4");
        assert_eq!(
            half.checked_sub(quarter).expect("difference").to_string(),
            "1/4"
        );
        assert_eq!(
            half.checked_mul(quarter).expect("product").to_string(),
            "1/8"
        );
    }

    #[test]
    fn rejects_noncanonical_or_out_of_budget_numbers() {
        assert_eq!(Rational::from_str("01"), Err(RationalError::InvalidSyntax));
        assert_eq!(Rational::from_str(".5"), Err(RationalError::InvalidSyntax));
        assert_eq!(Rational::from_str("1."), Err(RationalError::InvalidSyntax));
        assert_eq!(
            Rational::from_str("1e19"),
            Err(RationalError::ExponentOutOfRange)
        );
        assert_eq!(
            Rational::from_str("9223372036854775808"),
            Err(RationalError::Overflow)
        );
    }
}
