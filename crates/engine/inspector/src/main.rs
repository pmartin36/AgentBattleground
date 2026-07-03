mod client;
mod app;

fn main() {
    let path = match std::env::args().nth(1) {
        Some(p) => p,
        None => {
            eprintln!("usage: inspector <socket-path>");
            std::process::exit(2);
        }
    };

    let client = match client::connect(std::path::Path::new(&path)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("inspector: connect to {path} failed: {e}");
            std::process::exit(1);
        }
    };

    let native_options = eframe::NativeOptions::default();
    if let Err(e) = eframe::run_native(
        "Agent Battleground Inspector",
        native_options,
        Box::new(|_cc| {
            let mut app = app::InspectorApp::new(client);
            app.start();
            Ok(Box::new(app))
        }),
    ) {
        eprintln!("inspector: eframe error: {e}");
        std::process::exit(1);
    }
}
