use crate::config::{self, Config};
use crate::download::{self, DownloadRequest, Format, UrlKind};
use crate::events::{log, Event, Level};
use crate::{tools, update};
use chrono::Local;
use eframe::egui::{
    self, Align, Color32, FontData, FontDefinitions, FontFamily, Frame, Layout, RichText, Stroke,
    Vec2,
};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

const BG: Color32 = Color32::from_rgb(8, 19, 33);
const SURFACE: Color32 = Color32::from_rgb(16, 32, 53);
const INPUT: Color32 = Color32::from_rgb(16, 31, 50);
const BORDER: Color32 = Color32::from_rgb(48, 68, 94);
const TEXT: Color32 = Color32::from_rgb(237, 245, 255);
const MUTED: Color32 = Color32::from_rgb(151, 173, 195);
const ACCENT: Color32 = Color32::from_rgb(9, 183, 203);

const QUALITIES: [(&str, Option<u32>); 7] = [
    ("Best available", None),
    ("Up to 2160p (4K)", Some(2160)),
    ("Up to 1440p (QHD)", Some(1440)),
    ("Up to 1080p (Full HD)", Some(1080)),
    ("Up to 720p (HD)", Some(720)),
    ("Up to 480p", Some(480)),
    ("Up to 360p", Some(360)),
];

struct LogEntry {
    time: String,
    message: String,
    level: Level,
}

struct PendingPlaylist {
    kind: UrlKind,
    request: DownloadRequest,
}

pub struct App {
    url: String,
    config: Config,
    data_dir: Result<PathBuf, String>,
    format: Format,
    quality: usize,
    busy: bool,
    checking_release: bool,
    settings_open: bool,
    pending_playlist: Option<PendingPlaylist>,
    release: Option<update::UpdateInfo>,
    update_prompt_open: bool,
    entries: VecDeque<LogEntry>,
    sender: Sender<Event>,
    receiver: Receiver<Event>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_style(&cc.egui_ctx);
        let data_dir = config::data_dir();
        let (config, config_error) = match &data_dir {
            Ok(dir) => match config::load(dir) {
                Ok(mut config) => {
                    if config.download_path.is_empty() {
                        config.download_path = dir.join("Downloads").to_string_lossy().into_owned();
                    }
                    (config, None)
                }
                Err(error) => (Config::default(), Some(error)),
            },
            Err(error) => (Config::default(), Some(error.clone())),
        };
        let (sender, receiver) = mpsc::channel();
        let mut app = Self {
            url: String::new(),
            config,
            data_dir,
            format: Format::Mp3,
            quality: 1,
            busy: false,
            checking_release: false,
            settings_open: false,
            pending_playlist: None,
            release: None,
            update_prompt_open: false,
            entries: VecDeque::new(),
            sender,
            receiver,
        };
        if let Some(error) = config_error {
            app.append_log(error, Level::Error);
        }
        app.check_releases(&cc.egui_ctx, true);
        app
    }

    fn append_log(&mut self, message: impl Into<String>, level: Level) {
        let entry = LogEntry {
            time: Local::now().format("%H:%M:%S").to_string(),
            message: message.into(),
            level,
        };
        if level == Level::Progress
            && self
                .entries
                .back()
                .is_some_and(|previous| previous.level == Level::Progress)
        {
            self.entries.pop_back();
        }
        self.entries.push_back(entry);
        if self.entries.len() > 500 {
            self.entries.pop_front();
        }
    }

    fn receive_events(&mut self, context: &egui::Context) {
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                Event::Log(message, level) => self.append_log(message, level),
                Event::Finished => self.busy = false,
                Event::ReleaseChecked {
                    result,
                    silent_if_current,
                } => {
                    self.checking_release = false;
                    match result {
                        Ok(info) => {
                            if info.update_available && info.asset_url.is_some() {
                                self.append_log(
                                    format!("Update available: {}.", info.latest_version),
                                    Level::Warning,
                                );
                                self.update_prompt_open = true;
                            } else {
                                self.update_prompt_open = false;
                                if !silent_if_current {
                                    let message = if info.latest_version.is_empty() {
                                        "No published releases were found.".to_owned()
                                    } else if info.update_available {
                                        format!("The latest release ({}) does not include a Rust update yet.", info.latest_version)
                                    } else {
                                        format!(
                                            "You are on the latest published version ({}).",
                                            info.current_version
                                        )
                                    };
                                    self.append_log(message, Level::Info);
                                }
                            }
                            self.release = Some(info);
                        }
                        Err(error) => self.append_log(error, Level::Warning),
                    }
                }
                Event::UpdateInstalled(result) => {
                    self.busy = false;
                    match result {
                        Ok(()) => {
                            self.append_log("Update ready. Restarting...", Level::Success);
                            context.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        Err(error) => {
                            self.append_log(error, Level::Error);
                            self.update_prompt_open = true;
                        }
                    }
                }
            }
        }
    }

    fn begin_download(&mut self, context: &egui::Context) {
        if self.busy {
            return;
        }
        let url = self.url.trim().to_owned();
        if url.is_empty() {
            self.append_log("YouTube URL required.", Level::Error);
            return;
        }
        let destination = self.config.download_path.trim();
        if destination.is_empty() {
            self.append_log("Choose a download folder in Settings.", Level::Error);
            return;
        }
        let request = DownloadRequest {
            url,
            destination: PathBuf::from(destination),
            format: self.format,
            height: if self.format == Format::Mp4 {
                QUALITIES[self.quality].1
            } else {
                None
            },
            no_playlist: false,
        };
        match download::classify_url(&request.url) {
            UrlKind::Single => self.start_download(request, context),
            kind => self.pending_playlist = Some(PendingPlaylist { kind, request }),
        }
    }

    fn start_download(&mut self, request: DownloadRequest, context: &egui::Context) {
        let dir = match &self.data_dir {
            Ok(dir) => dir.clone(),
            Err(error) => {
                self.append_log(error.clone(), Level::Error);
                return;
            }
        };
        self.busy = true;
        let sender = self.sender.clone();
        let repaint = context.clone();
        thread::spawn(move || {
            if let Err(error) = download::run(request, &dir, &sender) {
                log(&sender, error, Level::Error);
            }
            let _ = sender.send(Event::Finished);
            repaint.request_repaint();
        });
    }

    fn update_tools(&mut self, context: &egui::Context) {
        let dir = match &self.data_dir {
            Ok(dir) => dir.clone(),
            Err(error) => {
                self.append_log(error.clone(), Level::Error);
                return;
            }
        };
        self.busy = true;
        let sender = self.sender.clone();
        let repaint = context.clone();
        thread::spawn(move || {
            if let Err(error) = tools::update_tools(&dir, &sender) {
                log(&sender, error, Level::Error);
            }
            let _ = sender.send(Event::Finished);
            repaint.request_repaint();
        });
    }

    fn check_releases(&mut self, context: &egui::Context, silent_if_current: bool) {
        if self.checking_release {
            return;
        }
        self.checking_release = true;
        let sender = self.sender.clone();
        let repaint = context.clone();
        thread::spawn(move || {
            let _ = sender.send(Event::ReleaseChecked {
                result: update::check_for_update(),
                silent_if_current,
            });
            repaint.request_repaint();
        });
    }

    fn install_app_update(&mut self, context: &egui::Context) {
        if self.busy {
            self.append_log(
                "Finish the current task before installing an update.",
                Level::Warning,
            );
            return;
        }
        let Some(info) = self
            .release
            .clone()
            .filter(|info| info.update_available && info.asset_url.is_some())
        else {
            self.append_log("No installable app update is available.", Level::Warning);
            return;
        };
        let dir = match &self.data_dir {
            Ok(dir) => dir.clone(),
            Err(error) => {
                self.append_log(error.clone(), Level::Error);
                return;
            }
        };
        self.busy = true;
        let sender = self.sender.clone();
        let repaint = context.clone();
        thread::spawn(move || {
            let result = update::install_update(&info, &dir, &sender);
            let _ = sender.send(Event::UpdateInstalled(result));
            repaint.request_repaint();
        });
    }

    fn choose_folder(&mut self) {
        let mut dialog = rfd::FileDialog::new().set_title("Set Download Folder");
        let current = Path::new(&self.config.download_path);
        if current.is_dir() {
            dialog = dialog.set_directory(current);
        }
        if let Some(path) = dialog.pick_folder() {
            let new_path = path.to_string_lossy().into_owned();
            let previous = std::mem::replace(&mut self.config.download_path, new_path);
            match &self.data_dir {
                Ok(dir) => {
                    if let Err(error) = config::save(dir, &self.config) {
                        self.config.download_path = previous;
                        self.append_log(error, Level::Error);
                    } else {
                        self.append_log("Download folder updated.", Level::Success);
                    }
                }
                Err(error) => {
                    self.config.download_path = previous;
                    self.append_log(error.clone(), Level::Error);
                }
            }
        }
    }

    fn open_folder(&mut self) {
        let folder = self.config.download_path.trim().to_owned();
        if folder.is_empty() {
            self.append_log("Choose a download folder in Settings.", Level::Error);
            return;
        }
        if let Err(error) = std::fs::create_dir_all(&folder) {
            self.append_log(format!("Could not open folder: {error}"), Level::Error);
            return;
        }
        match Command::new("explorer.exe").arg(&folder).spawn() {
            Ok(_) => self.append_log(format!("Opened folder: {folder}"), Level::Info),
            Err(error) => self.append_log(format!("Could not open folder: {error}"), Level::Error),
        }
    }

    fn draw_main(&mut self, ui: &mut egui::Ui, context: &egui::Context) {
        let enabled = !self.busy && self.pending_playlist.is_none();
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("YouTube to MP3")
                        .size(23.0)
                        .strong()
                        .color(TEXT),
                );
                ui.label(
                    RichText::new("AUDIO & VIDEO DOWNLOADER")
                        .size(10.5)
                        .strong()
                        .color(MUTED),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add_enabled(
                        enabled,
                        secondary_button("Settings").min_size(Vec2::new(100.0, 38.0)),
                    )
                    .clicked()
                {
                    self.settings_open = true;
                }
            });
        });
        ui.add_space(15.0);
        section_label(ui, "VIDEO LINK");
        ui.add_space(5.0);
        let url_input = ui.add_enabled(
            enabled,
            egui::TextEdit::singleline(&mut self.url)
                .hint_text("Paste a YouTube link here")
                .desired_width(f32::INFINITY)
                .margin(egui::Margin::symmetric(12, 11)),
        );
        if url_input.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            self.begin_download(context);
        }
        ui.add_space(15.0);

        let width = ui.available_width();
        let format_width = (width - 16.0) * 0.59;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.set_width(format_width);
                section_label(ui, "OUTPUT FORMAT");
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    let button_width = ((format_width - 8.0) / 2.0).max(110.0);
                    for (label, format) in [("MP3 Audio", Format::Mp3), ("MP4 Video", Format::Mp4)]
                    {
                        let selected = self.format == format;
                        let button =
                            format_button(label, selected).min_size(Vec2::new(button_width, 44.0));
                        if ui.add_enabled(enabled, button).clicked() {
                            self.format = format;
                        }
                    }
                });
            });
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.set_width((width - format_width - 16.0).max(160.0));
                section_label(ui, "VIDEO QUALITY");
                ui.add_space(5.0);
                ui.add_enabled_ui(enabled && self.format == Format::Mp4, |ui| {
                    ui.spacing_mut().interact_size.y = 44.0;
                    egui::ComboBox::from_id_salt("video-quality")
                        .selected_text(QUALITIES[self.quality].0)
                        .width(ui.available_width() - 18.0)
                        .show_ui(ui, |ui| {
                            for (index, (label, _)) in QUALITIES.iter().enumerate() {
                                ui.selectable_value(&mut self.quality, index, *label);
                            }
                        });
                });
            });
        });
        ui.add_space(14.0);
        let label = if self.format == Format::Mp3 {
            "Download MP3"
        } else {
            "Download MP4"
        };
        if download_button(ui, label, enabled).clicked() {
            self.begin_download(context);
        }
        ui.add_space(15.0);
        section_label(ui, "SAVE TO");
        ui.add_space(5.0);
        ui.horizontal(|ui| {
            let path_width = (ui.available_width() - 130.0).max(100.0);
            Frame::new()
                .fill(INPUT)
                .stroke(Stroke::new(1.0, BORDER))
                .corner_radius(7)
                .inner_margin(egui::Margin::symmetric(12, 11))
                .show(ui, |ui| {
                    let text = if self.config.download_path.is_empty() {
                        "No download folder set"
                    } else {
                        &self.config.download_path
                    };
                    ui.add_sized(
                        [path_width - 24.0, 20.0],
                        egui::Label::new(RichText::new(text).color(TEXT))
                            .truncate()
                            .halign(Align::Min),
                    )
                    .on_hover_text(text);
                });
            if ui
                .add_enabled(
                    enabled,
                    secondary_button("Open Folder").min_size(Vec2::new(120.0, 44.0)),
                )
                .clicked()
            {
                self.open_folder();
            }
        });
        ui.add_space(15.0);
        self.draw_log(ui);
    }

    fn draw_log(&mut self, ui: &mut egui::Ui) {
        let log_height = ui.available_height().max(100.0);
        Frame::new()
            .fill(SURFACE)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(9)
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.set_min_height((log_height - 28.0).max(70.0));
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Activity log")
                            .size(15.0)
                            .strong()
                            .color(TEXT),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(secondary_button("Clear")).clicked() {
                            self.entries.clear();
                            self.append_log("Log cleared.", Level::Info);
                        }
                    });
                });
                ui.add_space(8.0);
                let height = ui.available_height().max(45.0);
                Frame::new()
                    .fill(Color32::from_rgb(9, 22, 37))
                    .stroke(Stroke::new(1.0, BORDER))
                    .corner_radius(7)
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.set_min_height((height - 20.0).max(25.0));
                        egui::ScrollArea::vertical()
                            .id_salt("activity-log")
                            .stick_to_bottom(true)
                            .max_height((height - 20.0).max(25.0))
                            .show(ui, |ui| {
                                if self.entries.is_empty() {
                                    ui.label(
                                        RichText::new("Your download activity will appear here.")
                                            .color(MUTED),
                                    );
                                }
                                for entry in &self.entries {
                                    let color = match entry.level {
                                        Level::Success => Color32::from_rgb(92, 220, 139),
                                        Level::Warning => Color32::from_rgb(250, 204, 21),
                                        Level::Error => Color32::from_rgb(248, 113, 113),
                                        Level::Progress => Color32::from_rgb(147, 197, 253),
                                        Level::Info => Color32::from_rgb(209, 220, 232),
                                    };
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(
                                            RichText::new(format!("[{}]", entry.time))
                                                .size(12.0)
                                                .color(MUTED),
                                        );
                                        ui.label(
                                            RichText::new(&entry.message).size(12.0).color(color),
                                        );
                                    });
                                }
                            });
                    });
            });
    }

    fn draw_settings(&mut self, context: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut open = self.settings_open;
        egui::Window::new("Settings")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .default_width(540.0)
            .show(context, |ui| {
                Frame::new()
                    .inner_margin(egui::Margin::symmetric(16, 14))
                    .show(ui, |ui| {
                        ui.set_width(500.0);
                        ui.spacing_mut().item_spacing = Vec2::new(10.0, 8.0);
                        ui.label(
                            RichText::new("Manage your download folder and updates.").color(MUTED),
                        );
                        ui.add_space(12.0);
                        section_label(ui, "DOWNLOAD FOLDER");
                        ui.horizontal(|ui| {
                            let button_width = 160.0;
                            let path_width =
                                (ui.available_width() - button_width - 10.0).max(100.0);
                            let text = if self.config.download_path.is_empty() {
                                "No download folder set"
                            } else {
                                &self.config.download_path
                            };
                            Frame::new()
                                .fill(INPUT)
                                .stroke(Stroke::new(1.0, BORDER))
                                .corner_radius(7)
                                .inner_margin(egui::Margin::symmetric(12, 11))
                                .show(ui, |ui| {
                                    ui.add_sized(
                                        [path_width - 24.0, 18.0],
                                        egui::Label::new(RichText::new(text).color(TEXT))
                                            .truncate()
                                            .halign(Align::Min),
                                    )
                                    .on_hover_text(text);
                                });
                            if ui
                                .add(
                                    secondary_button("Set Download Folder")
                                        .min_size(Vec2::new(button_width, 42.0)),
                                )
                                .clicked()
                            {
                                self.choose_folder();
                            }
                        });
                        ui.add_space(12.0);
                        section_label(ui, "UPDATES");
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .add_enabled(
                                    !self.checking_release,
                                    secondary_button("Check for Updates")
                                        .min_size(Vec2::new(190.0, 40.0)),
                                )
                                .clicked()
                            {
                                self.check_releases(context, false);
                            }
                            if ui
                                .add_enabled(
                                    !self.busy,
                                    secondary_button("Update yt-dlp, Deno & FFmpeg")
                                        .min_size(Vec2::new(280.0, 40.0)),
                                )
                                .clicked()
                            {
                                self.update_tools(context);
                            }
                        });
                        if let Some(info) = self
                            .release
                            .as_ref()
                            .filter(|info| !info.latest_version.is_empty())
                        {
                            let version = info.latest_version.clone();
                            let url = info.release_url.clone();
                            let can_install = info.update_available && info.asset_url.is_some();
                            let mut show_update = false;
                            ui.horizontal_wrapped(|ui| {
                                ui.label(format!("Latest published app release: {version}"));
                                if can_install
                                    && ui
                                        .add(
                                            secondary_button("View Update")
                                                .min_size(Vec2::new(110.0, 36.0)),
                                        )
                                        .clicked()
                                {
                                    show_update = true;
                                }
                                if ui
                                    .add(
                                        secondary_button("Open Release Page")
                                            .min_size(Vec2::new(145.0, 36.0)),
                                    )
                                    .clicked()
                                {
                                    open_release_page(&url);
                                }
                            });
                            if show_update {
                                self.settings_open = false;
                                self.update_prompt_open = true;
                            }
                        }
                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(4.0);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add(secondary_button("Close").min_size(Vec2::new(96.0, 38.0)))
                                .clicked()
                            {
                                self.settings_open = false;
                            }
                        });
                    });
            });
        self.settings_open &= open;
    }

    fn draw_update(&mut self, context: &egui::Context) {
        if !self.update_prompt_open {
            return;
        }
        let Some(info) = self.release.clone() else {
            self.update_prompt_open = false;
            return;
        };
        let mut open = true;
        let mut install = false;
        let mut later = false;
        egui::Window::new("Update Available")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .default_width(440.0)
            .show(context, |ui| {
                ui.label(format!(
                    "YouTube to MP3 {} is available.",
                    info.latest_version
                ));
                ui.label(
                    RichText::new("The app will close and restart after the update is installed.")
                        .color(MUTED),
                );
                if self.busy {
                    ui.label(
                        RichText::new("Finish the current task before installing.").color(MUTED),
                    );
                }
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            !self.busy,
                            secondary_button("Install Update").min_size(Vec2::new(120.0, 38.0)),
                        )
                        .clicked()
                    {
                        install = true;
                    }
                    if ui
                        .add(secondary_button("Open Release Page").min_size(Vec2::new(150.0, 38.0)))
                        .clicked()
                    {
                        open_release_page(&info.release_url);
                    }
                    if ui
                        .add(secondary_button("Later").min_size(Vec2::new(80.0, 38.0)))
                        .clicked()
                    {
                        later = true;
                    }
                });
            });
        self.update_prompt_open = open && !later && !install;
        if install {
            self.install_app_update(context);
        }
    }

    fn draw_playlist(&mut self, context: &egui::Context) {
        let Some(pending) = &self.pending_playlist else {
            return;
        };
        let kind = pending.kind;
        let mut choice: Option<Option<bool>> = None;
        egui::Window::new("Playlist detected")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .default_width(410.0)
            .show(context, |ui| match kind {
                UrlKind::VideoInPlaylist => {
                    ui.label("This video is part of a playlist. What would you like to download?");
                    ui.add_space(10.0);
                    ui.horizontal_wrapped(|ui| {
                        if ui.add(secondary_button("This Video")).clicked() {
                            choice = Some(Some(true));
                        }
                        if ui.add(secondary_button("Entire Playlist")).clicked() {
                            choice = Some(Some(false));
                        }
                        if ui.add(secondary_button("Cancel")).clicked() {
                            choice = Some(None);
                        }
                    });
                }
                UrlKind::Playlist => {
                    ui.label("Download the entire playlist?");
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.add(secondary_button("Download Playlist")).clicked() {
                            choice = Some(Some(false));
                        }
                        if ui.add(secondary_button("Cancel")).clicked() {
                            choice = Some(None);
                        }
                    });
                }
                UrlKind::Single => {}
            });
        if let Some(choice) = choice {
            if let Some(mut pending) = self.pending_playlist.take() {
                if let Some(no_playlist) = choice {
                    pending.request.no_playlist = no_playlist;
                    let message = if no_playlist {
                        "Downloading this video."
                    } else {
                        "Downloading playlist."
                    };
                    self.append_log(message, Level::Info);
                    self.start_download(pending.request, context);
                } else {
                    self.append_log("Download canceled.", Level::Info);
                }
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let context = ui.ctx().clone();
        self.receive_events(&context);
        ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
        Frame::new()
            .fill(BG)
            .inner_margin(egui::Margin::symmetric(26, 24))
            .show(ui, |ui| {
                self.draw_main(ui, &context);
            });
        self.draw_settings(&context);
        self.draw_playlist(&context);
        self.draw_update(&context);
        if self.busy || self.checking_release {
            context.request_repaint_after(Duration::from_millis(100));
        }
    }
}

fn open_release_page(url: &str) {
    let safe = url::Url::parse(url).ok().is_some_and(|parsed| {
        parsed.scheme() == "https"
            && parsed.host_str() == Some("github.com")
            && (parsed.path() == "/jeremygold02/youtube-to-mp3/releases"
                || parsed
                    .path()
                    .starts_with("/jeremygold02/youtube-to-mp3/releases/"))
    });
    if safe {
        let _ = Command::new("explorer.exe").arg(url).spawn();
    }
}

fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(11.0).strong().color(MUTED));
}

fn download_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> egui::Response {
    ui.scope(|ui| {
        let widgets = &mut ui.visuals_mut().widgets;
        widgets.inactive.weak_bg_fill = ACCENT;
        widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(26, 211, 225));
        widgets.hovered.weak_bg_fill = Color32::from_rgb(49, 213, 226);
        widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(156, 244, 249));
        widgets.active.weak_bg_fill = Color32::from_rgb(5, 142, 158);
        widgets.active.bg_stroke = Stroke::new(1.0, Color32::from_rgb(156, 244, 249));
        let button = egui::Button::new((
            egui::Atom::grow(),
            RichText::new(text)
                .size(15.0)
                .strong()
                .color(Color32::from_rgb(5, 24, 32)),
            egui::Atom::grow(),
        ))
        .gap(0.0)
        .corner_radius(7)
        .min_size(Vec2::new(ui.available_width(), 48.0));
        ui.add_enabled(enabled, button)
    })
    .inner
}

fn secondary_button(text: &str) -> egui::Button<'_> {
    egui::Button::new((
        egui::Atom::grow(),
        RichText::new(text).size(13.0).color(TEXT),
        egui::Atom::grow(),
    ))
    .gap(0.0)
    .corner_radius(7)
}

fn format_button(text: &str, selected: bool) -> egui::Button<'_> {
    egui::Button::new((
        egui::Atom::grow(),
        RichText::new(text).size(13.0).strong().color(TEXT),
        egui::Atom::grow(),
    ))
    .gap(0.0)
    .selected(selected)
    .corner_radius(7)
}

fn configure_style(context: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = SURFACE;
    visuals.extreme_bg_color = INPUT;
    visuals.override_text_color = Some(TEXT);
    visuals.widgets.noninteractive.bg_fill = INPUT;
    visuals.widgets.inactive.bg_fill = INPUT;
    visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(26, 44, 67);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(36, 58, 84);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(42, 69, 96);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    visuals.widgets.active.weak_bg_fill = Color32::from_rgb(16, 95, 110);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, Color32::from_rgb(113, 230, 239));
    visuals.selection.bg_fill = Color32::from_rgb(10, 70, 85);
    visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
    context.set_visuals(visuals);
    if let Ok(windows) = std::env::var("WINDIR") {
        if let Ok(font) = std::fs::read(Path::new(&windows).join("Fonts").join("segoeui.ttf")) {
            let mut fonts = FontDefinitions::default();
            fonts
                .font_data
                .insert("windows-segoe".into(), FontData::from_owned(font).into());
            if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
                family.insert(0, "windows-segoe".into());
            }
            context.set_fonts(fonts);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let (sender, receiver) = mpsc::channel();
        App {
            url: String::new(),
            config: Config {
                download_path: "prototype output".into(),
            },
            data_dir: Ok(PathBuf::from("prototype data")),
            format: Format::Mp3,
            quality: 1,
            busy: false,
            checking_release: false,
            settings_open: false,
            pending_playlist: None,
            release: None,
            update_prompt_open: false,
            entries: VecDeque::new(),
            sender,
            receiver,
        }
    }

    #[test]
    fn progress_updates_replace_only_the_previous_progress_entry() {
        let mut app = app();
        app.append_log("Starting", Level::Info);
        for percent in 0..100 {
            app.append_log(format!("{percent}%"), Level::Progress);
        }
        assert_eq!(app.entries.len(), 2);
        assert_eq!(app.entries[1].message, "99%");
        app.append_log("Merging", Level::Info);
        app.append_log("10%", Level::Progress);
        assert_eq!(app.entries.len(), 4);
        assert_eq!(app.entries[1].message, "99%");
        assert_eq!(app.entries[3].message, "10%");
    }

    #[test]
    fn empty_url_does_not_start_a_worker() {
        let mut app = app();
        app.begin_download(&egui::Context::default());
        assert!(!app.busy);
        assert!(app.pending_playlist.is_none());
        assert_eq!(app.entries.back().unwrap().level, Level::Error);
    }

    #[test]
    fn playlist_url_requires_a_choice_before_starting() {
        let mut app = app();
        app.url = "https://youtube.com/watch?v=abc&list=xyz".into();
        app.begin_download(&egui::Context::default());
        assert!(!app.busy);
        assert_eq!(
            app.pending_playlist.as_ref().unwrap().kind,
            UrlKind::VideoInPlaylist
        );
    }

    #[test]
    fn update_prompt_requires_a_compatible_asset() {
        let mut app = app();
        let context = egui::Context::default();
        let info = update::UpdateInfo {
            current_version: "0.1.5".into(),
            latest_version: "0.1.6".into(),
            release_url: "https://github.com/jeremygold02/youtube-to-mp3/releases/tag/0.1.6".into(),
            update_available: true,
            asset_url: None,
        };
        app.sender
            .send(Event::ReleaseChecked {
                result: Ok(info.clone()),
                silent_if_current: true,
            })
            .unwrap();
        app.receive_events(&context);
        assert!(!app.update_prompt_open);
        assert!(app.entries.is_empty());
        let compatible = update::UpdateInfo { asset_url: Some("https://github.com/jeremygold02/youtube-to-mp3/releases/download/0.1.6/YouTube.to.MP3.Rust.exe".into()), ..info };
        app.sender
            .send(Event::ReleaseChecked {
                result: Ok(compatible),
                silent_if_current: true,
            })
            .unwrap();
        app.receive_events(&context);
        assert!(app.update_prompt_open);
        assert_eq!(app.entries.back().unwrap().level, Level::Warning);
        app.busy = true;
        app.install_app_update(&context);
        assert!(app.busy);
        assert_eq!(
            app.entries.back().unwrap().message,
            "Finish the current task before installing an update."
        );
        egui::__run_test_ui(|ui| app.draw_update(ui.ctx()));
    }

    #[test]
    fn main_window_and_settings_render_headlessly() {
        for format in [Format::Mp3, Format::Mp4] {
            let mut app = app();
            app.format = format;
            app.settings_open = true;
            egui::__run_test_ui(|ui| {
                let context = ui.ctx().clone();
                app.draw_main(ui, &context);
                app.draw_settings(&context);
            });
        }
    }

    #[test]
    fn buttons_show_hover_feedback_without_enabling_disabled_controls() {
        for kind in ["secondary", "selected", "download"] {
            let context = egui::Context::default();
            configure_style(&context);
            let render = |pointer: egui::Pos2, enabled: bool| {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(640.0, 540.0),
                    )),
                    events: vec![egui::Event::PointerMoved(pointer)],
                    ..Default::default()
                };
                let mut response = None;
                let output = context.run_ui(input, |ui| {
                    response = Some(match kind {
                        "secondary" => ui.add_enabled(
                            enabled,
                            secondary_button("Settings").min_size(Vec2::new(160.0, 40.0)),
                        ),
                        "selected" => ui.add_enabled(
                            enabled,
                            format_button("MP3 Audio", true).min_size(Vec2::new(160.0, 40.0)),
                        ),
                        _ => download_button(ui, "Download MP3", enabled),
                    });
                });
                let response = response.unwrap();
                let (fill, stroke) = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Rect(rect)
                            if rect.rect.contains(response.rect.center())
                                && rect.fill != Color32::TRANSPARENT =>
                        {
                            Some((rect.fill, rect.stroke.color))
                        }
                        _ => None,
                    })
                    .expect("button background should be painted");
                output.drop_without_applying_deltas();
                (response, fill, stroke)
            };
            let (resting, normal_fill, normal_stroke) = render(egui::pos2(630.0, 530.0), true);
            let center = resting.rect.center();
            let _ = render(center, true);
            let (hovered, hover_fill, hover_stroke) = render(center, true);
            assert!(hovered.hovered(), "{kind} should be hovered");
            assert!(
                normal_fill != hover_fill || normal_stroke != hover_stroke,
                "{kind} needs visible hover feedback"
            );
            let (disabled, _, _) = render(center, false);
            assert!(!disabled.enabled(), "{kind} must remain disabled");
        }
    }

    #[test]
    fn settings_actions_fit_the_minimum_window() {
        let context = egui::Context::default();
        configure_style(&context);
        let mut app = app();
        app.settings_open = true;
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(640.0, 540.0));
        let mut text_shapes = Vec::new();
        for _ in 0..3 {
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ui| {
                    app.draw_settings(ui.ctx());
                },
            );
            text_shapes = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some((
                        text.galley.job.text.clone(),
                        shape.clip_rect,
                        text.visual_bounding_rect(),
                    )),
                    _ => None,
                })
                .collect();
            output.drop_without_applying_deltas();
        }
        for label in [
            "Set Download Folder",
            "Check for Updates",
            "Update yt-dlp, Deno & FFmpeg",
            "Close",
        ] {
            let (_, clip, bounds) = text_shapes
                .iter()
                .find(|(text, _, _)| text == label)
                .unwrap_or_else(|| {
                    panic!("settings action missing: {label}; rendered: {text_shapes:?}")
                });
            assert!(
                screen.contains_rect(*bounds),
                "{label} lies outside the window: {bounds:?}"
            );
            assert!(
                clip.expand(1.0).contains_rect(*bounds),
                "{label} is clipped: {bounds:?} in {clip:?}"
            );
        }
        let check = text_shapes
            .iter()
            .find(|(text, _, _)| text == "Check for Updates")
            .unwrap()
            .2;
        let tools = text_shapes
            .iter()
            .find(|(text, _, _)| text == "Update yt-dlp, Deno & FFmpeg")
            .unwrap()
            .2;
        assert!(
            (check.center().y - tools.center().y).abs() < 2.0,
            "update actions should share a row"
        );
    }
}
