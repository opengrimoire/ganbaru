use crate::adaptive::experiments::models::{
    ExperimentAnalysis, ExperimentLane, ExperimentOutcome, OutcomeValues,
};
use crate::adaptive::experiments::outcomes::{pooled_outcomes, variant_outcome};
use crate::adaptive::statistics::{self as stats, ComparisonDirection, MeanVarianceEstimate};

const MIN_RUNS: f64 = 8.0;
const MIN_POOLING_RUNS: f64 = 4.0;
const MIN_DAY_OBSERVATIONS: f64 = 4.0;
const MIN_NEXT_DAY_OBSERVATIONS: f64 = 4.0;
const COMPLETION_DROP: f64 = 0.15;
const SEVERE_COMPLETION_DROP: f64 = 0.25;
const STOP_INCREASE: f64 = 0.1;
const SEVERE_STOP_INCREASE: f64 = 0.2;
const NEXT_DAY_DROP: f64 = 0.15;
const SEVERE_NEXT_DAY_DROP: f64 = 0.3;
const CLEAN_FOCUS_CHANGE_SECONDS: f64 = 240.0;
const COMPLETION_IMPROVEMENT: f64 = 0.1;
const COMPLETION_TOLERANCE: f64 = 0.05;
const NEXT_DAY_TOLERANCE: f64 = 0.05;

/// Analyze exact, neighboring, pooled, then global evidence, returning the first
/// scope with enough runs.
pub fn analyze_experiment(
    lane: ExperimentLane,
    outcomes: &[ExperimentOutcome],
    context: Option<&str>,
) -> ExperimentAnalysis {
    if let Some(context) = context.filter(|value| !value.is_empty()) {
        let exact = analyze_scope(lane, outcomes, Some(context), "context");
        if enough_runs(&exact, MIN_RUNS) {
            return exact;
        }
        if enough_runs(&exact, MIN_POOLING_RUNS) {
            let neighbors = analyze_scope(
                lane,
                &pooled_outcomes(outcomes, context, true),
                Some(context),
                "neighbor_pooled",
            );
            if enough_runs(&neighbors, MIN_RUNS) {
                return neighbors;
            }
            let pooled = analyze_scope(
                lane,
                &pooled_outcomes(outcomes, context, false),
                Some(context),
                "pooled",
            );
            if enough_runs(&pooled, MIN_RUNS) {
                return pooled;
            }
        }
    }
    analyze_scope(lane, outcomes, None, "global")
}

fn enough_runs(analysis: &ExperimentAnalysis, minimum: f64) -> bool {
    analysis.control_observed_runs >= minimum && analysis.treatment_observed_runs >= minimum
}

fn analyze_scope(
    lane: ExperimentLane,
    outcomes: &[ExperimentOutcome],
    context: Option<&str>,
    scope: &str,
) -> ExperimentAnalysis {
    let definition = lane.definition();
    let control = variant_outcome(
        outcomes,
        &definition.id,
        &definition.variants[0].variant_key,
        context,
    );
    let treatment = variant_outcome(
        outcomes,
        &definition.id,
        &definition.variants[1].variant_key,
        context,
    );
    let c = &control.values;
    let t = &treatment.values;
    let has_evidence = c.run_observed_count >= MIN_RUNS && t.run_observed_count >= MIN_RUNS;
    let breached = has_evidence && guardrail_breached(lane, c, t);
    let decision = if !has_evidence {
        "insufficient_data"
    } else if breached {
        "prefer_control"
    } else if prefer_treatment(lane, c, t) {
        "prefer_treatment"
    } else {
        "continue"
    };
    ExperimentAnalysis {
        experiment_id: definition.id,
        control_variant_key: definition.variants[0].variant_key.clone(),
        treatment_variant_key: definition.variants[1].variant_key.clone(),
        control_observed_runs: c.run_observed_count,
        treatment_observed_runs: t.run_observed_count,
        control_completion_rate: ratio(c.run_completed_count, c.run_observed_count),
        treatment_completion_rate: ratio(t.run_completed_count, t.run_observed_count),
        control_stop_rate: ratio(c.run_stopped_count, c.run_observed_count),
        treatment_stop_rate: ratio(t.run_stopped_count, t.run_observed_count),
        control_clean_focus_seconds_mean: mean(c.clean_focus_seconds_sum, c.run_observed_count),
        treatment_clean_focus_seconds_mean: mean(t.clean_focus_seconds_sum, t.run_observed_count),
        control_blocked_attempts_mean: mean(c.blocked_attempt_count_sum, c.run_observed_count),
        treatment_blocked_attempts_mean: mean(t.blocked_attempt_count_sum, t.run_observed_count),
        control_break_skipped_mean: mean(c.break_skipped_count_sum, c.run_observed_count),
        treatment_break_skipped_mean: mean(t.break_skipped_count_sum, t.run_observed_count),
        control_short_break_overtime_seconds_mean: mean(
            c.short_break_overtime_seconds_sum,
            c.run_observed_count,
        ),
        treatment_short_break_overtime_seconds_mean: mean(
            t.short_break_overtime_seconds_sum,
            t.run_observed_count,
        ),
        control_long_break_overtime_seconds_mean: mean(
            c.long_break_overtime_seconds_sum,
            c.run_observed_count,
        ),
        treatment_long_break_overtime_seconds_mean: mean(
            t.long_break_overtime_seconds_sum,
            t.run_observed_count,
        ),
        control_day_missed_planned_pomodoro_mean: enough_day(c).then(|| {
            mean(
                c.day_missed_planned_pomodoro_count_sum,
                c.day_observed_count,
            )
        }),
        treatment_day_missed_planned_pomodoro_mean: enough_day(t).then(|| {
            mean(
                t.day_missed_planned_pomodoro_count_sum,
                t.day_observed_count,
            )
        }),
        control_day_blocked_attempts_mean: enough_day(c)
            .then(|| mean(c.day_blocked_attempt_count_sum, c.day_observed_count)),
        treatment_day_blocked_attempts_mean: enough_day(t)
            .then(|| mean(t.day_blocked_attempt_count_sum, t.day_observed_count)),
        control_next_day_started_rate: enough_next_day(c)
            .then(|| ratio(c.next_day_started_run_count, c.next_day_observed_count)),
        treatment_next_day_started_rate: enough_next_day(t)
            .then(|| ratio(t.next_day_started_run_count, t.next_day_observed_count)),
        analysis_scope: scope.to_owned(),
        guardrail_breached: breached,
        decision: decision.to_owned(),
    }
}

fn guardrail_breached(lane: ExperimentLane, c: &OutcomeValues, t: &OutcomeValues) -> bool {
    let basic_breached = rate_harm(
        c.run_completed_count,
        c.run_observed_count,
        t.run_completed_count,
        t.run_observed_count,
        ComparisonDirection::HigherIsBetter,
        COMPLETION_DROP,
        SEVERE_COMPLETION_DROP,
    ) || rate_harm(
        c.run_stopped_count,
        c.run_observed_count,
        t.run_stopped_count,
        t.run_observed_count,
        ComparisonDirection::LowerIsBetter,
        STOP_INCREASE,
        SEVERE_STOP_INCREASE,
    ) || mean_harm(
        c,
        t,
        Metric::Blocked,
        ComparisonDirection::LowerIsBetter,
        2.0,
    ) || (enough_day(c)
        && enough_day(t)
        && (mean_harm(
            c,
            t,
            Metric::DayMissed,
            ComparisonDirection::LowerIsBetter,
            0.75,
        ) || mean_harm(
            c,
            t,
            Metric::DayBlocked,
            ComparisonDirection::LowerIsBetter,
            3.0,
        )))
        || (enough_next_day(c)
            && enough_next_day(t)
            && rate_harm(
                c.next_day_started_run_count,
                c.next_day_observed_count,
                t.next_day_started_run_count,
                t.next_day_observed_count,
                ComparisonDirection::HigherIsBetter,
                NEXT_DAY_DROP,
                SEVERE_NEXT_DAY_DROP,
            ));
    if basic_breached {
        return true;
    }
    if lane == ExperimentLane::FocusDuration {
        return false;
    }
    if mean_harm(
        c,
        t,
        Metric::Skipped,
        ComparisonDirection::LowerIsBetter,
        0.5,
    ) {
        return true;
    }
    match lane {
        ExperimentLane::FocusSupport | ExperimentLane::ShortBreak => mean_harm(
            c,
            t,
            Metric::ShortOvertime,
            ComparisonDirection::LowerIsBetter,
            60.0,
        ),
        ExperimentLane::LongBreak | ExperimentLane::LaterCadence => mean_harm(
            c,
            t,
            Metric::LongOvertime,
            ComparisonDirection::LowerIsBetter,
            120.0,
        ),
        ExperimentLane::LongRecovery => {
            clean_focus_loss(c, t)
                || mean_harm(
                    c,
                    t,
                    Metric::LongOvertime,
                    ComparisonDirection::LowerIsBetter,
                    120.0,
                )
        }
        ExperimentLane::EarlierCadence => clean_focus_loss(c, t),
        ExperimentLane::FocusDuration => false,
    }
}

fn prefer_treatment(lane: ExperimentLane, c: &OutcomeValues, t: &OutcomeValues) -> bool {
    if ratio(t.run_completed_count, t.run_observed_count)
        < ratio(c.run_completed_count, c.run_observed_count) - COMPLETION_TOLERANCE
    {
        return false;
    }
    if enough_next_day(c)
        && enough_next_day(t)
        && ratio(t.next_day_started_run_count, t.next_day_observed_count)
            < ratio(c.next_day_started_run_count, c.next_day_observed_count) - NEXT_DAY_TOLERANCE
    {
        return false;
    }
    match lane {
        ExperimentLane::FocusDuration
        | ExperimentLane::FocusSupport
        | ExperimentLane::LaterCadence => mean_gain(
            c,
            t,
            Metric::Clean,
            ComparisonDirection::HigherIsBetter,
            CLEAN_FOCUS_CHANGE_SECONDS,
        ),
        ExperimentLane::ShortBreak => mean_gain(
            c,
            t,
            Metric::ShortOvertime,
            ComparisonDirection::LowerIsBetter,
            60.0,
        ),
        ExperimentLane::LongBreak => mean_gain(
            c,
            t,
            Metric::LongOvertime,
            ComparisonDirection::LowerIsBetter,
            120.0,
        ),
        ExperimentLane::LongRecovery | ExperimentLane::EarlierCadence => {
            if clean_focus_loss(c, t) {
                return false;
            }
            mean_gain(
                c,
                t,
                Metric::Blocked,
                ComparisonDirection::LowerIsBetter,
                1.0,
            ) || (lane == ExperimentLane::LongRecovery
                && mean_gain(
                    c,
                    t,
                    Metric::LongOvertime,
                    ComparisonDirection::LowerIsBetter,
                    120.0,
                ))
                || stats::compare_rate_estimates(
                    stats::estimate_rate(
                        c.run_completed_count,
                        c.run_observed_count,
                        stats::DEFAULT_CONFIDENCE_Z,
                    ),
                    stats::estimate_rate(
                        t.run_completed_count,
                        t.run_observed_count,
                        stats::DEFAULT_CONFIDENCE_Z,
                    ),
                    ComparisonDirection::HigherIsBetter,
                    COMPLETION_IMPROVEMENT,
                )
                .meaningful
        }
    }
}

#[derive(Clone, Copy)]
enum Metric {
    Clean,
    Blocked,
    Skipped,
    ShortOvertime,
    LongOvertime,
    DayMissed,
    DayBlocked,
}
fn estimate(values: &OutcomeValues, metric: Metric) -> MeanVarianceEstimate {
    let (sum, squares, count) = match metric {
        Metric::Clean => (
            values.clean_focus_seconds_sum,
            values.clean_focus_seconds_square_sum,
            values.run_observed_count,
        ),
        Metric::Blocked => (
            values.blocked_attempt_count_sum,
            values.blocked_attempt_count_square_sum,
            values.run_observed_count,
        ),
        Metric::Skipped => (
            values.break_skipped_count_sum,
            values.break_skipped_count_square_sum,
            values.run_observed_count,
        ),
        Metric::ShortOvertime => (
            values.short_break_overtime_seconds_sum,
            values.short_break_overtime_seconds_square_sum,
            values.run_observed_count,
        ),
        Metric::LongOvertime => (
            values.long_break_overtime_seconds_sum,
            values.long_break_overtime_seconds_square_sum,
            values.run_observed_count,
        ),
        Metric::DayMissed => (
            values.day_missed_planned_pomodoro_count_sum,
            values.day_missed_planned_pomodoro_count_square_sum,
            values.day_observed_count,
        ),
        Metric::DayBlocked => (
            values.day_blocked_attempt_count_sum,
            values.day_blocked_attempt_count_square_sum,
            values.day_observed_count,
        ),
    };
    stats::estimate_mean_with_variance(sum, squares, count, stats::DEFAULT_CONFIDENCE_Z)
}
fn mean_gain(
    c: &OutcomeValues,
    t: &OutcomeValues,
    metric: Metric,
    direction: ComparisonDirection,
    minimum: f64,
) -> bool {
    stats::compare_mean_estimates(estimate(c, metric), estimate(t, metric), direction, minimum)
        .meaningful
}
fn mean_harm(
    c: &OutcomeValues,
    t: &OutcomeValues,
    metric: Metric,
    direction: ComparisonDirection,
    minimum: f64,
) -> bool {
    stats::compare_mean_guardrail(
        estimate(c, metric),
        estimate(t, metric),
        direction,
        minimum,
        minimum * 2.0,
    )
    .breached
}
fn clean_focus_loss(c: &OutcomeValues, t: &OutcomeValues) -> bool {
    mean_harm(
        c,
        t,
        Metric::Clean,
        ComparisonDirection::HigherIsBetter,
        CLEAN_FOCUS_CHANGE_SECONDS,
    )
}
fn rate_harm(
    control_successes: f64,
    control_trials: f64,
    treatment_successes: f64,
    treatment_trials: f64,
    direction: ComparisonDirection,
    minimum: f64,
    severe: f64,
) -> bool {
    stats::compare_rate_guardrail(
        stats::estimate_rate(
            control_successes,
            control_trials,
            stats::DEFAULT_CONFIDENCE_Z,
        ),
        stats::estimate_rate(
            treatment_successes,
            treatment_trials,
            stats::DEFAULT_CONFIDENCE_Z,
        ),
        direction,
        minimum,
        severe,
    )
    .breached
}
fn ratio(numerator: f64, denominator: f64) -> f64 {
    stats::estimate_rate(numerator, denominator, stats::DEFAULT_CONFIDENCE_Z).point
}
fn mean(total: f64, count: f64) -> f64 {
    stats::estimate_mean(total, count).point
}
fn enough_day(value: &OutcomeValues) -> bool {
    value.day_observed_count >= MIN_DAY_OBSERVATIONS
}
fn enough_next_day(value: &OutcomeValues) -> bool {
    value.next_day_observed_count >= MIN_NEXT_DAY_OBSERVATIONS
}
