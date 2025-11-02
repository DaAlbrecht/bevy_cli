use std::{
    env,
    path::PathBuf,
    process::{Child, Command},
};

use cargo_metadata::Metadata;
use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

use crate::{
    bin_target::BinTarget,
    commands::{
        build::{BuildArgs, BuildSubcommands},
        watch::message::BuilderUpdate,
    },
    config::CliConfig,
    external_cli::cargo,
    web::build::build_web,
};

pub struct AppBuilder {
    pub tx: UnboundedSender<BuilderUpdate>,
    pub rx: UnboundedReceiver<BuilderUpdate>,
    pub child: Option<Child>,
    pub build_task: JoinHandle<anyhow::Result<()>>,
    pub cli_config: CliConfig,
    pub metadata: Metadata,
    pub bin_target: BinTarget,
    pub target_dir: PathBuf,
    pub build_args: BuildArgs,
}

impl AppBuilder {
    pub fn new(
        cli_config: CliConfig,
        metadata: Metadata,
        bin_target: BinTarget,
        target_dir: PathBuf,
        build_args: BuildArgs,
    ) -> Self {
        let (tx, rx) = futures_channel::mpsc::unbounded();

        Self {
            tx,
            rx,
            child: None,
            build_task: tokio::task::spawn(std::future::pending()),
            cli_config,
            metadata,
            bin_target,
            target_dir,
            build_args,
        }
    }

    pub async fn wait(&mut self) -> BuilderUpdate {
        use futures::StreamExt;

        tokio::select! {
            Some(progress) = self.rx.next() => progress,
            _ = (&mut self.build_task) => {
                // Replace the build with an infinitely pending task so it can be polled again
                // without worrying about the task being completed
                self.build_task = tokio::task::spawn(std::future::pending());
                BuilderUpdate::BuildFinished
            }
        }
    }

    pub fn build(&mut self) {
        self.build_task.abort();
        let _ = self.tx.unbounded_send(BuilderUpdate::BuildStartet);
        self.build_task = tokio::spawn({
            let build_request = BuildRequest {
                build_args: self.build_args.clone(),
                metadata: self.metadata.clone(),
                bin_target: self.bin_target.clone(),
            };
            async move { build_request.build() }
        });
    }

    pub fn update_cli_config(&mut self) -> anyhow::Result<()> {
        let metadata = cargo::metadata::metadata()?;

        let mut config = CliConfig::for_package(
            &metadata,
            &self.bin_target.package,
            self.build_args.is_web(),
            self.build_args.is_release(),
        )?;

        // Read config files hierarchically from the current directory, merge them,
        // apply environment variables, and resolve relative paths.
        let cargo_config = cargo_config2::Config::load()?;

        config.append_cargo_config_rustflags(self.build_args.target(), &cargo_config)?;

        self.build_args.apply_config(&config);
        self.cli_config = config;
        self.metadata = metadata;

        let _ = self
            .tx
            .unbounded_send(BuilderUpdate::ConfigChange(self.cli_config.clone()));

        Ok(())
    }

    pub fn open(&mut self) -> anyhow::Result<()> {
        if self.build_args.is_web() {
            self.open_web();
        } else {
            self.open_native()?;
        }
        Ok(())
    }

    fn open_native(&mut self) -> anyhow::Result<()> {
        if let Some(child) = self.child.take() {
            _ = Command::new("kill")
                .args(["-s", "TERM", &child.id().to_string()])
                .status();
        }

        // TODO: clean this.
        let mut exe = self.target_dir.clone();
        exe.push("debug");
        exe.push(self.bin_target.package.name.as_str());
        exe.with_extension(env::consts::EXE_EXTENSION);

        let child = Command::new(exe).spawn()?;

        self.child = Some(child);
        Ok(())
    }

    fn open_web(&self) {
        let _ = self.tx.unbounded_send(BuilderUpdate::ReloadWeb);
    }
}

struct BuildRequest {
    build_args: BuildArgs,
    metadata: Metadata,
    bin_target: BinTarget,
}

impl BuildRequest {
    fn build(&self) -> anyhow::Result<()> {
        if self.build_args.is_web() {
            let mut build_args = self.build_args.clone();
            let Some(BuildSubcommands::Web(ref mut web_args)) = build_args.subcommand else {
                unreachable!(
                    "We are building for the web, so the subcommand has to be the Web variant"
                );
            };

            // We crate the axum router before crating a bundle, thus we have to create a packed
            // bundle so the served location can be known in advanced.
            web_args.create_packed_bundle = true;
            build_web(&mut build_args, &self.metadata, &self.bin_target)?;
        } else {
            let cargo_args = self.build_args.cargo_args_builder();
            cargo::build::command()
                .args(cargo_args)
                .env("RUSTFLAGS", self.build_args.rustflags())
                .ensure_status(self.build_args.auto_install())?;
        }

        Ok(())
    }
}
