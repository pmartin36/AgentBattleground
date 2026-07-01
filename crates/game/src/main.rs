use std::io;

fn main() -> io::Result<()> {
    match game::cli::resolve_boot(std::env::args().skip(1)) {
        Ok((id, params)) => game::app::run_with_params(game::registry::construct(id), params),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
