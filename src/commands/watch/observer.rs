use std::path::PathBuf;

use cargo_metadata::Metadata;
use futures::StreamExt;
use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender};
use notify::{EventKind, Watcher};

use crate::{
    bin_target::select_run_binary,
    commands::{
        run::RunArgs,
        watch::{builder::AppBuilder, message::Message, web_server::WebServer},
    },
    config::CliConfig,
};

pub struct AppObserver {
    pub builder: AppBuilder,
    pub server: Option<WebServer>,
    pub watcher: Box<dyn Watcher>,
    pub _watcher_tx: UnboundedSender<notify::Event>,
    pub watcher_rx: UnboundedReceiver<notify::Event>,
}

impl AppObserver {
    pub fn start(args: &RunArgs, metadata: &Metadata) -> anyhow::Result<Self> {
        // Create notify watcher
        let (watcher_tx, watcher_rx) = futures_channel::mpsc::unbounded();

        let watcher = create_watcher(watcher_tx.clone());

        let bin_target = select_run_binary(
            metadata,
            args.cargo_args.package_args.package.as_deref(),
            args.cargo_args.target_args.bin.as_deref(),
            args.cargo_args.target_args.example.as_deref(),
            args.target().as_deref(),
            args.profile(),
        )?;

        let mut cli_config = CliConfig::for_package(
            metadata,
            &bin_target.package,
            args.is_web(),
            args.is_release(),
        )?;

        // Read config files hierarchically from the current directory, merge them,
        // apply environment variables, and resolve relative paths.
        let cargo_config = cargo_config2::Config::load()?;
        cli_config.append_cargo_config_rustflags(args.target(), &cargo_config)?;

        let mut builder = AppBuilder::new(
            cli_config,
            metadata.clone(),
            bin_target,
            metadata.target_directory.as_std_path().to_owned(),
            args.clone().into(),
        );

        // Build initial app without waiting on a change from a watched file first.
        builder.build();

        // create the axum server if we are serving a web app.
        let server = args.is_web().then_some(WebServer::start(&builder)?);

        let mut app_observer = Self {
            watcher,
            _watcher_tx: watcher_tx,
            watcher_rx,
            builder,
            server,
        };

        // Start watching for changes.
        app_observer.watch_files()?;

        Ok(app_observer)
    }

    pub async fn wait(&mut self) -> Message {
        let builder_wait = self.builder.wait();
        let watcher_wait = self.watcher_rx.next();

        tokio::select! {
            builder_update = builder_wait =>{
                Message::BuildUpdate(builder_update)
            },
            watcher_event = watcher_wait => {
                let mut changes: Vec<_> = watcher_event.into_iter().collect();

                while let Some(event) = self.watcher_rx.try_next().ok().flatten() {
                    changes.push(event);
                }

                let mut files: Vec<PathBuf> = vec![];

                for event in changes.drain(..) {
                    for path in event.paths {
                        // There are events for files that do not contain any data, thus only add files that
                        // have some data to the changed list.

                        if let Ok(metadata) = std::fs::metadata(&path)
                            && metadata.len() == 0
                        {
                            continue;
                        }
                        files.push(path);
                    }
                }

                Message::FilesChanged { files }
            }
        }
    }

    pub fn handle_file_changes(&mut self, files: Vec<PathBuf>) -> anyhow::Result<()> {
        let mut needs_rebuild = false;
        for file in files {
            // If the file path matches the current package path, we have to check if we
            // have to update the cli config.
            if file == self.builder.bin_target.package.manifest_path {
                needs_rebuild = true;
                self.builder.update_cli_config()?;
            }

            if let Some(extension) = file.extension()
                && extension == "rs"
            {
                needs_rebuild = true;
            }
        }

        // Only rebuild if either a rust file or the cli config has changes. This prevents
        // the need to exclude the target directory.
        if needs_rebuild {
            self.builder.build();
        }

        Ok(())
    }

    pub fn watch_files(&mut self) -> anyhow::Result<()> {
        // TODO: Add assets directory.
        let paths = paths_to_watch(&self.builder.metadata);

        for path in &paths {
            self.watcher.watch(path, notify::RecursiveMode::Recursive)?;
        }

        // Watch the Cargo.toml for cli config changes.
        self.watcher.watch(
            self.builder.bin_target.package.manifest_path.as_std_path(),
            notify::RecursiveMode::NonRecursive,
        )?;

        Ok(())
    }
}

fn create_watcher(watcher_tx: UnboundedSender<notify::Event>) -> Box<dyn Watcher> {
    let handler = move |info: notify::Result<notify::Event>| {
        if let Ok(event) = info {
            // TODO: narrow down the events.
            let notify_on_event = matches!(event.kind, |EventKind::Modify(_)| EventKind::Create(_)
                | EventKind::Remove(_));

            if notify_on_event {
                let _ = watcher_tx.unbounded_send(event);
            }
        }
    };

    // TODO: switch to different watcher, this has known issues with WSL: https://github.com/notify-rs/notify/issues/254
    Box::new(notify::recommended_watcher(handler).expect("failed to create notify watcher"))
}

// Returns a list of Paths, that the filewatcher should watch for (ignoring .cargo).
fn paths_to_watch(metadata: &Metadata) -> Vec<PathBuf> {
    metadata
        .workspace_packages()
        .into_iter()
        .filter_map(|package| {
            if package
                .manifest_path
                .components()
                .any(|c| c.as_str() == ".cargo")
            {
                None
            } else {
                Some(
                    package
                        .manifest_path
                        .parent()
                        .unwrap()
                        .to_path_buf()
                        .into_std_path_buf(),
                )
            }
        })
        .collect()
}
