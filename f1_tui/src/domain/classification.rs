//! Status labels never synthesize position or infer retirement from missing samples.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DriverStatus {
    Retired,
    Dnf,
    Dns,
    Dsq,
}
impl DriverStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Retired => "retired",
            Self::Dnf => "DNF",
            Self::Dns => "DNS",
            Self::Dsq => "DSQ",
        }
    }
}
