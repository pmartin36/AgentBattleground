//! Local-GPU capability probe. Every generation operation queries this
//! before choosing between the GPU generation path and a fallback (import
//! for images, a caller-defined fallback for animation) — it must never
//! panic and must always resolve to one of the two determinate values,
//! regardless of whether `nvidia-smi` is present on the host.

/// Whether a local GPU suitable for generation is available on this
/// machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuCapability {
    Available,
    Unavailable,
}

/// Probes for a local GPU. Never panics: a missing `nvidia-smi`, a
/// non-zero exit, or an empty device list all resolve to `Unavailable`
/// rather than propagating an error.
pub fn capability() -> GpuCapability {
    match std::process::Command::new("nvidia-smi").arg("-L").output() {
        Ok(output)
            if output.status.success()
                && !String::from_utf8_lossy(&output.stdout).trim().is_empty() =>
        {
            GpuCapability::Available
        }
        _ => GpuCapability::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `capability()` always resolves to one of the two determinate
    /// variants (and therefore never panics), independent of whether this
    /// host has a GPU.
    #[test]
    fn capability_returns_determinate_value() {
        let result = capability();
        assert!(
            matches!(result, GpuCapability::Available | GpuCapability::Unavailable),
            "capability() must resolve to a determinate value, got {result:?}"
        );
    }
}
