// A convenience feature used in `find_bevy_rlib()` that lets you chain multiple `if let`
// statements together with `&&`.
#![feature(let_chains)]

use serde::Deserialize;
use std::{
    env::{self},
    ffi::OsStr,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use ui_test::{
    color_eyre::{self, eyre::ensure},
    run_tests, status_emitter, CommandBuilder, Config,
};

// This is set by `build.rs`. It is the version specified in `rust-toolchain.toml`.
const RUST_TOOLCHAIN_CHANNEL: &str = env!("RUST_TOOLCHAIN_CHANNEL");
const DRIVER_STEM: &str = "../target/debug/bevy_lint_driver";

fn main() {
    run_ui();
    run_ui_cargo();
}

fn run_ui() {
    let config = base_config("ui").unwrap();
    run_tests(config).unwrap();
}

/// This [`Config`] will run the `bevy_lint` command for all paths that end in `Cargo.toml`
/// # Example:
/// ```bash
/// bevy_lint" "--quiet" "--target-dir"
/// "../target/ui/0/tests/ui-cargo/duplicate_bevy_dependencies/fail" "--manifest-path"
/// "tests/ui-cargo/duplicate_bevy_dependencies/fail/Cargo.toml"```
fn run_ui_cargo() {
    let mut config = base_config("ui-cargo").unwrap();

    let defaults = config.comment_defaults.base();
    // The driver returns a '101' on error.
    // This allows for any status code to be considered a success.
    defaults.exit_status = None.into();

    defaults.require_annotations = None.into();

    // This sets the '--manifest-path' flag
    config.program.input_file_flag = CommandBuilder::cargo().input_file_flag;
    config.program.out_dir_flag = CommandBuilder::cargo().out_dir_flag;
    // Do not print cargo log messages
    config.program.args = vec!["--quiet".into(), "--color".into(), "never".into()];

    let current_exe_path = env::current_exe().unwrap();
    let deps_path = current_exe_path.parent().unwrap();
    let profile_path = deps_path.parent().unwrap();

    // Specify the binary to use when executing tests with this `Config`
    config.program.program = profile_path.join(if cfg!(windows) {
        "bevy_lint_driver.exe"
    } else {
        "bevy_lint_driver"
    });

    config.program.program.set_file_name(if cfg!(windows) {
        "bevy_lint.exe"
    } else {
        "bevy_lint"
    });

    // this clears the default `--edition` flag
    config.comment_defaults.base().custom.clear();

    // Run this `Config` for all paths that end with `Cargo.toml` resulting
    // only in the `Cargo` lints.
    ui_test::run_tests_generic(
        vec![config],
        |path, config| {
            path.ends_with("Cargo.toml")
                .then(|| ui_test::default_any_file_filter(path, config))
        },
        |_config, _file_contents| {},
        status_emitter::Text::from(ui_test::Format::Pretty),
    )
    .unwrap();
}

/// Generates a custom [`Config`] for `bevy_lint`'s UI tests.
fn base_config(test_dir: &str) -> color_eyre::Result<Config> {
    // The path to the `bevy_lint_driver` executable, relative from inside the `bevy_lint` folder.
    // We use `with_extension()` to potentially add the `.exe` suffix, if on Windows.
    let driver_path = Path::new(DRIVER_STEM).with_extension(env::consts::EXE_EXTENSION);

    ensure!(
        driver_path.is_file(),
        "`bevy_lint_driver` could not be found at {}, make sure to build it with `cargo build -p bevy_lint --bin bevy_lint_driver`.",
        driver_path.display(),
    );

    let config = Config {
        // When `host` is `None`, `ui_test` will attempt to auto-discover the host by calling
        // `program -vV`. Unfortunately, `bevy_lint_driver` does not yet support the version flag,
        // so we manually specify the host as an empty string. This means that, for now, host-
        // specific configuration in UI tests will not work.
        host: Some(String::new()),
        program: CommandBuilder {
            // We call `rustup run` to setup the proper environmental variables, so that
            // `bevy_lint_driver` can link to `librustc_driver.so`.
            program: "rustup".into(),
            args: vec![
                "run".into(),
                RUST_TOOLCHAIN_CHANNEL.into(),
                driver_path.into(),
                // `bevy_lint_driver` expects the first argument to be the path to `rustc`.
                "rustc".into(),
                // This is required so that `ui_test` can parse warnings and errors.
                "--error-format=json".into(),
                // These two lines tell `rustc` to search in `target/debug/deps` for dependencies.
                // This is required for UI tests to import `bevy`.
                "-L".into(),
                "all=../target/debug/deps".into(),
                // Make the `bevy` crate directly importable from the UI tests.
                format!("--extern=bevy={}", find_bevy_rlib()?.display()).into(),
            ],
            out_dir_flag: Some("--out-dir".into()),
            input_file_flag: None,
            envs: Vec::new(),
            cfg_flag: Some("--print=cfg".into()),
        },
        out_dir: PathBuf::from("../target/ui"),
        ..Config::rustc(Path::new("tests").join(test_dir))
    };

    Ok(config)
}

/// An artifact message printed to stdout by Cargo.
///
/// This only deserializes the fields necessary to run UI tests, the rest of skipped.
///
/// See <https://doc.rust-lang.org/cargo/reference/external-tools.html#artifact-messages> for more
/// information on the exact format.
#[derive(Deserialize, Debug)]
#[serde(rename = "compiler-artifact", tag = "reason")]
struct ArtifactMessage<'a> {
    #[serde(borrow)]
    target: ArtifactTarget<'a>,

    #[serde(borrow)]
    filenames: Vec<&'a Path>,
}

/// The `"target"` field of an [`ArtifactMessage`].
#[derive(Deserialize, Debug)]
struct ArtifactTarget<'a> {
    name: &'a str,

    #[serde(borrow)]
    kind: Vec<&'a str>,
}

/// Tries to find the path to `libbevy.rlib` that UI tests import.
///
/// `bevy` is a dev-dependency, and as such is only built for tests and examples. We can force it
/// to be built by calling `cargo build --test=ui --message-format=json`, then scan the printed
/// JSON for the artifact message with the path to `libbevy.rlib`.
///
/// The reason we specify `--extern bevy=PATH` instead of just `--extern bevy` is because `rustc`
/// will fail to compile if multiple `libbevy.rlib` files are found, which usually is the case.
fn find_bevy_rlib() -> color_eyre::Result<PathBuf> {
    // `bevy` is a dev-dependency, so building a test will require it to be built as well.
    let output = Command::new("cargo")
        .arg("build")
        .arg("--test=ui")
        .arg("--message-format=json")
        // Show error messages to the user for easier debugging.
        .stderr(Stdio::inherit())
        .output()?;

    ensure!(output.status.success(), "`cargo build --test=ui` failed.");

    // It's theoretically possible for there to be multiple messages about building `libbevy.rlib`.
    // We support this, but optimize for just 1 message.
    let mut messages = Vec::with_capacity(1);

    // Convert the `stdout` to a string, replacing invalid characters with `�`.
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Iterate over each line in stdout, trying to deserialize it from JSON.
    for line in stdout.lines() {
        if let Ok(message) = serde_json::from_str::<ArtifactMessage>(line)
            // If the message passes the following conditions, it's probably the one we want.
            && message.target.name == "bevy"
            && message.target.kind.contains(&"lib")
        {
            messages.push(message);
        }
    }

    ensure!(
        messages.len() == 1,
        "More than one `libbevy.rlib` was built for UI tests. Please ensure there is not more than 1 version of Bevy in `Cargo.lock`.",
    );

    // The message usually has multiple files, often `libbevy.rlib` and `libbevy.rmeta`. Filter
    // through these to find the `rlib`.
    let rlib = messages[0]
        .filenames
        .iter()
        .find(|p| p.extension() == Some(OsStr::new("rlib")))
        .expect("`libbevy.rlib` not found within artifact message filenames.");

    Ok(rlib.to_path_buf())
}
