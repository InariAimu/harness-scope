//! Query-time speed policy. Raw local timings and billing facts remain unchanged.
use crate::model::UsageTiming;
use serde::Serialize;

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

pub(crate) fn speed_estimate(harness: &str, timing: &UsageTiming, output: u64) -> SpeedEstimate {
    // Only DSH currently records a first streamed token for the same request.
    // Item creation (Codex/OpenCode) can precede or follow actual token delivery.
    let decode = harness == "dsh"
        && timing.duration_source == "dsh_step_message"
        && timing.first_token_source.as_deref() == Some("dsh_stream_delta");
    let (basis, duration, min_output_tokens, min_duration_ms) = if decode {
        (
            SpeedBasis::Decode,
            timing
                .first_token_ms
                .filter(|first| *first >= 0)
                .and_then(|first| timing.duration_ms.checked_sub(first)),
            100,
            100,
        )
    } else {
        (SpeedBasis::EndToEnd, Some(timing.duration_ms), 200, 1000)
    };
    let duration_ms = duration.filter(|ms| *ms > 0);
    SpeedEstimate {
        basis,
        duration_ms,
        eligible: (100..=3_600_000).contains(&timing.duration_ms)
            && output >= min_output_tokens
            && duration_ms.is_some_and(|ms| ms >= min_duration_ms),
        min_output_tokens,
        min_duration_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stream_provenance_and_sensible_windows_are_required_for_decode_speed() {
        let mut timing = UsageTiming {
            duration_ms: 6846,
            first_token_ms: Some(4364),
            duration_source: "dsh_step_message".into(),
            first_token_source: Some("dsh_stream_delta".into()),
        };
        let speed = speed_estimate("dsh", &timing, 488);
        assert!(speed.basis == SpeedBasis::Decode && speed.eligible);
        assert_eq!(speed.duration_ms, Some(2482));
        for first in [None, Some(-1), Some(6846), Some(7000), Some(6800)] {
            timing.first_token_ms = first;
            let speed = speed_estimate("dsh", &timing, 488);
            assert!(speed.basis == SpeedBasis::Decode && !speed.eligible);
        }
        timing.first_token_ms = Some(4364);
        for harness in ["codex", "opencode", "claude"] {
            let speed = speed_estimate(harness, &timing, 488);
            assert!(speed.basis == SpeedBasis::EndToEnd && speed.eligible);
            assert_eq!(speed.duration_ms, Some(6846));
        }
        timing.first_token_source = None;
        assert!(speed_estimate("dsh", &timing, 488).basis == SpeedBasis::EndToEnd);
        timing.duration_ms = 3_600_001;
        assert!(!speed_estimate("dsh", &timing, 488).eligible);
    }
}
