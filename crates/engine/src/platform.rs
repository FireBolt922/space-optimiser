//! macOS, Windows, and Linux path adapters. Implemented in M2 (phase 4) and
//! hardened in M7 (phase 9).

use crate::model::CandidateId;
use crate::{EngineError, Result};

pub(crate) fn reveal(_candidate: CandidateId) -> Result<()> {
    Err(EngineError::Unsupported)
}
