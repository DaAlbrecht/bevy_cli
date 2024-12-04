//! Checks for multiple versions of `bevy` in the dependencies.
//!
//! # Motivation
//!
//! When different third party crates use incompatible versions of Bevy, it can lead to confusing
//! errors and type incompatibilities.

use std::path::Path;

use crate::{declare_bevy_lint, lints::cargo::toml_span};
use cargo_metadata::{semver::VersionReq, Metadata, MetadataCommand, Package};
use clippy_utils::{diagnostics::span_lint, find_crates, sym};
use rustc_lint::{LateContext, LateLintPass};
use rustc_session::impl_lint_pass;
use rustc_span::Symbol;

declare_bevy_lint! {
    pub DUPLICATE_BEVY_DEPENDENCIES,
    CORRECTNESS,
    "duplicate bevy dependencies",
}

pub(crate) struct DuplicateBevyDependencies {
    bevy_symbol: Symbol,
}

impl Default for DuplicateBevyDependencies {
    fn default() -> Self {
        Self {
            bevy_symbol: sym!(bevy),
        }
    }
}

impl_lint_pass! {
     DuplicateBevyDependencies => [DUPLICATE_BEVY_DEPENDENCIES.lint]
}

impl<'tcx> LateLintPass<'tcx> for DuplicateBevyDependencies {
    fn check_crate(&mut self, cx: &LateContext<'tcx>) {
        let bevy_crates = find_crates(cx.tcx, self.bevy_symbol);

        if bevy_crates.len() > 1 {
            match MetadataCommand::new().exec() {
                Ok(metadata) => {
                    check(cx, &metadata);
                }
                Err(e) => {
                    span_lint(
                        cx,
                        DUPLICATE_BEVY_DEPENDENCIES.lint,
                        rustc_span::DUMMY_SP,
                        format!("could not read cargo metadata: {e}"),
                    );
                }
            }
        }
    }
}

fn check(cx: &LateContext<'_>, metadata: &Metadata) {
    let file = cx
        .tcx
        .sess
        .source_map()
        .load_file(Path::new("Cargo.toml"))
        .unwrap();

    let cargo_src = file.src.as_deref();

    let toml_schmomel = toml::from_str::<String>(cargo_src.unwrap());

    dbg!(toml_schmomel);

    let bevy_crate = metadata
        .packages
        .iter()
        .filter(|package| package.name == "bevy")
        .cloned()
        .collect::<Vec<Package>>();

    let bevy_dependents = metadata
        .packages
        .iter()
        .map(|package| {
            package
                .dependencies
                .iter()
                .filter_map(|dependency| match dependency.name.as_str() {
                    "bevy" => Some((package.name.as_str(), dependency.req.clone())),
                    _ => None,
                })
                .collect::<Vec<(&str, VersionReq)>>()
        })
        .flatten()
        .collect::<Vec<(&str, VersionReq)>>();

    println!("bevy_dependents: {:?}", bevy_dependents);

    bevy_crate.iter().for_each(|bevy| {
        println!("version: {}", bevy.version);
    });

    span_lint(
        cx,
        DUPLICATE_BEVY_DEPENDENCIES.lint,
        toml_span(&file),
        "Multiple versions of `bevy` found",
    );
}
