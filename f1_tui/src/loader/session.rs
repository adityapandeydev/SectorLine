//! Real session preparation and catalogue search, executed only on the loader worker.
use super::Loader;
use crate::replay::RecordedReplay;
use crate::{
    app::SessionOption,
    data::{
        cache::{CacheSpec, CacheWriter, SessionCache},
        catalogue::{Catalogue, Query, now_ms},
        limits::{Cancellation, Limits},
        openf1::{
            AcquisitionRequest, Channel, ChunkStore, OpenF1Client, Result, ValidatedChunk, Window,
        },
    },
    stream::TelemetryStream,
};
use std::{path::PathBuf, time::Duration};

#[derive(Clone)]
pub enum SessionRequest {
    Demo,
    Search(String),
    Recording(u32),
}
pub enum SessionResult {
    Demo(Box<TelemetryStream>),
    Search(Vec<SessionOption>),
    Recording(Box<RecordedReplay>),
}
pub struct WorkerConfig {
    pub demo_dir: PathBuf,
    pub catalogue_path: PathBuf,
    pub recording_root: PathBuf,
    pub limits: Limits,
    pub endpoint: String,
    pub job_timeout: Duration,
}
pub fn session_loader(config: WorkerConfig) -> Loader<SessionRequest, SessionResult> {
    let mut client = None;
    Loader::new(config.job_timeout, move |request, cancel, progress| {
        prepare(&config, request, &cancel, progress, &mut client)
            .map_err(|e| format!("{e}. Retry with r or choose another session with s"))
    })
}
fn prepare(
    config: &WorkerConfig,
    request: SessionRequest,
    cancel: &Cancellation,
    progress: &dyn Fn(String),
    client: &mut Option<OpenF1Client>,
) -> Result<SessionResult> {
    cancel.check()?;
    if matches!(request, SessionRequest::Demo) {
        progress("Opening cached generated demo".into());
        let stream = TelemetryStream::load_from_dir(&config.demo_dir).map_err(|e| e.to_string())?;
        cancel.check()?;
        return Ok(SessionResult::Demo(Box::new(stream)));
    }
    progress("Reading offline session catalogue".into());
    let catalogue = Catalogue::load(&config.catalogue_path)
        .map_err(|e| format!("{e}. Fetch catalogue with --catalogue --refresh --year YYYY"))?;
    cancel.check()?;
    let now = now_ms().map_err(|e| e.to_string())?;
    if let SessionRequest::Search(text) = request {
        let years = text
            .split_whitespace()
            .filter_map(|s| s.parse::<u16>().ok())
            .filter(|y| *y >= 1950)
            .collect::<Vec<_>>();
        if years.len() > 1 {
            return Err("Use one explicit year per search".into());
        }
        let matches = catalogue
            .search(
                &Query {
                    year: years.first().copied(),
                    text,
                },
                now,
            )
            .map_err(|e| e.to_string())?;
        if matches.is_empty() {
            return Err("No matching cached sessions; refresh that year's catalogue with --catalogue --refresh --year YYYY".into());
        }
        let options = matches
            .into_iter()
            .map(|s| SessionOption {
                title: format!(
                    "#{} {} {} | {}",
                    s.session.session_key,
                    s.session.year,
                    s.session.session_name,
                    s.meeting.meeting_name
                ),
                gp_key: s.meeting.circuit_short_name.clone(),
                year: s.session.year as u32,
                session_type: s.session.session_type.clone(),
                badge: "RECORDING".into(),
                subtitle: s
                    .unavailable_reason
                    .clone()
                    .unwrap_or_else(|| "Open recorded timing; event-time estimates".into()),
                recording_key: Some(s.session.session_key),
                unavailable_reason: s.unavailable_reason,
            })
            .collect();
        return Ok(SessionResult::Search(options));
    }
    let SessionRequest::Recording(key) = request else {
        unreachable!()
    };
    let selection = catalogue.select(key, now).map_err(|e| e.to_string())?;
    let request = AcquisitionRequest::from_selection(&selection)?;
    let spec = CacheSpec::new(
        key,
        selection.meeting.meeting_key,
        selection.session.year,
        (
            &selection.session.session_name,
            &selection.session.session_type,
        ),
        request.window(),
        config.limits.chunk_span,
        Channel::ALL.to_vec(),
    )?;
    let verifying = |n, total| progress(format!("Verifying recording: {n}/{total} chunks"));
    let title = format!(
        "{} {} | {} | #{}",
        selection.session.year, selection.meeting.meeting_name, selection.session.session_name, key
    );
    let mut context = crate::domain::capabilities::SessionContext::from_names(
        selection.session.id(),
        &selection.session.session_name,
        &selection.session.session_type,
    );
    if let (Some(start), Some(end)) = (&selection.session.date_start, &selection.session.date_end) {
        context = context.with_schedule(
            crate::domain::normalize::utc_millis_ceiling(start)?,
            crate::domain::normalize::utc_millis_ceiling(end)?,
        )?;
    }
    if SessionCache::exists(&config.recording_root, &spec) {
        let cache =
            SessionCache::open_cancellable(&config.recording_root, &spec, cancel, &verifying)?;
        return normalized(&cache, title, context, cancel, progress);
    }
    progress("Opening resumable download; the previous session stays usable".into());
    let mut writer =
        CacheWriter::begin_cancellable(&config.recording_root, &spec, cancel, &verifying)?;
    if client.is_none() {
        *client = Some(OpenF1Client::with_endpoint(
            &config.endpoint,
            config.limits.clone(),
        )?);
    }
    client
        .as_mut()
        .expect("client initialized")
        .acquire_with_store(
            &request,
            spec.channels(),
            cancel,
            &mut ReportingStore {
                writer: &mut writer,
                progress,
            },
        )?;
    cancel.check()?;
    let cache = writer.finish_cancellable(cancel, &verifying)?;
    normalized(&cache, title, context, cancel, progress)
}
fn normalized(
    cache: &SessionCache,
    title: String,
    context: crate::domain::capabilities::SessionContext,
    cancel: &Cancellation,
    progress: &dyn Fn(String),
) -> Result<SessionResult> {
    progress("Normalizing recorded observations and ordering timeline".into());
    let replay = RecordedReplay::from_cache(cache, title, cancel, &|n, total| {
        progress(format!("Normalizing recorded data: {n}/{total} chunks"))
    })?
    .with_context(context)?;
    cancel.check()?;
    Ok(SessionResult::Recording(Box::new(replay)))
}
struct ReportingStore<'a> {
    writer: &'a mut CacheWriter,
    progress: &'a dyn Fn(String),
}
impl ChunkStore for ReportingStore<'_> {
    fn prepare(&self, r: &AcquisitionRequest, c: &[Channel], span: Duration) -> Result<()> {
        self.writer.prepare(r, c, span)
    }
    fn restore(&mut self, c: Channel, w: Option<Window>) -> Result<Option<ValidatedChunk>> {
        (self.progress)(format!(
            "Loading {} | {} chunks saved; Esc cancels",
            c.endpoint(),
            self.writer.completed_chunks()
        ));
        self.writer.restore(c, w)
    }
    fn accept(&mut self, c: ValidatedChunk) -> Result<()> {
        self.writer.accept(c)?;
        (self.progress)(format!(
            "Saved {} recording chunks; Esc cancels",
            self.writer.completed_chunks()
        ));
        Ok(())
    }
}
