//! Checks for multiple versions of `bevy` in the dependencies.
//!
//! # Motivation
//!
//! When different third party crates use incompatible versions of Bevy, it can lead to confusing
//! errors and type incompatibilities.

use std::{path::Path, str::FromStr};

use crate::{
    declare_bevy_lint,
    lints::cargo::{toml_span, CargoToml},
};
use cargo_metadata::{
    semver::{Error, Version, VersionReq},
    Metadata, MetadataCommand, Package,
};
use clippy_utils::{diagnostics::span_lint, find_crates, sym};
use rustc_hir::def_id::LOCAL_CRATE;
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
    let local_name = cx.tcx.crate_name(LOCAL_CRATE);

    let file = cx
        .tcx
        .sess
        .source_map()
        .load_file(Path::new("Cargo.toml"))
        .unwrap();

    let cargo_src = file.src.as_deref();

    let cargo_toml = toml::from_str::<CargoToml>(cargo_src.unwrap()).unwrap();

    let target_version = get_version_from_toml(
        cargo_toml
            .dependencies
            .get("bevy")
            .unwrap()
            .as_ref()
            .clone(),
    )
    .unwrap();

    dbg!(&target_version);

    let bevy_dependents = metadata
        .packages
        .iter()
        .flat_map(|package| {
            package
                .dependencies
                .iter()
                .filter_map(|dependency| match dependency.name.as_str() {
                    "bevy" => {
                        if package.name.as_str() == local_name.as_str() {
                            return None;
                        }
                        Some((package.name.as_str(), dependency.req.clone()))
                    }
                    _ => None,
                })
                .collect::<Vec<(&str, VersionReq)>>()
        })
        .collect::<Vec<(&str, VersionReq)>>();

    let incoherent_bevy_dependents = bevy_dependents
        .iter()
        .filter(|(_, dependent_version)| dependent_version.matches(&target_version))
        .cloned()
        .collect::<Vec<(&str, VersionReq)>>();

    incoherent_bevy_dependents
        .iter()
        .for_each(|incoherent_version| {
            dbg!(cargo_toml.dependencies.get(incoherent_version.0));
            let cargo_toml_reference = cargo_toml
                .dependencies
                .get(incoherent_version.0)
                .expect("crate to be present");
            span_lint(
                cx,
                DUPLICATE_BEVY_DEPENDENCIES.lint,
                toml_span(cargo_toml_reference.span(), &file),
                format!(
                    "Mismatching versions of `bevy` found, expected crate to be using bevy version: {}",
                    target_version.to_string()
                ),
            );
        });
}

fn get_version_from_toml(table: toml::Value) -> Result<Version, Error> {
    // TODO: make this more robust, this fails if someone uses version ranges for bevy
    match table {
        toml::Value::String(version) => Version::from_str(version.as_str()),
        toml::Value::Table(map) => {
            let version = map.get("version").expect("version field is required");
            Version::from_str(
                version
                    .as_str()
                    .expect("version field is required to be a string"),
            )
        }
        _ => panic!("impossible to hit"),
    }
}
