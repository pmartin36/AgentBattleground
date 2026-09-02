//! The recipe dispatch seam: a `RecipeBackend` supplies model choice and
//! prompt construction for a given request shape; the asset-generation API
//! owns turning that into a runnable `sd-cli` invocation and executing it.

use super::model_paths::{ModelPathError, ModelPaths};

/// A concrete `sd-cli` invocation: the mode and every model-path/sampling
/// flag, fully expressed in `args` (no separate positional model).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SdCliInvocation {
    pub args: Vec<String>,
}

/// Supplies model choice and prompt construction for one request shape.
/// Implementations own no execution; they only translate a request into an
/// `SdCliInvocation` for the runner to execute. `models` is a required
/// argument so no implementation can bypass path resolution.
pub trait RecipeBackend {
    type Request;

    fn invocation(&self, request: &Self::Request, models: &ModelPaths) -> Result<SdCliInvocation, ModelPathError>;
}
