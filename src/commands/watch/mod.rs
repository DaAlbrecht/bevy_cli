use tracing::info;

pub use self::args::*;
use crate::{commands::watch::observer::AppObserver, external_cli::cargo};

mod args;

pub(crate) mod builder;
pub(crate) mod message;
pub(crate) mod observer;
pub(crate) mod web_server;

#[tokio::main(flavor = "current_thread")]
#[allow(clippy::missing_panics_doc)]
pub async fn watch(args: &WatchArgs) -> anyhow::Result<()> {
    let metadata = cargo::metadata::metadata()?;

    let mut app_observer = AppObserver::start(&args.run_args, &metadata)?;

    loop {
        let msg = app_observer.wait().await;
        match msg {
            message::Message::FilesChanged { files } => {
                if !files.is_empty() {
                    let _ = app_observer.handle_file_changes(files);
                }
            }
            message::Message::BuildUpdate(build_update) => match build_update {
                message::BuilderUpdate::BuildStartet => info!("build_started"),
                message::BuilderUpdate::BuildFinished => {
                    app_observer.builder.open()?;
                    info!("build finished");
                }
                message::BuilderUpdate::ReloadWeb => {
                    if let Some(server) = app_observer.server.as_ref() {
                        server.reload();
                    }
                }
                message::BuilderUpdate::ConfigChange(config) => {
                    info!("update `bevy_cli` config:\n{}", config);
                }
            },
        }
    }
}
