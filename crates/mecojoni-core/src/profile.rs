use crate::{Rational, RationalError};

/// Version of the baseline diversity profile.
pub const LOCATION_PROFILE_VERSION: &str = "location/1";
/// Version of the baseline bounded interactive resource profile.
pub const INTERACTIVE_PROFILE_VERSION: &str = "interactive/1";
/// Version of the optional composition audit heuristic.
pub const COMPOSITION_PROFILE_VERSION: &str = "composition/1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocationProfile {
    pub candidate_attempts: u32,
    pub hard_minimum_gap: u32,
    pub soft_cooldown_horizon: u32,
    pub soft_cooldown_numerator: u32,
    pub soft_cooldown_denominator: u32,
    pub minimum_edge_words: u32,
    pub maximum_edge_words: u32,
    pub internal_boundary_words: u32,
    pub edge_history_window: u32,
    pub exact_history_window: u32,
    pub edge_history_logical_bytes: u64,
    pub exact_history_logical_bytes: u64,
}

impl LocationProfile {
    pub const V1: Self = Self {
        candidate_attempts: 12,
        hard_minimum_gap: 1,
        soft_cooldown_horizon: 4,
        soft_cooldown_numerator: 3,
        soft_cooldown_denominator: 4,
        minimum_edge_words: 3,
        maximum_edge_words: 8,
        internal_boundary_words: 2,
        edge_history_window: 300,
        exact_history_window: 50_000,
        edge_history_logical_bytes: 4 * 1024 * 1024,
        exact_history_logical_bytes: 16 * 1024 * 1024,
    };
}

/// Computes the exact `location/1` structural diversity factor in unsigned
/// 16.16 fixed-point units.
#[must_use]
pub fn diversity_factor_16_16(descendants: u64) -> u32 {
    let descendants = descendants.max(1);
    let floor_log2 = descendants.ilog2();
    let radicand = u64::from(floor_log2 + 1) << 32;
    u32::try_from(integer_sqrt(radicand))
        .unwrap_or(u32::MAX)
        .min(4 << 16)
}

/// Computes the exact `location/1` soft-cooldown multiplier for a committed
/// selection age. Hard-gap eligibility filtering happens separately.
///
/// # Errors
///
/// Returns [`RationalError`] only if the published profile constants cease to
/// fit the `rational/1` budget.
pub fn location_cooldown_multiplier(age: u32) -> Result<Rational, RationalError> {
    let profile = LocationProfile::V1;
    if age >= profile.soft_cooldown_horizon {
        return Ok(Rational::ONE);
    }
    if age <= profile.hard_minimum_gap {
        return Rational::new(1, 4);
    }
    let range = profile.soft_cooldown_horizon - profile.hard_minimum_gap;
    let progress = age - profile.hard_minimum_gap;
    let numerator = i64::from(range + 3 * progress);
    Rational::new(numerator, u64::from(4 * range))
}

const fn integer_sqrt(mut value: u64) -> u64 {
    let mut result = 0_u64;
    let mut bit = 1_u64 << 62;
    while bit > value {
        bit >>= 2;
    }
    while bit != 0 {
        if value >= result + bit {
            value -= result + bit;
            result = (result >> 1) + bit;
        } else {
            result >>= 1;
        }
        bit >>= 2;
    }
    result
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResourceProfile {
    pub candidate_attempts: u32,
    pub maximum_depth_per_candidate: u32,
    pub maximum_expansions_per_candidate: u32,
    pub maximum_unformatted_scalars_per_candidate: u32,
    pub maximum_sampler_steps_per_candidate: u32,
    pub maximum_aggregate_expansions: u32,
    pub maximum_aggregate_sampler_steps: u32,
    pub maximum_rendered_scalars: u32,
    pub maximum_rendered_utf8_bytes: u32,
    pub maximum_formatter_work_units: u32,
}

impl ResourceProfile {
    pub const WEIGHTED_INTERACTIVE_V1: Self = Self {
        candidate_attempts: 1,
        maximum_depth_per_candidate: 80,
        maximum_expansions_per_candidate: 2_000,
        maximum_unformatted_scalars_per_candidate: 16_384,
        maximum_sampler_steps_per_candidate: 8_192,
        maximum_aggregate_expansions: 2_000,
        maximum_aggregate_sampler_steps: 8_192,
        maximum_rendered_scalars: 16_384,
        maximum_rendered_utf8_bytes: 65_536,
        maximum_formatter_work_units: 10_000,
    };

    pub const DIVERSE_INTERACTIVE_V1: Self = Self {
        candidate_attempts: 12,
        maximum_depth_per_candidate: 80,
        maximum_expansions_per_candidate: 2_000,
        maximum_unformatted_scalars_per_candidate: 16_384,
        maximum_sampler_steps_per_candidate: 8_192,
        maximum_aggregate_expansions: 24_000,
        maximum_aggregate_sampler_steps: 98_304,
        maximum_rendered_scalars: 16_384,
        maximum_rendered_utf8_bytes: 65_536,
        maximum_formatter_work_units: 10_000,
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompositionProfile {
    pub minimum_direct_references: u32,
    pub maximum_literal_run_words: u32,
    pub complete_messages_are_exempt: bool,
}

impl CompositionProfile {
    pub const V1: Self = Self {
        minimum_direct_references: 3,
        maximum_literal_run_words: 2,
        complete_messages_are_exempt: true,
    };
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::{
        CompositionProfile, LocationProfile, ResourceProfile, diversity_factor_16_16,
        location_cooldown_multiplier,
    };

    #[test]
    fn profile_aggregate_limits_cover_every_candidate() {
        let diverse = ResourceProfile::DIVERSE_INTERACTIVE_V1;

        assert_eq!(
            diverse.maximum_aggregate_expansions,
            diverse.candidate_attempts * diverse.maximum_expansions_per_candidate
        );
        assert_eq!(
            diverse.maximum_aggregate_sampler_steps,
            diverse.candidate_attempts * diverse.maximum_sampler_steps_per_candidate
        );
    }

    #[test]
    fn location_and_composition_contracts_match_the_published_values() {
        assert_eq!(LocationProfile::V1.candidate_attempts, 12);
        assert_eq!(LocationProfile::V1.exact_history_window, 50_000);
        assert_eq!(CompositionProfile::V1.minimum_direct_references, 3);
    }

    #[test]
    fn location_math_uses_exact_fixed_point_and_rationals() {
        assert_eq!(diversity_factor_16_16(1), 65_536);
        assert_eq!(diversity_factor_16_16(2), 92_681);
        assert_eq!(diversity_factor_16_16(32_768), 262_144);
        assert_eq!(
            location_cooldown_multiplier(1)
                .expect("age one")
                .to_string(),
            "1/4"
        );
        assert_eq!(
            location_cooldown_multiplier(2)
                .expect("age two")
                .to_string(),
            "1/2"
        );
        assert_eq!(
            location_cooldown_multiplier(3)
                .expect("age three")
                .to_string(),
            "3/4"
        );
        assert_eq!(
            location_cooldown_multiplier(4)
                .expect("age four")
                .to_string(),
            "1"
        );
    }
}
