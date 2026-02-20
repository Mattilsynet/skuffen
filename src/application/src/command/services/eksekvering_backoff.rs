use chrono::{DateTime, Duration, Utc};

pub fn neste_backoff(attempt: u32) -> DateTime<Utc> {
    let delay = if attempt == 0 {
        Duration::minutes(1)
    } else if attempt == 1 {
        Duration::minutes(5)
    } else if attempt == 2 {
        Duration::minutes(15)
    } else if attempt == 3 {
        Duration::hours(1)
    } else if attempt == 4 {
        Duration::hours(6)
    } else if attempt == 5 {
        Duration::hours(12)
    } else {
        Duration::hours(24)
    };
    Utc::now() + delay
}
