use clap::Args;

use crate::commands::run::RunArgs;

#[derive(Debug, Args, Clone)]
pub struct WatchArgs {
    #[clap(flatten)]
    pub run_args: RunArgs,
}
