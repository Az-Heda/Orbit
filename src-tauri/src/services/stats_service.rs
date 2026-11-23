use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use chrono_tz::Tz;

use super::timer_service::Clock;
use crate::domain::stats::{Stats, build_stats};
use crate::domain::validation::{fmt_utc, invalid};
use crate::error::DomainResult;
use crate::infrastructure::database::StatsRepository;

/// Read-only statistics. Rows come from SQL; days are bucketed in the user's IANA timezone
/// (SQLite cannot do that), so a session at 00:30 local counts for the new local day.
pub struct StatsService {
    repo: Arc<dyn StatsRepository>,
    clock: Clock,
}

impl StatsService {
    pub fn new(repo: Arc<dyn StatsRepository>) -> Self {
        Self::with_clock(repo, Arc::new(Utc::now))
    }

    pub fn with_clock(repo: Arc<dyn StatsRepository>, clock: Clock) -> Self {
        Self { repo, clock }
    }

    /// Last `days` local days (1..=366) ending today.
    pub async fn overview(&self, timezone: &str, days: i64) -> DomainResult<Stats> {
        if !(1..=366).contains(&days) {
            return Err(invalid("days must be between 1 and 366"));
        }
        let tz: Tz = timezone.parse().map_err(|_| invalid("unknown timezone"))?;
        let now: DateTime<Utc> = (self.clock)();
        let today = now.with_timezone(&tz).date_naive();
        // Widen the UTC window by a day each side; bucketing drops anything outside the range.
        let first = today - Duration::days(days - 1);
        let from = first.and_hms_opt(0, 0, 0).unwrap().and_utc() - Duration::days(1);
        let to = today.and_hms_opt(0, 0, 0).unwrap().and_utc() + Duration::days(2);
        let (from, to) = (fmt_utc(from), fmt_utc(to));
        let focus = self.repo.focus_rows(&from, &to).await?;
        let done = self.repo.completions(&from, &to).await?;
        Ok(build_stats(tz, today, days, &focus, &done))
    }
}
