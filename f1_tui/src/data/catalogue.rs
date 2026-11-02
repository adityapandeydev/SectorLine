//! OpenF1 metadata discovery. Catalogue eligibility is not proof of telemetry coverage.
use crate::domain::{Provider, SessionId};
use chrono::DateTime;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::BTreeSet,
    error::Error,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const MAX_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meeting {
    pub meeting_key: u32,
    pub year: u16,
    pub meeting_name: String,
    pub circuit_short_name: String,
    pub location: String,
    pub country_name: String,
    #[serde(default)]
    pub is_cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub session_key: u32,
    pub meeting_key: u32,
    pub year: u16,
    pub session_name: String,
    pub session_type: String,
    pub date_start: Option<String>,
    pub date_end: Option<String>,
    #[serde(default)]
    pub is_cancelled: bool,
}
impl Session {
    pub fn id(&self) -> SessionId {
        SessionId::new(Provider::OpenF1, self.session_key.to_string()).expect("numeric key")
    }
    fn times(&self) -> Result<(Option<i64>, Option<i64>)> {
        fn parse(s: &Option<String>) -> Result<Option<i64>> {
            Ok(s.as_ref()
                .map(|v| DateTime::parse_from_rfc3339(v).map(|t| t.timestamp_millis()))
                .transpose()?)
        }
        Ok((parse(&self.date_start)?, parse(&self.date_end)?))
    }
    fn aliases(&self) -> &'static str {
        match self.session_name.as_str() {
            "Race" => "r",
            "Qualifying" => "q",
            "Practice 1" => "fp1",
            "Practice 2" => "fp2",
            "Practice 3" => "fp3",
            "Sprint" => "s",
            "Sprint Qualifying" | "Sprint Shootout" => "sq shootout",
            _ => "",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogueCache {
    pub version: u32,
    pub retrieved_at_ms: i64,
    pub loaded_years: Vec<u16>,
    pub meetings: Vec<Meeting>,
    pub sessions: Vec<Session>,
}

#[derive(Debug, Default)]
pub struct Query {
    pub year: Option<u16>,
    pub text: String,
}

#[derive(Debug)]
pub struct Selection<'a> {
    pub session: &'a Session,
    pub meeting: &'a Meeting,
    /// None means completed according to metadata, not guaranteed telemetry coverage.
    pub unavailable_reason: Option<String>,
}

#[derive(Debug)]
pub struct Catalogue {
    cache: CatalogueCache,
}

fn normalized(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            c if c.is_alphanumeric() => c,
            _ => ' ',
        })
        .collect()
}

fn safe_label(s: &str) -> bool {
    !s.chars().any(char::is_control)
}

impl Catalogue {
    pub fn new(mut cache: CatalogueCache) -> Result<Self> {
        if cache.version != 1 {
            return Err("unsupported catalogue version; refresh explicitly".into());
        }
        let mut meetings = BTreeSet::new();
        for m in &cache.meetings {
            if m.meeting_key == 0
                || m.year < 2023
                || m.meeting_name.trim().is_empty()
                || [
                    &m.meeting_name,
                    &m.circuit_short_name,
                    &m.location,
                    &m.country_name,
                ]
                .iter()
                .any(|s| !safe_label(s))
                || !meetings.insert(m.meeting_key)
            {
                return Err("invalid or duplicate catalogue meeting".into());
            }
        }
        let coverage: BTreeSet<_> = cache.loaded_years.iter().copied().collect();
        if coverage.len() != cache.loaded_years.len() || coverage.iter().any(|y| *y < 2023) {
            return Err("invalid catalogue year coverage".into());
        }
        let mut sessions = BTreeSet::new();
        for s in &cache.sessions {
            let m = cache
                .meetings
                .iter()
                .find(|m| m.meeting_key == s.meeting_key)
                .ok_or("orphan catalogue session")?;
            if s.session_key == 0
                || s.year != m.year
                || !coverage.contains(&s.year)
                || s.session_name.trim().is_empty()
                || s.session_type.trim().is_empty()
                || !safe_label(&s.session_name)
                || !safe_label(&s.session_type)
                || !sessions.insert(s.session_key)
            {
                return Err("invalid, duplicate or mismatched catalogue session".into());
            }
            if let (Some(start), Some(end)) = s.times()?
                && end < start
            {
                return Err("session ends before it starts".into());
            }
        }
        cache.meetings.sort_by_key(|m| (m.year, m.meeting_key));
        cache
            .sessions
            .sort_by_key(|s| (s.year, s.meeting_key, s.session_key));
        Ok(Self { cache })
    }

    pub fn years(&self) -> Vec<u16> {
        self.cache
            .meetings
            .iter()
            .map(|m| m.year)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    pub fn meetings(&self) -> &[Meeting] {
        &self.cache.meetings
    }
    pub fn retrieved_at_ms(&self) -> i64 {
        self.cache.retrieved_at_ms
    }

    fn selection<'a>(&'a self, session: &'a Session, now: i64) -> Selection<'a> {
        let meeting = self
            .cache
            .meetings
            .iter()
            .find(|m| m.meeting_key == session.meeting_key)
            .expect("validated meeting");
        let unavailable_reason = if session.is_cancelled || meeting.is_cancelled {
            Some("Session or meeting is cancelled".into())
        } else {
            match session.times().expect("validated timestamps") {
                (Some(_), Some(end)) if end <= now => None,
                (Some(_), Some(_)) => Some("Session has not completed".into()),
                _ => Some("Completion timestamps are unavailable".into()),
            }
        };
        Selection {
            session,
            meeting,
            unavailable_reason,
        }
    }

    pub fn search(&self, query: &Query, now: i64) -> Result<Vec<Selection<'_>>> {
        if let Some(year) = query.year
            && !self.cache.loaded_years.contains(&year)
        {
            return Err(
                format!("Sessions for {year} are not cached; refresh that exact year").into(),
            );
        }
        let query_text = normalized(&query.text);
        let terms: Vec<_> = query_text.split_whitespace().collect();
        Ok(self
            .cache
            .sessions
            .iter()
            .filter(|s| query.year.is_none_or(|y| s.year == y))
            .filter_map(|s| {
                let selection = self.selection(s, now);
                let m = selection.meeting;
                // Session type is retained as metadata but not searched: Sprint can have type Race.
                let text = normalized(&format!(
                    "{} {} {} {} {} {} {}",
                    m.meeting_name,
                    m.circuit_short_name,
                    m.location,
                    m.country_name,
                    s.year,
                    s.session_name,
                    s.aliases()
                ));
                let words: BTreeSet<_> = text.split_whitespace().collect();
                terms.iter().all(|t| words.contains(t)).then_some(selection)
            })
            .collect())
    }

    pub fn resolve(&self, query: &Query, now: i64) -> Result<Selection<'_>> {
        let mut matches = self.search(query, now)?;
        if matches.len() != 1 {
            return Err(format!(
                "Expected one session, found {}; refine year/name/type or select a session ID",
                matches.len()
            )
            .into());
        }
        Ok(matches.remove(0))
    }

    pub fn select(&self, key: u32, now: i64) -> Result<Selection<'_>> {
        let s = self
            .cache
            .sessions
            .iter()
            .find(|s| s.session_key == key)
            .ok_or("Session ID is not in the cached catalogue")?;
        Ok(self.selection(s, now))
    }

    pub fn load(path: &Path) -> Result<Self> {
        let bytes = bounded_read(fs::File::open(path).map_err(|e| {
            format!(
                "Cannot open catalogue {}: {e}; use --refresh to fetch metadata",
                path.display()
            )
        })?)?;
        Self::new(serde_json::from_slice(&bytes)?)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec(&self.cache)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("catalogue exceeds size limit".into());
        }
        let temp = path.with_extension(format!(
            "{}.{}.tmp",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        let result = (|| -> Result<()> {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temp, path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result
    }

    /// Explicit metadata refresh only; never invoked from a render callback.
    /// A year refresh retains other cached years, and publishes only after validation.
    pub fn refresh(previous: Option<&Self>, year: Option<u16>) -> Result<Self> {
        if year.is_some_and(|y| y < 2023) {
            return Err("OpenF1 historical catalogue starts in 2023".into());
        }
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build();
        let meetings: Vec<Meeting> = fetch(&agent, "meetings", None)?;
        let mut cache = CatalogueCache {
            version: 1,
            retrieved_at_ms: now_ms()?,
            loaded_years: vec![],
            meetings,
            sessions: vec![],
        };
        if let Some(old) = previous {
            cache.sessions = old.cache.sessions.clone();
            cache.loaded_years = old.cache.loaded_years.clone();
        }
        if let Some(year) = year {
            let sessions: Vec<Session> = fetch(&agent, "sessions", Some(year))?;
            if sessions.iter().any(|s| s.year != year) {
                return Err("Provider returned another year's sessions".into());
            }
            cache.sessions.retain(|s| s.year != year);
            cache.sessions.extend(sessions);
            if !cache.loaded_years.contains(&year) {
                cache.loaded_years.push(year);
            }
        }
        Self::new(cache)
    }
}

fn bounded_read(reader: impl Read) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("catalogue exceeds size limit".into());
    }
    Ok(bytes)
}

fn fetch<T: DeserializeOwned>(agent: &ureq::Agent, endpoint: &str, year: Option<u16>) -> Result<T> {
    let url = format!("https://api.openf1.org/v1/{endpoint}");
    let mut request = agent.get(&url);
    if let Some(y) = year {
        request = request.query("year", &y.to_string());
    }
    let response = request.call().map_err(|e| format!("Catalogue refresh failed ({endpoint}): {e}; existing cache is unchanged; retry explicitly later"))?;
    Ok(serde_json::from_slice(&bounded_read(
        response.into_reader(),
    )?)?)
}

pub fn now_ms() -> Result<i64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_millis()
        .try_into()?)
}

pub fn default_cache_path() -> Result<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".cache")))
    };
    Ok(base
        .filter(|p| p.is_absolute())
        .ok_or("No user cache directory; pass --catalogue-cache PATH")?
        .join("sectorline")
        .join("catalogue.json"))
}
