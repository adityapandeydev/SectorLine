//! Pure monotonic playback clock. No wall time, source cadence, disk or network.
use crate::domain::ElapsedMillis;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackStatus {
    Paused,
    Playing,
    Ended,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speed {
    Quarter,
    Half,
    Normal,
    Double,
    Five,
    Ten,
    Twenty,
}
impl Speed {
    fn quarters(self) -> u128 {
        match self {
            Self::Quarter => 1,
            Self::Half => 2,
            Self::Normal => 4,
            Self::Double => 8,
            Self::Five => 20,
            Self::Ten => 40,
            Self::Twenty => 80,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Quarter => "0.25x",
            Self::Half => "0.5x",
            Self::Normal => "1x",
            Self::Double => "2x",
            Self::Five => "5x",
            Self::Ten => "10x",
            Self::Twenty => "20x",
        }
    }
}
pub struct PlaybackClock {
    end: ElapsedMillis,
    position_quarter_ns: u128,
    last: Duration,
    status: PlaybackStatus,
    speed: Speed,
}
impl PlaybackClock {
    pub fn new(end: ElapsedMillis) -> Self {
        Self {
            end,
            position_quarter_ns: 0,
            last: Duration::ZERO,
            status: if end.0 == 0 {
                PlaybackStatus::Ended
            } else {
                PlaybackStatus::Paused
            },
            speed: Speed::Normal,
        }
    }
    pub fn position(&self) -> ElapsedMillis {
        ElapsedMillis((self.position_quarter_ns / 4_000_000) as u64)
    }
    pub fn status(&self) -> PlaybackStatus {
        self.status
    }
    pub fn speed(&self) -> Speed {
        self.speed
    }
    pub fn sync(&mut self, now: Duration) -> Result<(), String> {
        let elapsed = now
            .checked_sub(self.last)
            .ok_or("Playback clock moved backwards")?;
        if self.status == PlaybackStatus::Playing {
            self.position_quarter_ns = self
                .position_quarter_ns
                .saturating_add(elapsed.as_nanos().saturating_mul(self.speed.quarters()));
            self.clamp();
        }
        self.last = now;
        Ok(())
    }
    pub fn toggle(&mut self, now: Duration) -> Result<(), String> {
        self.sync(now)?;
        self.status = match self.status {
            PlaybackStatus::Paused => PlaybackStatus::Playing,
            PlaybackStatus::Playing => PlaybackStatus::Paused,
            PlaybackStatus::Ended => PlaybackStatus::Ended,
        };
        Ok(())
    }
    pub fn pause(&mut self, now: Duration) -> Result<(), String> {
        self.sync(now)?;
        if self.status != PlaybackStatus::Ended {
            self.status = PlaybackStatus::Paused;
        }
        Ok(())
    }
    pub fn set_speed(&mut self, speed: Speed, now: Duration) -> Result<(), String> {
        self.sync(now)?;
        self.speed = speed;
        Ok(())
    }
    pub fn restart(&mut self, now: Duration) -> Result<(), String> {
        self.sync(now)?;
        self.position_quarter_ns = 0;
        self.status = if self.end.0 == 0 {
            PlaybackStatus::Ended
        } else {
            PlaybackStatus::Paused
        };
        Ok(())
    }
    /// Seeking is deliberate and always pauses, retaining the speed.
    pub fn seek(&mut self, target: ElapsedMillis, now: Duration) -> Result<(), String> {
        self.pause(now)?;
        self.position_quarter_ns = target.0.min(self.end.0) as u128 * 4_000_000;
        self.status = PlaybackStatus::Paused;
        self.clamp();
        Ok(())
    }
    pub fn step(&mut self, milliseconds: u64, now: Duration) -> Result<(), String> {
        self.pause(now)?;
        self.position_quarter_ns = self
            .position_quarter_ns
            .saturating_add(milliseconds as u128 * 4_000_000);
        self.clamp();
        Ok(())
    }
    fn clamp(&mut self) {
        let end = self.end.0 as u128 * 4_000_000;
        if self.position_quarter_ns >= end {
            self.position_quarter_ns = end;
            self.status = PlaybackStatus::Ended;
        }
    }
}
