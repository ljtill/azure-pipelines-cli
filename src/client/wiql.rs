//! WIQL (Work Item Query Language) helpers for safe query construction.

/// Escapes a string for safe interpolation into a WIQL single-quoted literal.
///
/// WIQL string literals use single quotes and escape an embedded single quote
/// by doubling it (`'` → `''`). Apply this to any user-controlled value before
/// interpolating it into a WIQL string.
pub(crate) fn wiql_escape(s: &str) -> String {
    s.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::wiql_escape;

    #[test]
    fn escapes_empty_string() {
        assert_eq!(wiql_escape(""), "");
    }

    #[test]
    fn passes_through_when_no_quotes() {
        assert_eq!(wiql_escape("MyProject"), "MyProject");
    }

    #[test]
    fn doubles_single_quote() {
        assert_eq!(wiql_escape("it's mine"), "it''s mine");
    }

    #[test]
    fn doubles_every_single_quote() {
        assert_eq!(wiql_escape("'a'b'c'"), "''a''b''c''");
    }

    #[test]
    fn property_style_escapes_sampled_wiql_literals() {
        let samples = [
            "",
            "plain",
            "it's mine",
            "''",
            "'leading",
            "trailing'",
            "a'b'c",
            "line\nbreak",
            "tab\tvalue",
            "emoji 🦀 'quote'",
            "[System.Title] = 'x' OR '1' = '1'",
        ];

        for input in samples {
            assert_wiql_escape_properties(input);
        }
    }

    #[test]
    fn property_style_escapes_generated_fragment_combinations() {
        let fragments = [
            "",
            "'",
            "''",
            "name",
            " space ",
            "line\nbreak",
            "tab\tvalue",
            "🦀",
            "[System.Title]",
            "; DROP?",
        ];

        for &left in &fragments {
            for &right in &fragments {
                let input = format!("{left}middle{right}");
                assert_wiql_escape_properties(&input);
            }
        }
    }

    fn assert_wiql_escape_properties(input: &str) {
        let escaped = wiql_escape(input);
        let quote_count = input.chars().filter(|&ch| ch == '\'').count();

        assert_eq!(
            escaped.chars().filter(|&ch| ch == '\'').count(),
            quote_count * 2,
            "escaped WIQL should double every quote for {input:?}"
        );
        assert_eq!(
            wiql_unescape(&escaped),
            input,
            "escaped WIQL should round trip for {input:?}"
        );
    }

    fn wiql_unescape(escaped: &str) -> String {
        let mut output = String::with_capacity(escaped.len());
        let mut chars = escaped.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '\'' {
                assert_eq!(
                    chars.next(),
                    Some('\''),
                    "escaped WIQL literal contains an unpaired quote: {escaped:?}"
                );
                output.push('\'');
            } else {
                output.push(ch);
            }
        }

        output
    }
}
