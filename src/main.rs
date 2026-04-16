use gtk4::prelude::*;
use libadwaita::prelude::*;
use libadwaita::Application;
use log::info;

mod ui;
mod core;

const APP_ID: &str = "org.vinmod.KernelBuilder";

fn main() {
    // Initialize standard logging
    env_logger::init();
    info!("Starting VinMod GNOME application...");

    // Create a new libadwaita application
    let app = Application::builder().application_id(APP_ID).build();

    // Connect to "activate" signal of `app`
    app.connect_activate(build_ui);

    // Run the application
    app.run();
}

fn build_ui(app: &Application) {
    info!("Building user interface...");
    ui::window::build_main_window(app);
}
