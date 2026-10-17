use serde::{Deserialize, Serialize};

pub const DEFAULT_CONFIDENCE_Z: f64 = 1.96;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonDirection {
    HigherIsBetter,
    LowerIsBetter,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RateEstimate {
    pub successes: f64,
    pub trials: f64,
    pub point: f64,
    pub lower_bound: f64,
    pub upper_bound: f64,
    pub standard_error: f64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeanEstimate {
    pub total: f64,
    pub count: f64,
    pub point: f64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeanVarianceEstimate {
    pub total: f64,
    pub count: f64,
    pub point: f64,
    pub square_total: f64,
    pub sample_variance: f64,
    pub standard_error: f64,
    pub lower_bound: f64,
    pub upper_bound: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreatmentComparison {
    pub direction: ComparisonDirection,
    pub control_point: f64,
    pub treatment_point: f64,
    pub point_improvement: f64,
    pub conservative_improvement: f64,
    pub minimum_meaningful_improvement: f64,
    pub meaningful: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuardrailComparison {
    pub direction: ComparisonDirection,
    pub control_point: f64,
    pub treatment_point: f64,
    pub point_harm: f64,
    pub conservative_harm: f64,
    pub minimum_meaningful_harm: f64,
    pub severe_point_harm_threshold: f64,
    pub severe_point_harm: bool,
    pub breached: bool,
}

/// Round like JavaScript `Math.round`: halves, including negative ones, round up.
pub fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

fn clamp(value: f64, minimum: f64, maximum: f64) -> f64 {
    if value.is_finite() {
        value.max(minimum).min(maximum)
    } else {
        minimum
    }
}

/// Estimate a bounded binary rate using the policy's Wilson interval.
pub fn estimate_rate(successes: f64, trials: f64, confidence_z: f64) -> RateEstimate {
    let safe_trials = js_round(trials).max(0.0);
    let safe_successes = clamp(js_round(successes), 0.0, safe_trials);
    if safe_trials == 0.0 {
        return RateEstimate::default();
    }
    let point = safe_successes / safe_trials;
    let z_squared = confidence_z * confidence_z;
    let denominator = 1.0 + z_squared / safe_trials;
    let center = point + z_squared / (2.0 * safe_trials);
    let margin = confidence_z
        * ((point * (1.0 - point) + z_squared / (4.0 * safe_trials)) / safe_trials).sqrt();
    RateEstimate {
        successes: safe_successes,
        trials: safe_trials,
        point,
        lower_bound: clamp((center - margin) / denominator, 0.0, 1.0),
        upper_bound: clamp((center + margin) / denominator, 0.0, 1.0),
        standard_error: ((point * (1.0 - point)) / safe_trials).sqrt(),
    }
}

/// Estimate a mean from its aggregate, rounding the count like JavaScript `Math.round`.
pub fn estimate_mean(total: f64, count: f64) -> MeanEstimate {
    let count = js_round(count).max(0.0);
    let total = if total.is_finite() { total } else { 0.0 };
    MeanEstimate {
        total,
        count,
        point: if count > 0.0 { total / count } else { 0.0 },
    }
}

/// Estimate an aggregate mean and interval without rounding weighted counts.
pub fn estimate_mean_with_variance(
    total: f64,
    square_total: f64,
    count: f64,
    confidence_z: f64,
) -> MeanVarianceEstimate {
    let total = if total.is_finite() { total } else { 0.0 };
    let square_total = if square_total.is_finite() {
        square_total.max(0.0)
    } else {
        0.0
    };
    let count = if count.is_finite() {
        count.max(0.0)
    } else {
        0.0
    };
    if count <= 0.0 {
        return MeanVarianceEstimate::default();
    }
    let point = total / count;
    let sample_variance = if count > 1.0 {
        ((square_total - (total * total) / count) / (count - 1.0)).max(0.0)
    } else {
        0.0
    };
    let standard_error = (sample_variance / count).sqrt();
    let margin = confidence_z * standard_error;
    MeanVarianceEstimate {
        total,
        count,
        point,
        square_total,
        sample_variance,
        standard_error,
        lower_bound: point - margin,
        upper_bound: point + margin,
    }
}

/// Compare interval improvement; an improvement equal to the minimum is meaningful.
pub fn compare_point_estimates(
    control_point: f64,
    treatment_point: f64,
    conservative_improvement: f64,
    direction: ComparisonDirection,
    minimum_meaningful_improvement: f64,
) -> TreatmentComparison {
    let point_improvement = match direction {
        ComparisonDirection::HigherIsBetter => treatment_point - control_point,
        ComparisonDirection::LowerIsBetter => control_point - treatment_point,
    };
    TreatmentComparison {
        direction,
        control_point,
        treatment_point,
        point_improvement,
        conservative_improvement,
        minimum_meaningful_improvement,
        meaningful: conservative_improvement >= minimum_meaningful_improvement,
    }
}

pub fn compare_rate_estimates(
    control: RateEstimate,
    treatment: RateEstimate,
    direction: ComparisonDirection,
    minimum: f64,
) -> TreatmentComparison {
    let improvement = match direction {
        ComparisonDirection::HigherIsBetter => treatment.lower_bound - control.upper_bound,
        ComparisonDirection::LowerIsBetter => control.lower_bound - treatment.upper_bound,
    };
    compare_point_estimates(
        control.point,
        treatment.point,
        improvement,
        direction,
        minimum,
    )
}

pub fn compare_mean_estimates(
    control: MeanVarianceEstimate,
    treatment: MeanVarianceEstimate,
    direction: ComparisonDirection,
    minimum: f64,
) -> TreatmentComparison {
    let improvement = match direction {
        ComparisonDirection::HigherIsBetter => treatment.lower_bound - control.upper_bound,
        ComparisonDirection::LowerIsBetter => control.lower_bound - treatment.upper_bound,
    };
    compare_point_estimates(
        control.point,
        treatment.point,
        improvement,
        direction,
        minimum,
    )
}

fn guardrail_comparison(
    control_point: f64,
    treatment_point: f64,
    conservative_harm: f64,
    direction: ComparisonDirection,
    minimum_meaningful_harm: f64,
    severe_point_harm_threshold: f64,
) -> GuardrailComparison {
    let point_harm = match direction {
        ComparisonDirection::HigherIsBetter => control_point - treatment_point,
        ComparisonDirection::LowerIsBetter => treatment_point - control_point,
    };
    let severe_point_harm = point_harm >= severe_point_harm_threshold;
    GuardrailComparison {
        direction,
        control_point,
        treatment_point,
        point_harm,
        conservative_harm,
        minimum_meaningful_harm,
        severe_point_harm_threshold,
        severe_point_harm,
        breached: conservative_harm >= minimum_meaningful_harm || severe_point_harm,
    }
}

pub fn compare_rate_guardrail(
    control: RateEstimate,
    treatment: RateEstimate,
    direction: ComparisonDirection,
    minimum: f64,
    severe: f64,
) -> GuardrailComparison {
    let harm = match direction {
        ComparisonDirection::HigherIsBetter => control.lower_bound - treatment.upper_bound,
        ComparisonDirection::LowerIsBetter => treatment.lower_bound - control.upper_bound,
    };
    guardrail_comparison(
        control.point,
        treatment.point,
        harm,
        direction,
        minimum,
        severe,
    )
}

pub fn compare_mean_guardrail(
    control: MeanVarianceEstimate,
    treatment: MeanVarianceEstimate,
    direction: ComparisonDirection,
    minimum: f64,
    severe: f64,
) -> GuardrailComparison {
    let harm = match direction {
        ComparisonDirection::HigherIsBetter => control.lower_bound - treatment.upper_bound,
        ComparisonDirection::LowerIsBetter => treatment.lower_bound - control.upper_bound,
    };
    guardrail_comparison(
        control.point,
        treatment.point,
        harm,
        direction,
        minimum,
        severe,
    )
}
