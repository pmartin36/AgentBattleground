//! `TextBackend`: the one contract every text-generation backend
//! implements. `generate` executes the request itself and returns inert
//! text (or a structured error) directly; it never defers execution to a
//! caller the way `asset_gen`'s `RecipeBackend` defers to a runner.

use super::job::CancelFlag;
use super::types::{TextError, TextRequest};

/// `Send + Sync` so a `Box<dyn TextBackend>` can be captured into a job
/// queue's `Send + 'static` work-unit closure.
pub trait TextBackend: Send + Sync {
    /// Executes `request` and returns the model's completion text, or a
    /// structured error. `cancel` is observed so a timed-out call can tear
    /// down promptly instead of running unbounded.
    fn generate(&self, request: &TextRequest, cancel: &CancelFlag) -> Result<String, TextError>;
}
