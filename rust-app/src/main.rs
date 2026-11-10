#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod download;
mod events;
mod tools;
mod ui;
mod update;

#[cfg(windows)]
fn configure_windows_app_id() {
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SetCurrentProcessExplicitAppUserModelID(app_id: *const u16) -> i32;
    }
    let app_id: Vec<u16> = "YoutubeToMP3.PyQt".encode_utf16().chain(Some(0)).collect();
    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(app_id.as_ptr());
    }
}

fn main() -> eframe::Result {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--version")) {
        println!("{}", update::current_version());
        return Ok(());
    }
    if update::handle_update_mode() {
        return Ok(());
    }
    #[cfg(windows)]
    configure_windows_app_id();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([760.0, 620.0])
            .with_min_inner_size([640.0, 540.0]),
        ..Default::default()
    };
    eframe::run_native(
        "YouTube to MP3",
        options,
        Box::new(|cc| Ok(Box::new(ui::App::new(cc)))),
    )
}
