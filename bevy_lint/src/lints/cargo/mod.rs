use std::{collections::BTreeMap, ops::Range};

use rustc_span::{BytePos, Pos, SourceFile, Span, SyntaxContext};
use serde::Deserialize;
use toml::Spanned;

pub mod duplicate_bevy_dependencies;

pub(crate) fn toml_span(range: Range<usize>, file: &SourceFile) -> Span {
    Span::new(
        file.start_pos + BytePos::from_usize(range.start),
        file.start_pos + BytePos::from_usize(range.end),
        SyntaxContext::root(),
        None,
    )
}

#[derive(Deserialize, Debug)]
pub(crate) struct CargoToml {
    dependencies: BTreeMap<Spanned<String>, Spanned<toml::Value>>,
}
