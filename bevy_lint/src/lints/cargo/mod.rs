use crate::{declare_bevy_lint, declare_bevy_lint_pass};
use cargo_metadata::MetadataCommand;
use clippy_utils::{diagnostics::span_lint, sym};
use rustc_lint::{LateContext, LateLintPass};
use rustc_session::utils::was_invoked_from_cargo;
use rustc_span::{BytePos, Pos, SourceFile, Span, Symbol, SyntaxContext};
use serde::Deserialize;
use std::{collections::BTreeMap, ops::Range};
use toml::Spanned;

pub mod duplicate_bevy_dependencies;

declare_bevy_lint! {
    pub DUPLICATE_BEVY_DEPENDENCIES,
    CORRECTNESS,
    "duplicate bevy dependencies",
}

declare_bevy_lint_pass! {
    pub Cargo => [DUPLICATE_BEVY_DEPENDENCIES.lint],
    @default = {
        bevy: Symbol = sym!(bevy),
    },
}

impl LateLintPass<'_> for Cargo {
    fn check_crate(&mut self, cx: &LateContext<'_>) {
        // If rustc was not launched by cargo, skip all cargo based lints
        if !was_invoked_from_cargo() {
            return;
        }

        match MetadataCommand::new().exec() {
            Ok(metadata) => {
                duplicate_bevy_dependencies::check(cx, &metadata, self.bevy);
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

#[derive(Deserialize, Debug)]
struct CargoToml {
    dependencies: BTreeMap<Spanned<String>, Spanned<toml::Value>>,
}

fn toml_span(range: Range<usize>, file: &SourceFile) -> Span {
    Span::new(
        file.start_pos + BytePos::from_usize(range.start),
        file.start_pos + BytePos::from_usize(range.end),
        SyntaxContext::root(),
        None,
    )
}
