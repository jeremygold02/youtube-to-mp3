fn main() {
    println!("cargo:rerun-if-changed=icon.ico");
    println!("cargo:rerun-if-env-changed=YTDLP_APP_VERSION");
    #[cfg(windows)]
    winres::WindowsResource::new()
        .set_icon("icon.ico")
        .compile()
        .expect("could not embed the application icon");
}
