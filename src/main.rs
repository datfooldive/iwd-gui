mod app;
mod dbus;
mod models;

use app::IwdGuiApp;

fn main() {
    let options = eframe::NativeOptions::default();
    let run = eframe::run_native(
        "iwd-gui",
        options,
        Box::new(|_cc| Ok(Box::new(IwdGuiApp::default()))),
    );

    if let Err(err) = run {
        eprintln!("failed to start GUI: {err}");
    }
}
