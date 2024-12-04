use rustc_span::{SourceFile, Span, SyntaxContext};

pub mod duplicate_bevy_dependencies;

pub(crate) fn toml_span(file: &SourceFile) -> Span {
    Span::new(file.start_pos, file.start_pos, SyntaxContext::root(), None)
}

struct CargoToml {}
