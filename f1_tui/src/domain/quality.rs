//! Explicit conservative UI freshness policy, not a provider delivery guarantee.
use super::*;
pub fn stale_after(channel: &str) -> Option<u64> {
    match channel {
        "car_data" | "location" => Some(2000),
        "intervals" => Some(10_000),
        "weather" => Some(120_000),
        "race_control" => Some(120_000),
        "stints" => Some(180_000),
        _ => None,
    }
}
pub fn at_replay_time<T: Clone>(
    value: &Observation<T>,
    now: UtcMillis,
    channel: &str,
) -> Result<Observation<T>, ContractError> {
    if value.value().is_some()
        && stale_after(channel)
            .zip(value.timing().observed_at)
            .is_some_and(|(budget, t)| now.0.saturating_sub(t.0) > budget as i64)
    {
        Observation::new(
            value.value().cloned(),
            value.source().clone(),
            value.timing().clone(),
            Quality::Stale,
            Some(format!(
                "Last {channel} evidence exceeds replay freshness policy; not current confirmation"
            )),
        )
    } else {
        Ok(value.clone())
    }
}
