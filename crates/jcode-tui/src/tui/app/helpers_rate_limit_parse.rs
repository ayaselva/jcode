//! Rate-limit error parsing (reset/retry timing) for TUI auto-retry logic.
use std::time::Duration;

use super::parse_clock_time_to_duration;

/// Wait to honor when a transient in-flight budget rejection carries no usable
/// `Retry-After`; OpenRouter's own hint for that state is two minutes.
const IN_FLIGHT_BUDGET_DEFAULT_WAIT: Duration = Duration::from_secs(120);

/// Seconds from an explicit `Retry-After`, in either its header spelling
/// (`retry-after: 120`) or the JSON one OpenRouter echoes in the error body
/// (`"retry-after":"120"`).
///
/// Every occurrence is tried because the first mention is often prose ("see the
/// Retry-After header") rather than the value itself.
fn parse_retry_after_seconds(error_lower: &str) -> Option<u64> {
    error_lower
        .match_indices("retry-after")
        .find_map(|(idx, marker)| {
            let rest = error_lower[idx + marker.len()..]
                .trim_start_matches([':', '"', '\'', '=', ' ', '\t']);
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            match digits.parse::<u64>() {
                Ok(secs) if secs > 0 && secs < 86400 => Some(secs),
                _ => None,
            }
        })
}

/// Parse rate limit reset time from error message
/// Returns the Duration until rate limit resets, if this is a rate limit error
pub(crate) fn parse_rate_limit_error(error: &str) -> Option<Duration> {
    let error_lower = error.to_lowercase();

    // A transient in-flight budget cap is a throttle, not a spent balance: the
    // provider rejects with `402` and no rate-limit wording at all, but asks to
    // resend after its `Retry-After`. Treat it as a rate limit so the existing
    // wait-and-resume machinery picks the turn back up by itself.
    if jcode_provider_core::is_transient_in_flight_budget_error(&error_lower) {
        return Some(
            parse_retry_after_seconds(&error_lower)
                .map_or(IN_FLIGHT_BUDGET_DEFAULT_WAIT, Duration::from_secs),
        );
    }

    if !error_lower.contains("rate limit")
        && !error_lower.contains("rate_limit")
        && !error_lower.contains("429")
        && !error_lower.contains("too many requests")
        && !error_lower.contains("hit your limit")
    {
        return None;
    }

    if let Some(idx) = error_lower.find("retry") {
        let after = &error_lower[idx..];
        for word in after.split_whitespace() {
            if let Ok(secs) = word
                .trim_matches(|c: char| !c.is_ascii_digit())
                .parse::<u64>()
                && secs > 0
                && secs < 86400
            {
                return Some(Duration::from_secs(secs));
            }
        }
    }

    if let Some(idx) = error_lower.find("resets") {
        let after = &error_lower[idx..];
        for word in after.split_whitespace() {
            let word = word.trim_matches(|c: char| c == '·' || c == ' ');
            if (word.ends_with("am") || word.ends_with("pm"))
                && let Some(duration) = parse_clock_time_to_duration(word)
            {
                return Some(duration);
            }
        }
    }

    if let Some(idx) = error_lower.find("reset") {
        let after = &error_lower[idx..];
        // Unit-suffixed durations like "resets in 30d 4h 29m" (OpenAI usage
        // limit messages). Without this, "30d" would parse as 30 seconds and
        // schedule a bogus 30s auto-retry against a limit that resets in days.
        let mut unit_total = Duration::ZERO;
        let mut saw_unit = false;
        for word in after.split_whitespace().take(8) {
            let digits: String = word.chars().take_while(|c| c.is_ascii_digit()).collect();
            let rest = &word[digits.len()..];
            if digits.is_empty() {
                continue;
            }
            let value: u64 = match digits.parse() {
                Ok(v) => v,
                Err(_) => continue,
            };
            let secs = match rest.trim_matches(|c: char| !c.is_ascii_alphabetic()) {
                "d" => Some(value * 86400),
                "h" => Some(value * 3600),
                "m" | "min" => Some(value * 60),
                "s" | "sec" => Some(value),
                _ => None,
            };
            if let Some(secs) = secs {
                unit_total += Duration::from_secs(secs);
                saw_unit = true;
            }
        }
        if saw_unit {
            // Only auto-retry within a day; longer windows should be treated
            // as terminal by the caller (fallback offer / stop auto-poke).
            if unit_total > Duration::ZERO && unit_total < Duration::from_secs(86400) {
                return Some(unit_total);
            }
            return None;
        }
        for word in after.split_whitespace() {
            if let Ok(secs) = word
                .trim_matches(|c: char| !c.is_ascii_digit())
                .parse::<u64>()
                && secs > 0
                && secs < 86400
            {
                return Some(Duration::from_secs(secs));
            }
        }
    }

    None
}

#[cfg(test)]
#[cfg(test)]
mod rate_limit_parse_tests {
    use super::parse_rate_limit_error;
    use std::time::Duration;

    #[test]
    fn usage_limit_reset_in_days_does_not_schedule_bogus_short_retry() {
        // "30d" must not be misread as 30 seconds.
        let err = "Rate limited: The usage limit has been reached. Plan: team. \
                   Resets in 30d 4h 29m (2026-08-21 04:31 UTC).";
        assert_eq!(parse_rate_limit_error(err), None);
    }

    #[test]
    fn unit_suffixed_reset_within_a_day_is_parsed() {
        let err = "429 rate limit exceeded. Resets in 2h 5m.";
        assert_eq!(
            parse_rate_limit_error(err),
            Some(Duration::from_secs(2 * 3600 + 5 * 60))
        );
    }

    #[test]
    fn in_flight_budget_402_waits_for_the_servers_retry_after() {
        let err = "OpenAI-compatible chat request failed\n  status: 402 Payment Required\n  \
            response: {\"error\":{\"message\":\"This request would exceed your available credits \
            given your current in-flight requests. Retry after in-flight requests settle, or add \
            credits.\",\"code\":402,\"metadata\":{\"reason\":\"in_flight_budget_exhausted\",\
            \"remedy_hint\":\"Retry after your in-flight requests settle (see the Retry-After \
            header).\",\"headers\":{\"Retry-After\":\"120\"}}}}";
        assert_eq!(parse_rate_limit_error(err), Some(Duration::from_secs(120)));
    }

    #[test]
    fn in_flight_budget_402_without_a_hint_falls_back_to_the_default_wait() {
        let err = "status: 402 payment required {\"metadata\":{\"reason\":\
            \"in_flight_budget_exhausted\"}}";
        assert_eq!(parse_rate_limit_error(err), Some(Duration::from_secs(120)));
    }

    #[test]
    fn a_spent_balance_402_is_not_treated_as_a_wait() {
        let err = "status: 402 payment required: this request requires more credits";
        assert_eq!(parse_rate_limit_error(err), None);
    }

    #[test]
    fn plain_retry_seconds_still_parse() {
        let err = "429 Too Many Requests: retry after 30 seconds";
        assert_eq!(parse_rate_limit_error(err), Some(Duration::from_secs(30)));
    }
}
