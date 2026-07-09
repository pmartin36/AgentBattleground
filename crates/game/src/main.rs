use std::io;

fn main() -> io::Result<()> {
    // MUST be the very first statement — installs the panic hook + writer before anything runs.
    let log = engine_core::logging::init("agent-battleground")?;
    // stdout, before the alternate screen — same convention as inspect's "[inspect] socket: ..."
    println!("[log] {}", log.log_path.display());
    tracing::info!(
        log_path = %log.log_path.display(),
        version = env!("CARGO_PKG_VERSION"),
        "agent battleground starting"
    );

    match game::cli::resolve_boot(std::env::args().skip(1)) {
        Ok((id, params)) => game::app::run_with_params(game::registry::construct(id), params),
        Err(e) => {
            tracing::error!(error = %e, "cli boot failed");
            eprintln!("{e}");
            drop(log); // REQUIRED: flush the non-blocking writer before exit (see DATA_FLOW)
            std::process::exit(1);
        }
    }
}
