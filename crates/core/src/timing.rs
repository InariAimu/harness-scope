//! Query-time speed policy. Raw local timings and billing facts remain unchanged.
use crate::model::UsageTiming;
use serde::Serialize;

// Session logs measure client-side delivery. Subsecond bursts can include output
// buffered upstream, so they cannot reliably estimate model generation speed.
const MIN_ESTIMATED_OUTPUT_TOKENS: u64 = 200;
const MIN_ESTIMATED_DURATION_MS: i64 = 1000;

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SpeedBasis {
    Decode,
    EndToEnd,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SpeedEstimate {
    pub basis: SpeedBasis,
    pub duration_ms: Option<i64>,
    pub eligible: bool,
    pub min_output_tokens: u64,
    pub min_duration_ms: i64,
}

pub(crate) fn speed_estimate(_harness: &str, timing: &UsageTiming, output: u64) -> SpeedEstimate {
    // First-token timestamps vary by source and remain estimates. Use E2E
    // only when no first-token time was recorded.
    let (basis, duration_ms) = match timing.first_token_ms {
        Some(first) => (
            SpeedBasis::Decode,
            (first >= 0)
                .then(|| timing.duration_ms.checked_sub(first))
                .flatten()
                .filter(|ms| *ms > 0),
        ),
        None => (
            SpeedBasis::EndToEnd,
            Some(timing.duration_ms).filter(|ms| *ms > 0),
        ),
    };
    SpeedEstimate {
        basis,
        duration_ms,
        eligible: (100..=3_600_000).contains(&timing.duration_ms)
            && output >= MIN_ESTIMATED_OUTPUT_TOKENS
            && duration_ms.is_some_and(|ms| ms >= MIN_ESTIMATED_DURATION_MS),
        min_output_tokens: MIN_ESTIMATED_OUTPUT_TOKENS,
        min_duration_ms: MIN_ESTIMATED_DURATION_MS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_token_and_sensible_windows_are_required_for_decode_speed() {
        let mut timing = UsageTiming {
            duration_ms: 6846,
            first_token_ms: Some(4364),
            duration_source: "dsh_step_message".into(),
            first_token_source: Some("dsh_stream_delta".into()),
        };
        let speed = speed_estimate("dsh", &timing, 488);
        assert!(speed.basis == SpeedBasis::Decode && speed.eligible);
        assert_eq!(speed.duration_ms, Some(2482));
        for first in [Some(-1), Some(6846), Some(7000), Some(6800)] {
            timing.first_token_ms = first;
            let speed = speed_estimate("dsh", &timing, 488);
            assert!(speed.basis == SpeedBasis::Decode && !speed.eligible);
        }
        timing.first_token_ms = Some(4364);
        for harness in ["codex", "opencode", "claude"] {
            let speed = speed_estimate(harness, &timing, 488);
            assert!(speed.basis == SpeedBasis::Decode && speed.eligible);
            assert_eq!(speed.duration_ms, Some(2482));
        }
        timing.first_token_source = None;
        timing.first_token_ms = None;
        let fallback = speed_estimate("dsh", &timing, 488);
        assert!(fallback.basis == SpeedBasis::EndToEnd && fallback.eligible);
        assert_eq!(fallback.duration_ms, Some(6846));
        timing.duration_ms = 3_600_001;
        assert!(!speed_estimate("dsh", &timing, 488).eligible);
    }

    #[test]
    fn short_or_buffered_output_is_excluded_without_falling_back_to_e2e() {
        for (output, duration, first, eligible) in [
            (124, 6491, 6331, false),  // Observed 775 tok/s: 124 / 0.160.
            (1311, 8836, 8058, false), // More output still arrives in a subsecond burst.
            (199, 5000, 1000, false),
            (200, 5000, 4001, false),
            (200, 5000, 4000, true),
            (2000, 5000, 3000, true), // No arbitrary upper limit on measured speed.
        ] {
            let timing = UsageTiming {
                duration_ms: duration,
                first_token_ms: Some(first),
                duration_source: "dsh_step_message".into(),
                first_token_source: Some("dsh_stream_delta".into()),
            };
            let speed = speed_estimate("dsh", &timing, output);
            assert!(speed.basis == SpeedBasis::Decode);
            assert_eq!(speed.duration_ms, Some(duration - first));
            assert_eq!(
                speed.eligible,
                eligible,
                "{output} tokens / {} ms",
                duration - first
            );
            assert_eq!(speed.min_output_tokens, 200);
            assert_eq!(speed.min_duration_ms, 1000);
        }
    }
}
