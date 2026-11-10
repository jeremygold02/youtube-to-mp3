use crate::update::UpdateInfo;
use std::sync::mpsc::Sender;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Progress,
    Success,
    Warning,
    Error,
}

#[derive(Debug)]
pub enum Event {
    Log(String, Level),
    Finished,
    ReleaseChecked {
        result: Result<UpdateInfo, String>,
        silent_if_current: bool,
    },
    UpdateInstalled(Result<(), String>),
}

pub fn log(sender: &Sender<Event>, message: impl Into<String>, level: Level) {
    let _ = sender.send(Event::Log(message.into(), level));
}
