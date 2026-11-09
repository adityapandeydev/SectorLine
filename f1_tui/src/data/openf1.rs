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
pub(crate) const ARCHIVE_ROW_LIMIT: usize = 100_000;
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
    pub(crate) fn windowed(self) -> bool {
        matches!(self, Self::CarData | Self::Location | Self::Intervals)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldIssue {
    pub row: usize,
    pub field: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChunkMetadata {
    pub channel: Channel,
    pub session_key: u32,
    pub meeting_key: u32,
    pub window: Option<Window>,
    pub source_url: String,
    #[serde(alias = "retrieved_at_utc_ms")]
    pub retrieved_at: UtcMillis,
    pub status: u16,
    pub attempts: u32,
}

/// Restore returns previously verified data; accept journals a newly downloaded chunk.
pub trait ChunkStore {
    fn prepare(
        &self,
        _request: &AcquisitionRequest,
        _channels: &[Channel],
        _chunk_span: Duration,
    ) -> Result<()> {
        Ok(())
    }
    fn restore(
        &mut self,
        channel: Channel,
        window: Option<Window>,
    ) -> Result<Option<ValidatedChunk>>;
    fn accept(&mut self, chunk: ValidatedChunk) -> Result<()>;
}

struct CallbackStore<F>(F);
impl<F: FnMut(ValidatedChunk) -> Result<()>> ChunkStore for CallbackStore<F> {
    fn restore(&mut self, _: Channel, _: Option<Window>) -> Result<Option<ValidatedChunk>> {
        Ok(None)
    }
    fn accept(&mut self, chunk: ValidatedChunk) -> Result<()> {
        (self.0)(chunk)
    }
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
    pub fn metadata(&self) -> ChunkMetadata {
        ChunkMetadata {
            channel: self.channel,
            session_key: self.session_key,
            meeting_key: self.meeting_key,
            window: self.window,
            source_url: self.source_url.clone(),
            retrieved_at: self.retrieved_at,
            status: self.status,
            attempts: self.attempts,
        }
    }
    pub(crate) fn from_archive(
        meta: ChunkMetadata,
        raw: Vec<u8>,
        request: &AcquisitionRequest,
        drivers: &BTreeSet<u32>,
    ) -> Result<Self> {
        if meta.session_key != request.session_key
            || meta.meeting_key != request.meeting_key
            || !matches!(meta.status, 200 | 404)
            || meta.attempts == 0
            || meta.attempts > 9
            || meta.retrieved_at.0 < 0
        {
            return Err("Invalid archived chunk identity or metadata".into());
        }
        let expected = source_url(
            "https://api.openf1.org/v1",
            request,
            meta.channel,
            meta.window,
        )?;
        // Local stub provenance is useful for tests, but never rewrite it to OpenF1.
        let path = meta
            .source_url
            .strip_prefix("http://127.0.0.1:")
            .and_then(|s| s.split_once('/'))
            .filter(|(port, _)| port.parse::<u16>().is_ok())
            .map(|(_, path)| path);
        if meta.source_url != expected
            && path != expected.strip_prefix("https://api.openf1.org/v1/")
        {
            return Err("Archived source URL does not match chunk query".into());
        }
        if raw.len() > 64 * 1024 * 1024 {
            return Err("Archived chunk exceeds byte limit".into());
        }
        let mut records: Vec<Value> = if meta.status == 404 {
            vec![]
        } else {
            serde_json::from_slice(&raw)?
        };
        if records.len() > ARCHIVE_ROW_LIMIT {
            return Err("Archived chunk exceeds size/row limit".into());
        }
        let issues =
            validation::validate(meta.channel, &mut records, request, meta.window, drivers)?;
        let empty_reason = if meta.status == 404 {
            Some("HTTP 404: no records for this query; endpoint support is not inferred".into())
        } else if records.is_empty() {
            Some("Provider returned an empty array for this query".into())
        } else {
            None
        };
        Ok(Self {
            channel: meta.channel,
            session_key: meta.session_key,
            meeting_key: meta.meeting_key,
            window: meta.window,
            source_url: meta.source_url,
            retrieved_at: meta.retrieved_at,
            status: meta.status,
            attempts: meta.attempts,
            raw,
            records,
            issues,
            empty_reason,
        })
    }
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
        sink: impl FnMut(ValidatedChunk) -> Result<()>,
    ) -> Result<AcquisitionSummary> {
        self.acquire_with_store(request, channels, cancel, &mut CallbackStore(sink))
    }

    pub fn acquire_with_store(
        &mut self,
        request: &AcquisitionRequest,
        channels: &[Channel],
        cancel: &Cancellation,
        store: &mut impl ChunkStore,
    ) -> Result<AcquisitionSummary> {
        store.prepare(request, channels, self.limits.chunk_span)?;
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
                let restored = store.restore(channel, window)?;
                let reused = restored.is_some();
                let chunk = if let Some(chunk) = restored {
                    if chunk.channel != channel || chunk.window != window {
                        return Err("Restored chunk does not match request".into());
                    }
                    ValidatedChunk::from_archive(chunk.metadata(), chunk.raw, request, &drivers)?
                } else {
                    self.chunk(request, channel, window, &drivers, cancel)?
                };
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
                if !reused {
                    store.accept(chunk)?;
                }
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
        let url = source_url(&self.endpoint, request, channel, window)?;
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
            let mut read_error = None;
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
                    Err(error) => {
                        read_error = Some(error.kind());
                        break;
                    }
                }
            }
            cancel.check()?;
            if let Some(kind) = read_error {
                self.retry(attempt, None, cancel).map_err(|e| {
                    format!(
                        "{} response read interrupted ({kind:?}): {e}",
                        channel.endpoint()
                    )
                })?;
                continue;
            }
            let mut records: Vec<Value> = if status == 404 {
                vec![]
            } else {
                serde_json::from_slice(&raw)
                    .map_err(|e| format!("{} JSON decoding failed: {e}", channel.endpoint()))?
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
fn source_url(
    endpoint: &str,
    request: &AcquisitionRequest,
    channel: Channel,
    window: Option<Window>,
) -> Result<String> {
    let mut url = format!(
        "{endpoint}/{}?session_key={}",
        channel.endpoint(),
        request.session_key
    );
    if let Some(w) = window {
        url.push_str(&format!(
            "&date>={}&date<{}",
            utc_query(w.start)?,
            utc_query(w.end)?
        ));
    }
    Ok(url)
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
