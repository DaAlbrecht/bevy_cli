use std::path::PathBuf;

use crate::config::CliConfig;

#[derive(Debug)]
pub enum Message {
    FilesChanged { files: Vec<PathBuf> },
    BuildUpdate(BuilderUpdate),
}

#[derive(Debug, PartialEq)]
pub enum BuilderUpdate {
    BuildStartet,
    BuildFinished,
    ReloadWeb,
    ConfigChange(CliConfig),
}
