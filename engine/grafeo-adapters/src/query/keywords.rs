//! Shared keyword definitions for graph query language parsers.
//!
//! Keywords are categorized as:
//! - **Common**: shared across GQL, Cypher, and SQL/PGQ
//! - **Language-specific**: unique to a single parser
//!
//! Each lexer calls `CommonKeyword::from_str` for shared keyword
//! recognition, then maps the result to its own `TokenKind`.
//! Language-specific keywords remain in each lexer.

/// A keyword shared across multiple query language parsers.
///
/// Each parser maps these to its own `TokenKind` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommonKeyword {
    // Query structure
    /// The `MATCH` keyword.
    Match,
    /// The `RETURN` keyword.
    Return,
    /// The `WHERE` keyword.
    Where,
    /// The `AS` keyword.
    As,
    /// The `DISTINCT` keyword.
    Distinct,
    /// The `WITH` keyword.
    With,
    /// The `OPTIONAL` keyword.
    Optional,

    // Ordering & pagination
    /// The `ORDER` keyword.
    Order,
    /// The `BY` keyword.
    By,
    /// The `ASC` keyword.
    Asc,
    /// The `DESC` keyword.
    Desc,
    /// The `LIMIT` keyword.
    Limit,
    /// The `SKIP` keyword.
    Skip,

    // Logical operators
    /// The `AND` keyword.
    And,
    /// The `OR` keyword.
    Or,
    /// The `NOT` keyword.
    Not,

    // Comparison
    /// The `IN` keyword.
    In,
    /// The `IS` keyword.
    Is,
    /// The `LIKE` keyword.
    Like,

    // String predicates
    /// The `STARTS` keyword.
    Starts,
    /// The `ENDS` keyword.
    Ends,
    /// The `CONTAINS` keyword.
    Contains,

    // Literals
    /// The `NULL` keyword.
    Null,
    /// The `TRUE` keyword.
    True,
    /// The `FALSE` keyword.
    False,

    // Mutation
    /// The `CREATE` keyword.
    Create,
    /// The `DELETE` keyword.
    Delete,
    /// The `SET` keyword.
    Set,
    /// The `REMOVE` keyword.
    Remove,
    /// The `MERGE` keyword.
    Merge,
    /// The `DETACH` keyword.
    Detach,
    /// The `ON` keyword.
    On,

    // Subquery / procedure
    /// The `CALL` keyword.
    Call,
    /// The `YIELD` keyword.
    Yield,
    /// The `EXISTS` keyword.
    Exists,
    /// The `UNWIND` keyword.
    Unwind,

    // Graph structure
    /// The `NODE` keyword.
    Node,
    /// The `EDGE` keyword.
    Edge,

    // Aggregate / grouping
    /// The `HAVING` keyword.
    Having,
    /// The `CASE` keyword.
    Case,
    /// The `WHEN` keyword.
    When,
    /// The `THEN` keyword.
    Then,
    /// The `ELSE` keyword.
    Else,
    /// The `END` keyword.
    End,
}

/// Generates a `map_common_keyword` function that maps [`CommonKeyword`] variants
/// to the local `TokenKind` enum.
///
/// Listed keywords produce `TokenKind::$name`; unlisted keywords fall through
/// to `TokenKind::Identifier`.
///
/// # Example
///
/// ```ignore
/// map_common_keywords! { Match, Return, Where }
/// ```
///
/// expands to:
///
/// ```ignore
/// fn map_common_keyword(kw: CommonKeyword) -> TokenKind {
///     match kw {
///         CommonKeyword::Match  => TokenKind::Match,
///         CommonKeyword::Return => TokenKind::Return,
///         CommonKeyword::Where  => TokenKind::Where,
///         _ => TokenKind::Identifier,
///     }
/// }
/// ```
#[macro_export]
macro_rules! map_common_keywords {
    ( $( $kw:ident ),+ $(,)? ) => {
        fn map_common_keyword(kw: $crate::query::keywords::CommonKeyword) -> TokenKind {
            use $crate::query::keywords::CommonKeyword;
            #[allow(unreachable_patterns)]
            match kw {
                $( CommonKeyword::$kw => TokenKind::$kw, )+
                _ => TokenKind::Identifier,
            }
        }
    };
}

impl CommonKeyword {
    /// Recognizes a keyword from its uppercase text.
    ///
    /// Returns `None` for language-specific or unrecognized identifiers.
    /// The caller should convert the input to uppercase before calling.
    #[must_use]
    pub fn from_uppercase(text: &str) -> Option<Self> {
        match text {
            // Query structure
            "MATCH" => Some(Self::Match),
            "RETURN" => Some(Self::Return),
            "WHERE" => Some(Self::Where),
            "AS" => Some(Self::As),
            "DISTINCT" => Some(Self::Distinct),
            "WITH" => Some(Self::With),
            "OPTIONAL" => Some(Self::Optional),

            // Ordering & pagination
            "ORDER" => Some(Self::Order),
            "BY" => Some(Self::By),
            "ASC" => Some(Self::Asc),
            "DESC" => Some(Self::Desc),
            "LIMIT" => Some(Self::Limit),
            "SKIP" => Some(Self::Skip),

            // Logical
            "AND" => Some(Self::And),
            "OR" => Some(Self::Or),
            "NOT" => Some(Self::Not),

            // Comparison
            "IN" => Some(Self::In),
            "IS" => Some(Self::Is),
            "LIKE" => Some(Self::Like),

            // String predicates
            "STARTS" => Some(Self::Starts),
            "ENDS" => Some(Self::Ends),
            "CONTAINS" => Some(Self::Contains),

            // Literals
            "NULL" => Some(Self::Null),
            "TRUE" => Some(Self::True),
            "FALSE" => Some(Self::False),

            // Mutation
            "CREATE" => Some(Self::Create),
            "DELETE" => Some(Self::Delete),
            "SET" => Some(Self::Set),
            "REMOVE" => Some(Self::Remove),
            "MERGE" => Some(Self::Merge),
            "DETACH" => Some(Self::Detach),
            "ON" => Some(Self::On),

            // Subquery / procedure
            "CALL" => Some(Self::Call),
            "YIELD" => Some(Self::Yield),
            "EXISTS" => Some(Self::Exists),
            "UNWIND" => Some(Self::Unwind),

            // Graph structure
            "NODE" => Some(Self::Node),
            "EDGE" => Some(Self::Edge),

            // Aggregate / grouping
            "HAVING" => Some(Self::Having),
            "CASE" => Some(Self::Case),
            "WHEN" => Some(Self::When),
            "THEN" => Some(Self::Then),
            "ELSE" => Some(Self::Else),
            "END" => Some(Self::End),

            _ => None,
        }
    }

    /// Returns true if the given uppercase text is a keyword in any parser.
    #[must_use]
    pub fn is_keyword(text: &str) -> bool {
        Self::from_uppercase(text).is_some()
    }
}

/// Unescapes backslash-escaped characters in a string literal.
///
/// Supports standard escapes (`\n`, `\r`, `\t`, `\\`, `\'`, `\"`) as well as
/// Unicode escapes: `\uXXXX` (4-hex-digit BMP) and `\UXXXXXXXX` (8-hex-digit
/// full Unicode range).
#[must_use]
pub fn unescape_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('r') => result.push('\r'),
                Some('t') => result.push('\t'),
                Some('\\') => result.push('\\'),
                Some('\'') => result.push('\''),
                Some('"') => result.push('"'),
                Some('u') => {
                    // \uXXXX: 4-hex-digit Unicode escape (BMP)
                    let hex: String = chars.by_ref().take(4).collect();
                    if hex.len() == 4 {
                        if let Some(cp) =
                            u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)
                        {
                            result.push(cp);
                        } else {
                            // Invalid codepoint (e.g. surrogate): preserve literal
                            result.push_str("\\u");
                            result.push_str(&hex);
                        }
                    } else {
                        // Not enough hex digits: preserve literal
                        result.push_str("\\u");
                        result.push_str(&hex);
                    }
                }
                Some('U') => {
                    // \UXXXXXXXX: 8-hex-digit Unicode escape (full range)
                    let hex: String = chars.by_ref().take(8).collect();
                    if hex.len() == 8 {
                        if let Some(cp) =
                            u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)
                        {
                            result.push(cp);
                        } else {
                            result.push_str("\\U");
                            result.push_str(&hex);
                        }
                    } else {
                        result.push_str("\\U");
                        result.push_str(&hex);
                    }
                }
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(ch);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_common_keywords_recognized() {
        // Query structure
        assert_eq!(
            CommonKeyword::from_uppercase("MATCH"),
            Some(CommonKeyword::Match)
        );
        assert_eq!(
            CommonKeyword::from_uppercase("RETURN"),
            Some(CommonKeyword::Return)
        );
        assert_eq!(
            CommonKeyword::from_uppercase("WHERE"),
            Some(CommonKeyword::Where)
        );

        // Logical
        assert_eq!(
            CommonKeyword::from_uppercase("AND"),
            Some(CommonKeyword::And)
        );
        assert_eq!(CommonKeyword::from_uppercase("OR"), Some(CommonKeyword::Or));
        assert_eq!(
            CommonKeyword::from_uppercase("NOT"),
            Some(CommonKeyword::Not)
        );

        // Literals
        assert_eq!(
            CommonKeyword::from_uppercase("NULL"),
            Some(CommonKeyword::Null)
        );
        assert_eq!(
            CommonKeyword::from_uppercase("TRUE"),
            Some(CommonKeyword::True)
        );
        assert_eq!(
            CommonKeyword::from_uppercase("FALSE"),
            Some(CommonKeyword::False)
        );
    }

    #[test]
    fn test_non_keywords_return_none() {
        assert_eq!(CommonKeyword::from_uppercase("FOOBAR"), None);
        assert_eq!(CommonKeyword::from_uppercase("person"), None);
        assert_eq!(CommonKeyword::from_uppercase(""), None);
    }

    #[test]
    fn test_is_keyword() {
        assert!(CommonKeyword::is_keyword("MATCH"));
        assert!(CommonKeyword::is_keyword("WHERE"));
        assert!(!CommonKeyword::is_keyword("FOOBAR"));
    }

    #[test]
    fn test_language_specific_not_common() {
        // SQL/PGQ specific
        assert_eq!(CommonKeyword::from_uppercase("SELECT"), None);
        assert_eq!(CommonKeyword::from_uppercase("FROM"), None);
        assert_eq!(CommonKeyword::from_uppercase("GRAPH_TABLE"), None);

        // Cypher specific
        assert_eq!(CommonKeyword::from_uppercase("UNION"), None);
        assert_eq!(CommonKeyword::from_uppercase("XOR"), None);

        // GQL specific
        assert_eq!(CommonKeyword::from_uppercase("VECTOR"), None);
        assert_eq!(CommonKeyword::from_uppercase("INDEX"), None);
    }

    #[test]
    fn test_all_common_keywords_covered() {
        let all_keywords = [
            "MATCH", "RETURN", "WHERE", "AS", "DISTINCT", "WITH", "OPTIONAL", "ORDER", "BY", "ASC",
            "DESC", "LIMIT", "SKIP", "AND", "OR", "NOT", "IN", "IS", "LIKE", "STARTS", "ENDS",
            "CONTAINS", "NULL", "TRUE", "FALSE", "CREATE", "DELETE", "SET", "REMOVE", "MERGE",
            "DETACH", "ON", "CALL", "YIELD", "EXISTS", "UNWIND", "NODE", "EDGE", "HAVING", "CASE",
            "WHEN", "THEN", "ELSE", "END",
        ];

        for kw in &all_keywords {
            assert!(
                CommonKeyword::from_uppercase(kw).is_some(),
                "Expected '{kw}' to be recognized as a common keyword"
            );
        }
    }

    // ==================== Unicode escape sequence tests ====================

    #[test]
    fn test_unescape_basic_escapes_unchanged() {
        assert_eq!(unescape_string(r"\n"), "\n");
        assert_eq!(unescape_string(r"\r"), "\r");
        assert_eq!(unescape_string(r"\t"), "\t");
        assert_eq!(unescape_string(r"\\"), "\\");
        assert_eq!(unescape_string(r"\'"), "'");
        assert_eq!(unescape_string(r#"\""#), "\"");
    }

    #[test]
    fn test_unescape_unicode_bmp_4digit() {
        // \u0041 = 'A'
        assert_eq!(unescape_string(r"\u0041"), "A");
        // \u00E9 = 'e' (e with acute)
        assert_eq!(unescape_string(r"\u00E9"), "\u{00E9}");
        // \u4EBA = Chinese character for 'person'
        assert_eq!(unescape_string(r"\u4EBA"), "\u{4EBA}");
        // \u0000 = null character
        assert_eq!(unescape_string(r"\u0000"), "\0");
    }

    #[test]
    fn test_unescape_unicode_full_8digit() {
        // \U0001F600 = grinning face emoji
        assert_eq!(unescape_string(r"\U0001F600"), "\u{1F600}");
        // \U00000041 = 'A' (padded)
        assert_eq!(unescape_string(r"\U00000041"), "A");
    }

    #[test]
    fn test_unescape_unicode_invalid_surrogate_preserved() {
        // \uD800 is a surrogate, not a valid scalar value
        let result = unescape_string(r"\uD800");
        assert_eq!(
            result, r"\uD800",
            "Surrogates should be preserved as literal"
        );
    }

    #[test]
    fn test_unescape_unicode_invalid_too_large_preserved() {
        // \U00110000 exceeds Unicode max (0x10FFFF)
        let result = unescape_string(r"\U00110000");
        assert_eq!(result, r"\U00110000", "Out-of-range should be preserved");
    }

    #[test]
    fn test_unescape_unicode_incomplete_hex_preserved() {
        // Only 2 hex digits instead of 4
        let result = unescape_string(r"\u00");
        assert_eq!(result, r"\u00", "Incomplete hex should be preserved");
    }

    #[test]
    fn test_unescape_unicode_mixed_with_text() {
        assert_eq!(unescape_string(r"Hello \u0041\u0042\u0043"), "Hello ABC");
    }

    #[test]
    fn test_unescape_unicode_case_insensitive_hex() {
        assert_eq!(unescape_string(r"\u00e9"), "\u{00E9}");
        assert_eq!(unescape_string(r"\u00E9"), "\u{00E9}");
    }
}
