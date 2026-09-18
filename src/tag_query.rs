use anyhow::{Result, bail};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagExpr {
    Tag(String),
    And(Box<TagExpr>, Box<TagExpr>),
    Or(Box<TagExpr>, Box<TagExpr>),
    Not(Box<TagExpr>),
}

impl TagExpr {
    /// Tags are already lowercased both here (at parse time) and at storage
    /// time (src/store.rs), so direct equality is correct.
    pub fn eval(&self, tags: &[String]) -> bool {
        match self {
            TagExpr::Tag(t) => tags.iter().any(|x| x == t),
            TagExpr::And(a, b) => a.eval(tags) && b.eval(tags),
            TagExpr::Or(a, b) => a.eval(tags) || b.eval(tags),
            TagExpr::Not(a) => !a.eval(tags),
        }
    }

    /// Builds an AND-chain from a flat list of required tags — the AND-only
    /// special case used by memory_search's `tags` param (a plain JSON array
    /// has no way to express OR/NOT, so this is the only combinator it needs).
    pub fn and_all(tags: &[String]) -> Option<TagExpr> {
        let mut iter = tags.iter();
        let first = iter.next()?;
        let mut expr = TagExpr::Tag(first.to_lowercase());
        for t in iter {
            expr = TagExpr::And(Box::new(expr), Box::new(TagExpr::Tag(t.to_lowercase())));
        }
        Some(expr)
    }
}

/// True if `s` looks like an attempted tag expression. Callers use this to
/// decide whether to call `parse` or fall back to normal title/FTS
/// resolution — see the detection rule in the design spec.
pub fn looks_like_tag_expr(s: &str) -> bool {
    let t = s.trim();
    t.starts_with("tag:") || t.starts_with("!tag:") || t.starts_with('(')
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    And,
    Or,
    Not,
    LParen,
    RParen,
    Tag(String),
}

/// Upper bound on the length of a tag expression. Expressions arrive from
/// unauthenticated HTTP query strings (`GET /api/v1/search?q=`), so the
/// parser must have a hard ceiling on the work it does.
pub const MAX_EXPR_LEN: usize = 1024;

/// Upper bound on nesting depth (`!` prefixes and parentheses). The parser
/// is recursive descent; without this cap a long run of `!` overflows the
/// stack and aborts the whole process, which is a one-request denial of
/// service against the HTTP server. Real expressions nest two or three
/// levels deep.
pub const MAX_EXPR_DEPTH: usize = 32;

pub fn parse(s: &str) -> Result<TagExpr> {
    if s.len() > MAX_EXPR_LEN {
        bail!(
            "tag expression is {} bytes, exceeds the {MAX_EXPR_LEN}-byte limit",
            s.len()
        );
    }
    let tokens = tokenize(s)?;
    let mut parser = Parser {
        tokens: &tokens,
        pos: 0,
        depth: 0,
    };
    let expr = parser.parse_or()?;
    if parser.pos != tokens.len() {
        bail!("unexpected trailing input in tag expression: {s:?}");
    }
    Ok(expr)
}

fn tokenize(s: &str) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            c if c.is_whitespace() => {
                chars.next();
            }
            '&' => {
                chars.next();
                tokens.push(Token::And);
            }
            '|' => {
                chars.next();
                tokens.push(Token::Or);
            }
            '!' => {
                chars.next();
                tokens.push(Token::Not);
            }
            '(' => {
                chars.next();
                tokens.push(Token::LParen);
            }
            ')' => {
                chars.next();
                tokens.push(Token::RParen);
            }
            _ => {
                let mut word = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() || "&|!()".contains(c) {
                        break;
                    }
                    word.push(c);
                    chars.next();
                }
                match word.strip_prefix("tag:") {
                    Some(value) if !value.is_empty() => {
                        tokens.push(Token::Tag(value.to_lowercase()));
                    }
                    Some(_) => bail!("empty tag value in tag expression: {s:?}"),
                    None => bail!("expected 'tag:' atom, found {word:?} in tag expression: {s:?}"),
                }
            }
        }
    }
    Ok(tokens)
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    /// Current recursion depth; see `MAX_EXPR_DEPTH`.
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    /// Runs `f` one level deeper, refusing past `MAX_EXPR_DEPTH`.
    fn nested<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        if self.depth >= MAX_EXPR_DEPTH {
            bail!("tag expression nests deeper than {MAX_EXPR_DEPTH} levels");
        }
        self.depth += 1;
        let out = f(self);
        self.depth -= 1;
        out
    }

    fn parse_or(&mut self) -> Result<TagExpr> {
        let mut expr = self.parse_and()?;
        while matches!(self.peek(), Some(Token::Or)) {
            self.pos += 1;
            let rhs = self.parse_and()?;
            expr = TagExpr::Or(Box::new(expr), Box::new(rhs));
        }
        Ok(expr)
    }

    fn parse_and(&mut self) -> Result<TagExpr> {
        let mut expr = self.parse_not()?;
        while matches!(self.peek(), Some(Token::And)) {
            self.pos += 1;
            let rhs = self.parse_not()?;
            expr = TagExpr::And(Box::new(expr), Box::new(rhs));
        }
        Ok(expr)
    }

    fn parse_not(&mut self) -> Result<TagExpr> {
        if matches!(self.peek(), Some(Token::Not)) {
            self.pos += 1;
            let inner = self.nested(|p| p.parse_not())?;
            return Ok(TagExpr::Not(Box::new(inner)));
        }
        self.parse_atom()
    }

    fn parse_atom(&mut self) -> Result<TagExpr> {
        match self.peek() {
            Some(Token::Tag(t)) => {
                let t = t.clone();
                self.pos += 1;
                Ok(TagExpr::Tag(t))
            }
            Some(Token::LParen) => {
                self.pos += 1;
                let expr = self.nested(|p| p.parse_or())?;
                match self.peek() {
                    Some(Token::RParen) => {
                        self.pos += 1;
                        Ok(expr)
                    }
                    _ => bail!("missing closing paren in tag expression"),
                }
            }
            other => bail!("expected tag atom or '(', found {other:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_like_tag_expr_detects_expected_prefixes() {
        assert!(looks_like_tag_expr("tag:project:hivemind"));
        assert!(looks_like_tag_expr("!tag:status:done"));
        assert!(looks_like_tag_expr("(tag:a & tag:b)"));
        assert!(looks_like_tag_expr("  tag:project:hivemind")); // leading whitespace trimmed
        assert!(!looks_like_tag_expr("my exact memory title"));
        assert!(!looks_like_tag_expr("plain fts keywords"));
    }

    #[test]
    fn parses_single_tag() {
        let expr = parse("tag:project:hivemind").unwrap();
        assert_eq!(expr, TagExpr::Tag("project:hivemind".to_string()));
    }

    #[test]
    fn parses_and() {
        let expr = parse("tag:a & tag:b").unwrap();
        assert!(expr.eval(&["a".to_string(), "b".to_string()]));
        assert!(!expr.eval(&["a".to_string()]));
    }

    #[test]
    fn parses_or() {
        let expr = parse("tag:a | tag:b").unwrap();
        assert!(expr.eval(&["a".to_string()]));
        assert!(expr.eval(&["b".to_string()]));
        assert!(!expr.eval(&["c".to_string()]));
    }

    #[test]
    fn parses_not() {
        let expr = parse("!tag:done").unwrap();
        assert!(expr.eval(&["other".to_string()]));
        assert!(!expr.eval(&["done".to_string()]));
    }

    #[test]
    fn and_binds_tighter_than_or() {
        // a & b | c  ==  (a & b) | c
        let expr = parse("tag:a & tag:b | tag:c").unwrap();
        // Only c present: (a&b) is false, so result depends on c being true
        assert!(expr.eval(&["c".to_string()]));
        // Only a present: (a&b) false, c absent -> false
        assert!(!expr.eval(&["a".to_string()]));
        // a and b present, c absent: (a&b) true -> true
        assert!(expr.eval(&["a".to_string(), "b".to_string()]));
    }

    #[test]
    fn parens_override_precedence() {
        // a & (b | c)
        let expr = parse("tag:a & (tag:b | tag:c)").unwrap();
        assert!(expr.eval(&["a".to_string(), "c".to_string()]));
        assert!(!expr.eval(&["c".to_string()])); // a missing
    }

    #[test]
    fn tag_values_are_lowercased_on_parse() {
        let expr = parse("tag:Project:HiveMind").unwrap();
        assert_eq!(expr, TagExpr::Tag("project:hivemind".to_string()));
    }

    #[test]
    fn unbalanced_paren_is_an_error() {
        assert!(parse("(tag:a & tag:b").is_err());
        assert!(parse("tag:a)").is_err());
    }

    #[test]
    fn bare_word_without_tag_prefix_is_an_error() {
        assert!(parse("tag:a & oops").is_err());
    }

    #[test]
    fn empty_tag_value_is_an_error() {
        assert!(parse("tag:").is_err());
    }

    #[test]
    fn deeply_nested_not_is_rejected_instead_of_overflowing_the_stack() {
        // Regression: 60k `!` used to abort the process (stack overflow on a
        // 2 MiB tokio worker stack) from an unauthenticated GET. Run on a
        // 2 MiB thread so the test would also crash if the cap regressed.
        let q = format!("{}tag:a", "!".repeat(MAX_EXPR_LEN - 5));
        let handle = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || parse(&q).is_err())
            .unwrap();
        assert!(handle.join().unwrap(), "over-deep expression must error");
    }

    #[test]
    fn deeply_nested_parens_are_rejected() {
        let q = format!(
            "{}tag:a{}",
            "(".repeat(MAX_EXPR_DEPTH + 1),
            ")".repeat(MAX_EXPR_DEPTH + 1)
        );
        assert!(parse(&q).is_err());
        // Right at the limit is still fine.
        let q = format!(
            "{}tag:a{}",
            "(".repeat(MAX_EXPR_DEPTH),
            ")".repeat(MAX_EXPR_DEPTH)
        );
        assert!(parse(&q).is_ok());
    }

    #[test]
    fn over_long_expression_is_rejected_before_tokenizing() {
        let q = format!("tag:{}", "a".repeat(MAX_EXPR_LEN));
        let err = parse(&q).unwrap_err().to_string();
        assert!(err.contains("exceeds"), "got: {err}");
    }

    #[test]
    fn realistic_nesting_still_parses() {
        // !(a & !(b | !c)): true unless a is present and neither b nor "not c" holds.
        let expr = parse("!(tag:a & !(tag:b | !tag:c))").unwrap();
        assert!(matches!(expr, TagExpr::Not(_)));
        assert!(expr.eval(&["b".to_string()]));
        assert!(!expr.eval(&["a".to_string(), "c".to_string()]));
        assert!(expr.eval(&["a".to_string(), "b".to_string(), "c".to_string()]));
    }

    #[test]
    fn and_all_builds_and_chain() {
        let expr = TagExpr::and_all(&["a".to_string(), "b".to_string(), "c".to_string()]).unwrap();
        assert!(expr.eval(&["a".to_string(), "b".to_string(), "c".to_string()]));
        assert!(!expr.eval(&["a".to_string(), "b".to_string()]));
    }

    #[test]
    fn and_all_empty_returns_none() {
        assert!(TagExpr::and_all(&[]).is_none());
    }
}
