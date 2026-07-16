use mecojoni_core::{SourceFile, SourceId, parse_module};

/// Conservative semantic formatter contract (`format/1`).
///
/// The initial formatter validates the complete source and returns it byte for byte.
/// This is deliberately useful as a safe editor boundary before style rewrites are
/// standardized: comments, quoted edge spaces, and block chomp behavior cannot drift.
///
/// # Errors
///
/// Returns the parser's diagnostics when the input is not valid v2 source.
pub fn format_source(source: &str, name: &str) -> Result<String, mecojoni_core::MecoError> {
    let file = SourceFile::new(SourceId::new(0), name, source);
    parse_module(&file)?;
    Ok(source.to_string())
}

#[cfg(test)]
mod tests {
    use super::format_source;

    #[test]
    fn formatter_preserves_comments_blocks_and_literal_edge_spaces_exactly() {
        let source = "---\nmeco: 2\nmodule: fmt\nentry: line\n---\n\n<!-- keep -->\n# line\n- \" edge \"@tail\n\n# tail\n- |raw-\n  $literal\n";
        assert_eq!(format_source(source, "fmt.meco.md").unwrap(), source);
    }
}
