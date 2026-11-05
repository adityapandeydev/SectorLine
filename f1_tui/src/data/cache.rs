//! Immutable completed recordings and a journal for resumable partial acquisition.
use super::openf1::{
    AcquisitionRequest, Channel, ChunkMetadata, ChunkStore, Result, ValidatedChunk, Window,
};
use crate::domain::UtcMillis;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const VERSION: u32 = 1;
const MANIFEST_LIMIT: u64 = 8 * 1024 * 1024;
const RAW_LIMIT: u64 = 64 * 1024 * 1024;
const PLAN_LIMIT: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CacheSpec {
    session_key: u32,
    meeting_key: u32,
    year: u16,
    session_name: String,
    session_type: String,
    window: Window,
    chunk_ms: u64,
    channels: Vec<Channel>,
}
impl CacheSpec {
    pub fn new(
        session_key: u32,
        meeting_key: u32,
        year: u16,
        kind: (&str, &str),
        window: Window,
        chunk_span: Duration,
        channels: Vec<Channel>,
    ) -> Result<Self> {
        let spec = Self {
            session_key,
            meeting_key,
            year,
            session_name: kind.0.into(),
            session_type: kind.1.into(),
            window,
            chunk_ms: chunk_span.as_millis().try_into()?,
            channels,
        };
        spec.plan()?;
        Ok(spec)
    }
    pub fn request(&self) -> Result<AcquisitionRequest> {
        AcquisitionRequest::new(self.session_key, self.meeting_key, self.window)
    }
    pub fn channels(&self) -> &[Channel] {
        &self.channels
    }
    pub fn chunk_span(&self) -> Duration {
        Duration::from_millis(self.chunk_ms)
    }
    fn plan(&self) -> Result<Vec<(Channel, Option<Window>)>> {
        self.request()?;
        if self.year < 2023
            || [&self.session_name, &self.session_type]
                .iter()
                .any(|s| s.trim().is_empty() || s.len() > 256 || s.chars().any(char::is_control))
            || self.chunk_ms == 0
            || self.chunk_ms > 3_600_000
            || self.channels.first() != Some(&Channel::Drivers)
            || self.channels.iter().collect::<BTreeSet<_>>().len() != self.channels.len()
        {
            return Err("Invalid recording cache specification".into());
        }
        let mut plan = Vec::new();
        for &channel in &self.channels {
            if !channel.windowed() {
                plan.push((channel, None));
                continue;
            }
            let mut start = self.window.start().0;
            while start < self.window.end().0 {
                let end = start
                    .saturating_add(self.chunk_ms as i64)
                    .min(self.window.end().0);
                plan.push((
                    channel,
                    Some(Window::new(UtcMillis(start), UtcMillis(end))?),
                ));
                if plan.len() > PLAN_LIMIT {
                    return Err("Cache coverage requires too many chunks".into());
                }
                start = end;
            }
        }
        Ok(plan)
    }
    fn stem(&self) -> String {
        format!("openf1-{}", self.session_key)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    metadata: ChunkMetadata,
    bytes: usize,
    sha256: String,
    records: usize,
    field_issues: usize,
    empty_reason: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
struct Manifest {
    version: u32,
    provider: String,
    spec: CacheSpec,
    entries: Vec<Entry>,
}
impl Manifest {
    fn validate(&self, expected: &CacheSpec, complete: bool) -> Result<()> {
        if self.version != VERSION {
            return Err(
                "Unsupported recording cache version; choose an explicit new cache root to rebuild"
                    .into(),
            );
        }
        if self.provider != "OpenF1" || &self.spec != expected {
            return Err("Recording cache identity/year/type/coverage mismatch".into());
        }
        let plan = self.spec.plan()?;
        if self.entries.len() > plan.len() || (complete && self.entries.len() != plan.len()) {
            return Err("Recording cache coverage is incomplete or invalid".into());
        }
        for (entry, (channel, window)) in self.entries.iter().zip(plan) {
            let m = &entry.metadata;
            if m.channel != channel
                || m.window != window
                || m.session_key != expected.session_key
                || m.meeting_key != expected.meeting_key
                || entry.bytes as u64 > RAW_LIMIT
                || entry.sha256.len() != 64
                || !entry.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err("Recording chunk metadata does not match coverage plan".into());
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct SessionCache {
    directory: PathBuf,
    manifest: Manifest,
    drivers: BTreeSet<u32>,
}
impl SessionCache {
    pub fn exists(root: &Path, spec: &CacheSpec) -> bool {
        root.join(spec.stem()).exists()
    }
    pub fn open(root: &Path, expected: &CacheSpec) -> Result<Self> {
        let directory = root.join(expected.stem());
        Self::load(directory.clone(), expected, true)
            .map_err(|e| format!("Cannot open recording {}: {e}", directory.display()).into())
    }
    fn load(directory: PathBuf, expected: &CacheSpec, complete: bool) -> Result<Self> {
        reject_link(&directory)?;
        let manifest: Manifest = serde_json::from_slice(&read_bounded(
            &directory.join("manifest.json"),
            MANIFEST_LIMIT,
        )?)?;
        manifest.validate(expected, complete)?;
        let mut cache = Self {
            directory,
            manifest,
            drivers: BTreeSet::new(),
        };
        // Validate one bounded chunk at a time, never trusting just the manifest.
        for i in 0..cache.chunk_count() {
            let chunk = cache.read_chunk(i)?;
            if i == 0 {
                cache.drivers = roster(&chunk)?;
            }
        }
        Ok(cache)
    }
    pub fn directory(&self) -> &Path {
        &self.directory
    }
    pub fn chunk_count(&self) -> usize {
        self.manifest.entries.len()
    }
    pub fn spec(&self) -> &CacheSpec {
        &self.manifest.spec
    }
    pub fn read_chunk(&self, index: usize) -> Result<ValidatedChunk> {
        let entry = self
            .manifest
            .entries
            .get(index)
            .ok_or("Chunk index outside recording")?;
        let raw = read_bounded(&self.directory.join(chunk_file(index)), RAW_LIMIT)?;
        if raw.len() != entry.bytes || checksum(&raw) != entry.sha256 {
            return Err(format!(
                "Recording chunk {index} checksum mismatch; cache was not modified"
            )
            .into());
        }
        let chunk = ValidatedChunk::from_archive(
            entry.metadata.clone(),
            raw,
            &self.manifest.spec.request()?,
            &self.drivers,
        )?;
        if chunk.records().len() != entry.records
            || chunk.issues().len() != entry.field_issues
            || chunk.empty_reason() != entry.empty_reason.as_deref()
        {
            return Err("Recording validation summary mismatch".into());
        }
        if index == 0 {
            roster(&chunk)?;
        }
        Ok(chunk)
    }
}

pub struct CacheWriter {
    root: PathBuf,
    partial: SessionCache,
    // Held by the OS, so process termination releases the lock automatically.
    // Leave the lock file in place to avoid unlink/recreate races on Unix.
    _lock: fs::File,
    plan: Vec<(Channel, Option<Window>)>,
}
impl CacheWriter {
    pub fn begin(root: &Path, spec: &CacheSpec) -> Result<Self> {
        let plan = spec.plan()?;
        fs::create_dir_all(root)?;
        reject_link(root)?;
        if root.join(spec.stem()).exists() {
            return Err("Completed recording already exists; open it offline or use an explicit new root to rebuild".into());
        }
        let lock_path = root.join(format!("{}.lock", spec.stem()));
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)?;
        lock.try_lock().map_err(|e| {
            format!(
                "Cannot lock recording ({}): {e}; another writer may be active",
                lock_path.display()
            )
        })?;
        if root.join(spec.stem()).exists() {
            return Err(
                "Completed recording appeared while acquiring writer lock; open it offline".into(),
            );
        }
        let mut writer = Self {
            root: root.into(),
            partial: SessionCache {
                directory: root.join(format!("{}.partial", spec.stem())),
                manifest: Manifest {
                    version: VERSION,
                    provider: "OpenF1".into(),
                    spec: spec.clone(),
                    entries: vec![],
                },
                drivers: BTreeSet::new(),
            },
            _lock: lock,
            plan,
        };
        if writer.partial.directory.exists() {
            writer.partial = SessionCache::load(writer.partial.directory.clone(), spec, false)?;
        } else {
            fs::create_dir(&writer.partial.directory)?;
            writer.journal()?;
        }
        Ok(writer)
    }
    pub fn completed_chunks(&self) -> usize {
        self.partial.chunk_count()
    }
    pub fn directory(&self) -> &Path {
        &self.partial.directory
    }
    /// Revalidate raw archived evidence (e.g. T05 scratch output), never trust its
    /// previously normalized JSON. Identity/coverage/checksums still apply.
    pub fn import(&mut self, metadata: ChunkMetadata, raw: &[u8]) -> Result<()> {
        if raw.len() as u64 > RAW_LIMIT {
            return Err("Imported chunk exceeds byte limit".into());
        }
        let chunk = ValidatedChunk::from_archive(
            metadata,
            raw.to_vec(),
            &self.partial.manifest.spec.request()?,
            &self.partial.drivers,
        )?;
        self.accept(chunk)
    }
    fn journal(&self) -> Result<()> {
        let bytes = serde_json::to_vec(&self.partial.manifest)?;
        if bytes.len() as u64 > MANIFEST_LIMIT {
            return Err("Recording manifest exceeds size limit".into());
        }
        atomic_file(&self.partial.directory.join("manifest.json"), &bytes)
    }
    pub fn finish(self) -> Result<SessionCache> {
        self.partial
            .manifest
            .validate(&self.partial.manifest.spec, true)?;
        let verified = SessionCache::load(
            self.partial.directory.clone(),
            &self.partial.manifest.spec,
            true,
        )?;
        let final_path = self.root.join(self.partial.manifest.spec.stem());
        if final_path.exists() {
            return Err(
                "Completed recording appeared while publishing; refusing replacement".into(),
            );
        }
        // Directory rename is the publication boundary: incomplete data never has
        // the final recording name. Both paths are on the same filesystem.
        fs::rename(&self.partial.directory, &final_path)?;
        Ok(SessionCache {
            directory: final_path,
            ..verified
        })
    }
}
impl ChunkStore for CacheWriter {
    fn prepare(
        &self,
        request: &AcquisitionRequest,
        channels: &[Channel],
        chunk_span: Duration,
    ) -> Result<()> {
        let spec = &self.partial.manifest.spec;
        if request.session_key() != spec.session_key
            || request.meeting_key() != spec.meeting_key
            || request.window() != spec.window
            || channels != spec.channels
            || chunk_span != spec.chunk_span()
        {
            return Err("Acquisition settings do not match recording cache plan".into());
        }
        Ok(())
    }
    fn restore(
        &mut self,
        channel: Channel,
        window: Option<Window>,
    ) -> Result<Option<ValidatedChunk>> {
        let index = self
            .plan
            .iter()
            .position(|p| *p == (channel, window))
            .ok_or("Requested chunk is outside cache coverage")?;
        if index < self.completed_chunks() {
            Ok(Some(self.partial.read_chunk(index)?))
        } else {
            Ok(None)
        }
    }
    fn accept(&mut self, chunk: ValidatedChunk) -> Result<()> {
        let index = self
            .plan
            .iter()
            .position(|p| *p == (chunk.channel(), chunk.window()))
            .ok_or("Chunk is outside recording coverage")?;
        if chunk.session_key() != self.partial.manifest.spec.session_key
            || chunk.meeting_key() != self.partial.manifest.spec.meeting_key
        {
            return Err("Chunk recording identity mismatch".into());
        }
        let digest = checksum(chunk.raw_bytes());
        if index < self.completed_chunks() {
            let existing = &self.partial.manifest.entries[index];
            if existing.sha256 != digest || existing.metadata != chunk.metadata() {
                return Err(
                    "Conflicting retry of a committed chunk; cache was not modified".into(),
                );
            }
            self.partial.read_chunk(index)?;
            return Ok(());
        }
        if index != self.completed_chunks() {
            return Err("Recording chunks must be committed in plan order".into());
        }
        let chunk = ValidatedChunk::from_archive(
            chunk.metadata(),
            chunk.raw_bytes().to_vec(),
            &self.partial.manifest.spec.request()?,
            &self.partial.drivers,
        )?;
        let new_roster = if index == 0 {
            Some(roster(&chunk)?)
        } else {
            None
        };
        atomic_file(
            &self.partial.directory.join(chunk_file(index)),
            chunk.raw_bytes(),
        )?;
        self.partial.manifest.entries.push(Entry {
            metadata: chunk.metadata(),
            bytes: chunk.raw_bytes().len(),
            sha256: digest,
            records: chunk.records().len(),
            field_issues: chunk.issues().len(),
            empty_reason: chunk.empty_reason().map(str::to_owned),
        });
        if let Err(error) = self.journal() {
            self.partial.manifest.entries.pop();
            return Err(error);
        }
        if let Some(drivers) = new_roster {
            self.partial.drivers = drivers;
        }
        Ok(())
    }
}

fn roster(chunk: &ValidatedChunk) -> Result<BTreeSet<u32>> {
    let mut drivers = BTreeSet::new();
    for row in chunk.records() {
        let number = row["driver_number"]
            .as_u64()
            .ok_or("Invalid cached driver")? as u32;
        if !drivers.insert(number) {
            return Err("Duplicate cached roster identity".into());
        }
    }
    if drivers.is_empty() {
        return Err("Cached roster is empty".into());
    }
    Ok(drivers)
}
fn checksum(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn chunk_file(index: usize) -> String {
    format!("{index:06}.json")
}
fn reject_link(path: &Path) -> Result<()> {
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err("Cache paths must not be symbolic links".into());
    }
    Ok(())
}
fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    reject_link(path)?;
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err("Recording cache file exceeds size limit".into());
    }
    Ok(bytes)
}
fn atomic_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let temp = path.with_extension(format!(
        "{}-{}.tmp",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)?;
        file.write_all(bytes)?;
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
