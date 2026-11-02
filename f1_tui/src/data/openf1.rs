//! Incremental historical acquisition. No UI, generated fallback or published cache.
use super::{
    catalogue::{Selection, now_ms},
    limits::{Cancellation, Limits},
    validation,
};
use crate::domain::UtcMillis;
use chrono::{DateTime, SecondsFormat};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    error::Error,
    io::Read,
    time::{Duration, Instant},
};

pub type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;
const ENDPOINT: &str = "https://api.openf1.org/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Window {
    start: UtcMillis,
    end: UtcMillis,
}
impl Window {
    pub fn new(start: UtcMillis, end: UtcMillis) -> Result<Self> {
        let span = end.0.checked_sub(start.0).ok_or("Invalid UTC window")?;
        if span <= 0
            || span > 24 * 60 * 60 * 1000
            || DateTime::from_timestamp_millis(start.0).is_none()
            || DateTime::from_timestamp_millis(end.0).is_none()
        {
            return Err("Acquisition window must be positive and at most 24 hours".into());
        }
        Ok(Self { start, end })
    }
    pub fn start(self) -> UtcMillis {
        self.start
    }
    pub fn end(self) -> UtcMillis {
        self.end
    }
}

#[derive(Debug, Clone)]
pub struct AcquisitionRequest {
    session_key: u32,
    meeting_key: u32,
    window: Window,
}
impl AcquisitionRequest {
    pub fn new(session_key: u32, meeting_key: u32, window: Window) -> Result<Self> {
        Window::new(window.start, window.end)?;
        if session_key == 0 || meeting_key == 0 {
            return Err("Exact nonzero session and meeting IDs are required".into());
        }
        Ok(Self {
            session_key,
            meeting_key,
            window,
        })
    }
    /// Catalogue schedules are not exact data bounds. Padding is explicit coverage,
    /// not a claim that every observation in the provider archive was acquired.
    pub fn from_selection(selection: &Selection<'_>) -> Result<Self> {
        if let Some(reason) = &selection.unavailable_reason {
            return Err(reason.clone().into());
        }
        let s = selection.session;
        if s.meeting_key != selection.meeting.meeting_key
            || s.year != selection.meeting.year
            || s.is_cancelled
            || selection.meeting.is_cancelled
        {
            return Err("Invalid catalogue selection".into());
        }
        let start =
            DateTime::parse_from_rfc3339(s.date_start.as_ref().ok_or("Missing session start")?)?
                .timestamp_millis();
        let end = DateTime::parse_from_rfc3339(s.date_end.as_ref().ok_or("Missing session end")?)?
            .timestamp_millis();
        if start >= end || end.saturating_add(30 * 60 * 1000) > current_time()?.0 {
            return Err("Session is not yet outside the historical access window".into());
        }
        Self::new(
            s.session_key,
            s.meeting_key,
            Window::new(UtcMillis(start - 3_600_000), UtcMillis(end + 3_600_000))?,
        )
    }
    pub fn window(&self) -> Window {
        self.window
    }
    pub fn session_key(&self) -> u32 {
        self.session_key
    }
    pub fn meeting_key(&self) -> u32 {
        self.meeting_key
    }
    pub fn with_window(&self, window: Window) -> Result<Self> {
        Self::new(self.session_key, self.meeting_key, window)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Channel {
    Drivers,
    Position,
    CarData,
    Location,
    Intervals,
    Laps,
    Stints,
    Pit,
    RaceControl,
    Weather,
}
impl Channel {
    pub const ALL: [Self; 10] = [
        Self::Drivers,
        Self::Position,
        Self::Laps,
        Self::Stints,
        Self::Pit,
        Self::RaceControl,
        Self::Weather,
        Self::CarData,
        Self::Location,
        Self::Intervals,
    ];
    pub fn endpoint(self) -> &'static str {
        match self {
            Self::Drivers => "drivers",
            Self::Position => "position",
            Self::CarData => "car_data",
            Self::Location => "location",
            Self::Intervals => "intervals",
            Self::Laps => "laps",
            Self::Stints => "stints",
            Self::Pit => "pit",
            Self::RaceControl => "race_control",
            Self::Weather => "weather",
        }
    }
    fn windowed(self) -> bool {
        matches!(self, Self::CarData | Self::Location | Self::Intervals)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldIssue {
    pub row: usize,
    pub field: String,
    pub reason: String,
}

#[derive(Debug)]
pub struct ValidatedChunk {
    channel: Channel,
    session_key: u32,
    meeting_key: u32,
    window: Option<Window>,
    source_url: String,
    retrieved_at: UtcMillis,
    status: u16,
    attempts: u32,
    raw: Vec<u8>,
    records: Vec<Value>,
    issues: Vec<FieldIssue>,
    empty_reason: Option<String>,
}
impl ValidatedChunk {
    pub fn channel(&self) -> Channel {
        self.channel
    }
    pub fn session_key(&self) -> u32 {
        self.session_key
    }
    pub fn meeting_key(&self) -> u32 {
        self.meeting_key
    }
    pub fn window(&self) -> Option<Window> {
        self.window
    }
    pub fn source_url(&self) -> &str {
        &self.source_url
    }
    pub fn retrieved_at(&self) -> UtcMillis {
        self.retrieved_at
    }
    pub fn status(&self) -> u16 {
        self.status
    }
    pub fn attempts(&self) -> u32 {
        self.attempts
    }
    pub fn raw_bytes(&self) -> &[u8] {
        &self.raw
    }
    /// Invalid numeric fields are null here, with issues; raw bytes stay untouched.
    pub fn records(&self) -> &[Value] {
        &self.records
    }
    pub fn issues(&self) -> &[FieldIssue] {
        &self.issues
    }
    pub fn empty_reason(&self) -> Option<&str> {
        self.empty_reason.as_deref()
    }
}

#[derive(Debug, Default)]
pub struct AcquisitionSummary {
    pub chunks: usize,
    pub records: usize,
    pub empty_chunks: usize,
    pub field_issues: usize,
}

pub struct OpenF1Client {
    endpoint: String,
    limits: Limits,
    agent: ureq::Agent,
    last_request: Option<Instant>,
}
impl OpenF1Client {
    pub fn new(limits: Limits) -> Result<Self> {
        Self::with_endpoint(ENDPOINT, limits)
    }
    /// The production endpoint or an IPv4 loopback HTTP stub; no arbitrary remote URL.
    pub fn with_endpoint(endpoint: &str, limits: Limits) -> Result<Self> {
        limits.validate()?;
        let local = endpoint
            .strip_prefix("http://127.0.0.1:")
            .and_then(|p| p.parse::<u16>().ok())
            .is_some_and(|p| p != 0);
        if endpoint != ENDPOINT && !local {
            return Err("Only OpenF1 HTTPS or a loopback test server is permitted".into());
        }
        let agent = ureq::AgentBuilder::new()
            .redirects(0)
            .timeout_connect(limits.connect_timeout)
            .timeout_read(limits.request_timeout)
            .timeout_write(limits.request_timeout)
            .timeout(limits.request_timeout)
            .build();
        Ok(Self {
            endpoint: endpoint.into(),
            limits,
            agent,
            last_request: None,
        })
    }

    pub fn acquire(
        &mut self,
        request: &AcquisitionRequest,
        cancel: &Cancellation,
        sink: impl FnMut(ValidatedChunk) -> Result<()>,
    ) -> Result<AcquisitionSummary> {
        self.acquire_channels(request, &Channel::ALL, cancel, sink)
    }

    pub fn acquire_channels(
        &mut self,
        request: &AcquisitionRequest,
        channels: &[Channel],
        cancel: &Cancellation,
        mut sink: impl FnMut(ValidatedChunk) -> Result<()>,
    ) -> Result<AcquisitionSummary> {
        if channels.first() != Some(&Channel::Drivers)
            || channels.iter().collect::<BTreeSet<_>>().len() != channels.len()
        {
            return Err("Acquisition needs drivers first and unique channels".into());
        }
        let mut drivers = BTreeSet::new();
        let mut summary = AcquisitionSummary::default();
        for &channel in channels {
            let mut cursor = request.window.start.0;
            loop {
                cancel.check()?;
                let window = if channel.windowed() {
                    Some(Window::new(
                        UtcMillis(cursor),
                        UtcMillis(
                            cursor
                                .saturating_add(self.limits.chunk_span.as_millis() as i64)
                                .min(request.window.end.0),
                        ),
                    )?)
                } else {
                    None
                };
                let chunk = self.chunk(request, channel, window, &drivers, cancel)?;
                if channel == Channel::Drivers {
                    for row in chunk.records() {
                        let number =
                            row["driver_number"].as_u64().expect("validated driver") as u32;
                        if !drivers.insert(number) {
                            return Err("Duplicate driver in roster".into());
                        }
                    }
                    if drivers.is_empty() {
                        return Err(
                            "Driver roster is unavailable; cannot validate session data".into()
                        );
                    }
                }
                summary.chunks += 1;
                summary.records += chunk.records.len();
                summary.field_issues += chunk.issues.len();
                if chunk.records.is_empty() {
                    summary.empty_chunks += 1;
                }
                cancel.check()?;
                sink(chunk)?;
                cancel.check()?;
                match window {
                    Some(w) if w.end.0 < request.window.end.0 => cursor = w.end.0,
                    _ => break,
                }
            }
        }
        Ok(summary)
    }

    fn chunk(
        &mut self,
        request: &AcquisitionRequest,
        channel: Channel,
        window: Option<Window>,
        drivers: &BTreeSet<u32>,
        cancel: &Cancellation,
    ) -> Result<ValidatedChunk> {
        let mut url = format!(
            "{}/{}?session_key={}",
            self.endpoint,
            channel.endpoint(),
            request.session_key
        );
        if let Some(w) = window {
            // Operators are written as documented, not passed as equality-filter keys.
            url.push_str(&format!(
                "&date>={}&date<{}",
                utc_query(w.start)?,
                utc_query(w.end)?
            ));
        }
        for attempt in 0..=self.limits.max_retries {
            cancel.check()?;
            if let Some(last) = self.last_request {
                cancel.wait(self.limits.request_spacing.saturating_sub(last.elapsed()))?;
            }
            cancel.check()?;
            self.last_request = Some(Instant::now());
            let response = match self.agent.get(&url).call() {
                Ok(response) => response,
                Err(ureq::Error::Status(404, response)) => response,
                Err(ureq::Error::Status(code, response))
                    if code == 429 || matches!(code, 500 | 502 | 503 | 504) =>
                {
                    self.retry(attempt, response.header("Retry-After"), cancel)
                        .map_err(|e| format!("{} HTTP {code}: {e}", channel.endpoint()))?;
                    continue;
                }
                Err(ureq::Error::Status(code, _)) => {
                    return Err(
                        format!("{} HTTP {code}; no data accepted", channel.endpoint()).into(),
                    );
                }
                Err(ureq::Error::Transport(error)) => {
                    self.retry(attempt, None, cancel).map_err(|e| {
                        format!("{} transport error ({error}): {e}", channel.endpoint())
                    })?;
                    continue;
                }
            };
            let status = response.status();
            if status != 200 && status != 404 {
                return Err(format!("Unexpected HTTP {status}; no data accepted").into());
            }
            let mut raw = Vec::new();
            let mut buffer = [0; 8192];
            let mut reader = response.into_reader();
            let mut read_failed = false;
            loop {
                cancel.check()?;
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => {
                        if raw.len() + n > self.limits.max_response_bytes {
                            return Err("Response byte limit exceeded; use smaller chunks".into());
                        }
                        raw.extend_from_slice(&buffer[..n]);
                    }
                    Err(_) => {
                        read_failed = true;
                        break;
                    }
                }
            }
            cancel.check()?;
            if read_failed {
                self.retry(attempt, None, cancel)?;
                continue;
            }
            let mut records: Vec<Value> = if status == 404 {
                vec![]
            } else {
                serde_json::from_slice(&raw)?
            };
            if records.len() > self.limits.max_rows {
                return Err("Response row limit exceeded; use smaller chunks".into());
            }
            let issues = validation::validate(channel, &mut records, request, window, drivers)
                .map_err(|e| format!("{} chunk validation failed: {e}", channel.endpoint()))?;
            let empty_reason = if status == 404 {
                Some("HTTP 404: no records for this query; endpoint support is not inferred".into())
            } else if records.is_empty() {
                Some("Provider returned an empty array for this query".into())
            } else {
                None
            };
            return Ok(ValidatedChunk {
                channel,
                session_key: request.session_key,
                meeting_key: request.meeting_key,
                window,
                source_url: url,
                retrieved_at: current_time()?,
                status,
                attempts: attempt + 1,
                raw,
                records,
                issues,
                empty_reason,
            });
        }
        unreachable!("retry returns an error at the budget")
    }

    fn retry(&self, attempt: u32, after: Option<&str>, cancel: &Cancellation) -> Result<()> {
        if attempt >= self.limits.max_retries {
            return Err(
                "Acquisition retry budget exhausted; incomplete chunk was not accepted".into(),
            );
        }
        let backoff = self
            .limits
            .retry_backoff
            .saturating_mul(1 << attempt)
            .min(self.limits.max_retry_wait);
        let wait = if let Some(after) = after {
            let delay = retry_after(after)?;
            if delay > self.limits.max_retry_wait {
                return Err("Retry-After exceeds automatic wait budget; retry later without shortening the provider delay".into());
            }
            delay.max(backoff)
        } else {
            backoff
        };
        cancel.wait(wait)?;
        Ok(())
    }
}

fn current_time() -> Result<UtcMillis> {
    Ok(UtcMillis(now_ms().map_err(|e| e.to_string())?))
}
fn utc_query(time: UtcMillis) -> Result<String> {
    Ok(DateTime::from_timestamp_millis(time.0)
        .ok_or("Invalid UTC timestamp")?
        .to_rfc3339_opts(SecondsFormat::Millis, true)
        .trim_end_matches('Z')
        .to_owned())
}
pub(crate) fn retry_after(text: &str) -> Result<Duration> {
    if let Ok(seconds) = text.trim().parse::<u64>() {
        return Ok(Duration::from_secs(seconds));
    }
    let at = DateTime::parse_from_rfc2822(text)?.timestamp_millis();
    Ok(Duration::from_millis(
        at.saturating_sub(current_time()?.0).max(0) as u64,
    ))
}
