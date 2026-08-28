//! The recipe dispatch seam: a `RecipeBackend` supplies model choice and
//! prompt construction for a given request shape; the asset-generation API
//! owns turning that into a runnable `sd-cli` invocation and executing it.

/// A concrete `sd-cli` invocation: the model to run and its arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SdCliInvocation {
    pub model: String,
    pub args: Vec<String>,
}

/// Supplies model choice and prompt construction for one request shape.
/// Implementations own no execution; they only translate a request into an
/// `SdCliInvocation` for the runner to execute.
pub trait RecipeBackend {
    type Request;

    fn invocation(&self, request: &Self::Request) -> SdCliInvocation;
}
