//! Walkers that emit unconfirmed candidates from catalog roots. Never deletes.
//! Implemented in M2 (phase 4).

use crate::model::{Candidate, ScanId, ScanMode};
use crate::{EngineError, Result};

pub(crate) fn start(_mode: ScanMode) -> Result<ScanId> {
    Err(EngineError::Unsupported)
}

pub(crate) fn cancel(_scan: ScanId) -> Result<()> {
    Err(EngineError::Unsupported)
}

pub(crate) fn candidates(_scan: ScanId) -> Result<Vec<Candidate>> {
    Err(EngineError::Unsupported)
}
