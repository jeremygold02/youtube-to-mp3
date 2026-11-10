use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const APP_DIRECTORY: &str = "YouTube to MP3";
const PROTOTYPE_DIRECTORY: &str = "YouTube Downloader Rust Prototype";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub download_path: String,
}

pub fn data_dir() -> Result<PathBuf, String> {
    let root = std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Windows local application data folder was not found.".to_owned())?;
    Ok(PathBuf::from(root).join(APP_DIRECTORY))
}

pub fn load(dir: &Path) -> Result<Config, String> {
    let original = original_config_path();
    let prototype = previous_data_dir(dir).map(|path| path.join("config.json"));
    load_with_sources(dir, prototype.as_deref(), original.as_deref())
}

pub fn previous_data_dir(dir: &Path) -> Option<PathBuf> {
    dir.parent().map(|parent| parent.join(PROTOTYPE_DIRECTORY))
}

fn load_with_sources(
    dir: &Path,
    prototype: Option<&Path>,
    original: Option<&Path>,
) -> Result<Config, String> {
    let path = dir.join("config.json");
    match fs::read(&path) {
        Ok(data) => serde_json::from_slice(&data)
            .map_err(|error| format!("Could not read settings: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let prototype_folder = if let Some(prototype) = prototype {
                match fs::read(prototype) {
                    Ok(data) => {
                        let settings: Config = serde_json::from_slice(&data).map_err(|error| {
                            format!("Could not read previous prototype settings: {error}")
                        })?;
                        (!settings.download_path.trim().is_empty())
                            .then(|| PathBuf::from(settings.download_path))
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => {
                        return Err(format!(
                            "Could not read previous prototype settings: {error}"
                        ))
                    }
                }
            } else {
                None
            };
            let Some(folder) =
                prototype_folder.or_else(|| original.and_then(read_original_download_dir))
            else {
                return Ok(Config::default());
            };
            let config = Config {
                download_path: folder.to_string_lossy().into_owned(),
            };
            let temporary = write_temporary(dir, &config)?;
            match temporary.persist_noclobber(&path) {
                Ok(_) => Ok(config),
                Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let data = fs::read(&path)
                        .map_err(|error| format!("Could not read settings: {error}"))?;
                    serde_json::from_slice(&data)
                        .map_err(|error| format!("Could not read settings: {error}"))
                }
                Err(error) => Err(format!("Could not save settings: {}", error.error)),
            }
        }
        Err(error) => Err(format!("Could not read settings: {error}")),
    }
}

fn write_temporary(dir: &Path, config: &Config) -> Result<tempfile::NamedTempFile, String> {
    fs::create_dir_all(dir)
        .map_err(|error| format!("Could not create settings folder: {error}"))?;
    let mut temporary = tempfile::NamedTempFile::new_in(dir)
        .map_err(|error| format!("Could not write settings: {error}"))?;
    serde_json::to_writer_pretty(&mut temporary, config)
        .map_err(|error| format!("Could not write settings: {error}"))?;
    temporary
        .flush()
        .map_err(|error| format!("Could not write settings: {error}"))?;
    Ok(temporary)
}

pub fn save(dir: &Path, config: &Config) -> Result<(), String> {
    let temporary = write_temporary(dir, config)?;
    temporary
        .persist(dir.join("config.json"))
        .map_err(|error| format!("Could not save settings: {}", error.error))?;
    Ok(())
}

fn original_config_path() -> Option<PathBuf> {
    let profile = std::env::var_os("USERPROFILE")?;
    Some(
        PathBuf::from(profile)
            .join("Documents")
            .join("ytmp3-config.json"),
    )
}

fn read_original_download_dir(path: &Path) -> Option<PathBuf> {
    let data = fs::read(path).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&data).ok()?;
    let folder = value.get("download_path")?.as_str()?.trim();
    if folder.is_empty() {
        None
    } else {
        Some(PathBuf::from(folder))
    }
}

pub fn original_download_dir() -> Option<PathBuf> {
    original_config_path().and_then(|path| read_original_download_dir(&path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_only_in_the_given_directory() {
        let parent = tempfile::tempdir().unwrap();
        let dir = parent.path().join("prototype");
        assert!(load_with_sources(&dir, None, None)
            .unwrap()
            .download_path
            .is_empty());
        let config = Config {
            download_path: "a folder".into(),
        };
        save(&dir, &config).unwrap();
        assert_eq!(
            load_with_sources(&dir, None, None).unwrap().download_path,
            "a folder"
        );
        assert!(!parent.path().join("ytmp3-config.json").exists());
    }

    #[test]
    fn migrates_original_folder_once_without_modifying_original() {
        let parent = tempfile::tempdir().unwrap();
        let dir = parent.path().join("prototype");
        let original = parent.path().join("ytmp3-config.json");
        let original_contents = br#"{"download_path":"C:\\Music","other":"keep"}"#;
        fs::write(&original, original_contents).unwrap();
        assert_eq!(
            load_with_sources(&dir, None, Some(&original))
                .unwrap()
                .download_path,
            r"C:\Music"
        );
        assert_eq!(fs::read(&original).unwrap(), original_contents);
        fs::write(&original, br#"{"download_path":"different folder"}"#).unwrap();
        assert_eq!(
            load_with_sources(&dir, None, Some(&original))
                .unwrap()
                .download_path,
            r"C:\Music"
        );
    }

    #[test]
    fn existing_prototype_settings_win_and_invalid_settings_are_preserved() {
        let parent = tempfile::tempdir().unwrap();
        let dir = parent.path().join("prototype");
        let original = parent.path().join("ytmp3-config.json");
        fs::write(&original, br#"{"download_path":"old folder"}"#).unwrap();
        save(
            &dir,
            &Config {
                download_path: "new folder".into(),
            },
        )
        .unwrap();
        assert_eq!(
            load_with_sources(&dir, None, Some(&original))
                .unwrap()
                .download_path,
            "new folder"
        );
        fs::write(dir.join("config.json"), b"invalid json").unwrap();
        assert!(load_with_sources(&dir, None, Some(&original)).is_err());
        assert_eq!(fs::read(dir.join("config.json")).unwrap(), b"invalid json");
    }

    #[test]
    fn missing_or_invalid_original_does_not_create_settings() {
        let parent = tempfile::tempdir().unwrap();
        let dir = parent.path().join("prototype");
        let original = parent.path().join("ytmp3-config.json");
        for content in [
            None,
            Some(&b"invalid json"[..]),
            Some(&b"{\"download_path\": \" \"}"[..]),
        ] {
            if let Some(content) = content {
                fs::write(&original, content).unwrap();
            }
            assert!(load_with_sources(&dir, None, Some(&original))
                .unwrap()
                .download_path
                .is_empty());
            assert!(!dir.exists());
        }
    }

    #[test]
    fn previous_rust_settings_take_precedence_over_python_settings() {
        let parent = tempfile::tempdir().unwrap();
        let dir = parent.path().join("current");
        let prototype = parent.path().join("previous.json");
        let original = parent.path().join("ytmp3-config.json");
        let previous = br#"{"download_path":"prototype folder"}"#;
        fs::write(&prototype, previous).unwrap();
        fs::write(&original, br#"{"download_path":"python folder"}"#).unwrap();
        assert_eq!(
            load_with_sources(&dir, Some(&prototype), Some(&original))
                .unwrap()
                .download_path,
            "prototype folder"
        );
        assert_eq!(fs::read(&prototype).unwrap(), previous);
        fs::write(&prototype, b"invalid json").unwrap();
        let another_dir = parent.path().join("another");
        assert!(load_with_sources(&another_dir, Some(&prototype), Some(&original)).is_err());
        assert!(!another_dir.exists());
    }
}
