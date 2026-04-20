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
}
