use serde::{Deserialize, Serialize};
use std::collections::{hash_map::DefaultHasher, HashMap, HashSet};
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FindingSeverity {
    Warning,
    Info,
}

impl FindingSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SuspiciousPatternKind {
    StructuralClone,
    IdentifierChurn,
    LiteralChurn,
    WrapperIndirection,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetamorphicFinding {
    pub severity: FindingSeverity,
    pub check_name: String,
    pub kind: SuspiciousPatternKind,
    pub message: String,
    pub file_path: Option<String>,
    pub line: Option<u32>,
    pub symbol: Option<String>,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuralFingerprint {
    pub hash: u64,
    pub normalized_token_count: usize,
    pub identifier_count: usize,
    pub literal_count: usize,
    pub control_flow_count: usize,
    pub call_like_count: usize,
    pub max_nesting_depth: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FingerprintComparison {
    pub structural_similarity: f32,
    pub identifier_churn: f32,
    pub literal_churn: f32,
    pub nesting_delta: isize,
    pub call_like_delta: isize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetamorphicAnalysis {
    pub before: StructuralFingerprint,
    pub after: StructuralFingerprint,
    pub comparison: FingerprintComparison,
    pub findings: Vec<MetamorphicFinding>,
}

#[derive(Debug, Clone)]
pub struct MetamorphicAnalyzer {
    structural_clone_threshold: f32,
    churn_threshold: f32,
}

impl Default for MetamorphicAnalyzer {
    fn default() -> Self {
        Self {
            structural_clone_threshold: 0.92,
            churn_threshold: 0.45,
        }
    }
}

impl MetamorphicAnalyzer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn fingerprint(source: &str) -> StructuralFingerprint {
        fingerprint(tokenize(source))
    }

    pub fn analyze_pair(
        &self,
        before: &str,
        after: &str,
        file_path: Option<impl Into<String>>,
    ) -> MetamorphicAnalysis {
        let before_tokens = tokenize(before);
        let after_tokens = tokenize(after);
        let before_fingerprint = fingerprint(before_tokens.clone());
        let after_fingerprint = fingerprint(after_tokens.clone());
        let comparison = compare_tokens(&before_tokens, &after_tokens);
        let file_path = file_path.map(Into::into);
        let findings = self.classify(&comparison, file_path);

        MetamorphicAnalysis {
            before: before_fingerprint,
            after: after_fingerprint,
            comparison,
            findings,
        }
    }

    fn classify(
        &self,
        comparison: &FingerprintComparison,
        file_path: Option<String>,
    ) -> Vec<MetamorphicFinding> {
        let mut findings = Vec::new();
        let structurally_close =
            comparison.structural_similarity >= self.structural_clone_threshold;

        if structurally_close {
            findings.push(MetamorphicFinding {
                severity: FindingSeverity::Info,
                check_name: "metamorphic.structural_clone".to_string(),
                kind: SuspiciousPatternKind::StructuralClone,
                message: "candidate preserves the source structure closely".to_string(),
                file_path: file_path.clone(),
                line: None,
                symbol: None,
                confidence: comparison.structural_similarity,
            });
        }

        if structurally_close && comparison.identifier_churn >= self.churn_threshold {
            findings.push(MetamorphicFinding {
                severity: FindingSeverity::Warning,
                check_name: "metamorphic.identifier_churn".to_string(),
                kind: SuspiciousPatternKind::IdentifierChurn,
                message: "structure is stable while identifiers changed substantially".to_string(),
                file_path: file_path.clone(),
                line: None,
                symbol: None,
                confidence: comparison.identifier_churn,
            });
        }

        if structurally_close && comparison.literal_churn >= self.churn_threshold {
            findings.push(MetamorphicFinding {
                severity: FindingSeverity::Warning,
                check_name: "metamorphic.literal_churn".to_string(),
                kind: SuspiciousPatternKind::LiteralChurn,
                message: "structure is stable while literal values changed substantially"
                    .to_string(),
                file_path: file_path.clone(),
                line: None,
                symbol: None,
                confidence: comparison.literal_churn,
            });
        }

        if comparison.nesting_delta >= 2 && comparison.call_like_delta >= 1 {
            findings.push(MetamorphicFinding {
                severity: FindingSeverity::Warning,
                check_name: "metamorphic.wrapper_indirection".to_string(),
                kind: SuspiciousPatternKind::WrapperIndirection,
                message: "candidate adds nested wrapper structure and call-like indirection"
                    .to_string(),
                file_path,
                line: None,
                symbol: None,
                confidence: (comparison.nesting_delta as f32 / 4.0).min(1.0),
            });
        }

        findings
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Token {
    Identifier(String),
    Literal(String),
    Keyword(String),
    Symbol(char),
}

impl Token {
    fn normalized(&self) -> String {
        match self {
            Self::Identifier(_) => "$ident".to_string(),
            Self::Literal(_) => "$literal".to_string(),
            Self::Keyword(value) => value.clone(),
            Self::Symbol(value) => value.to_string(),
        }
    }
}

fn fingerprint(tokens: Vec<Token>) -> StructuralFingerprint {
    let normalized: Vec<String> = tokens.iter().map(Token::normalized).collect();
    let mut hasher = DefaultHasher::new();
    normalized.hash(&mut hasher);

    StructuralFingerprint {
        hash: hasher.finish(),
        normalized_token_count: normalized.len(),
        identifier_count: tokens
            .iter()
            .filter(|token| matches!(token, Token::Identifier(_)))
            .count(),
        literal_count: tokens
            .iter()
            .filter(|token| matches!(token, Token::Literal(_)))
            .count(),
        control_flow_count: tokens
            .iter()
            .filter(|token| matches!(token, Token::Keyword(value) if is_control_flow(value)))
            .count(),
        call_like_count: call_like_count(&tokens),
        max_nesting_depth: max_nesting_depth(&tokens),
    }
}

fn compare_tokens(before: &[Token], after: &[Token]) -> FingerprintComparison {
    FingerprintComparison {
        structural_similarity: multiset_similarity(
            before.iter().map(Token::normalized),
            after.iter().map(Token::normalized),
        ),
        identifier_churn: value_churn(
            before.iter().filter_map(identifier_value),
            after.iter().filter_map(identifier_value),
        ),
        literal_churn: value_churn(
            before.iter().filter_map(literal_value),
            after.iter().filter_map(literal_value),
        ),
        nesting_delta: max_nesting_depth(after) as isize - max_nesting_depth(before) as isize,
        call_like_delta: call_like_count(after) as isize - call_like_count(before) as isize,
    }
}

fn tokenize(source: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = source.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch.is_whitespace() {
            continue;
        }

        if ch == '/' && chars.peek() == Some(&'/') {
            for next in chars.by_ref() {
                if next == '\n' {
                    break;
                }
            }
            continue;
        }

        if ch == '#' {
            for next in chars.by_ref() {
                if next == '\n' {
                    break;
                }
            }
            continue;
        }

        if ch == '"' || ch == '\'' || ch == '`' {
            let quote = ch;
            let mut literal = String::new();
            while let Some(next) = chars.next() {
                if next == '\\' {
                    if let Some(escaped) = chars.next() {
                        literal.push(escaped);
                    }
                    continue;
                }
                if next == quote {
                    break;
                }
                literal.push(next);
            }
            tokens.push(Token::Literal(literal));
            continue;
        }

        if ch.is_ascii_digit() {
            let mut literal = ch.to_string();
            while let Some(next) = chars.peek() {
                if next.is_ascii_alphanumeric() || *next == '.' || *next == '_' {
                    literal.push(*next);
                    chars.next();
                } else {
                    break;
                }
            }
            tokens.push(Token::Literal(literal));
            continue;
        }

        if is_identifier_start(ch) {
            let mut value = ch.to_string();
            while let Some(next) = chars.peek() {
                if is_identifier_continue(*next) {
                    value.push(*next);
                    chars.next();
                } else {
                    break;
                }
            }

            if is_keyword(&value) {
                tokens.push(Token::Keyword(value));
            } else {
                tokens.push(Token::Identifier(value));
            }
            continue;
        }

        if "{}[]().,:;+-*/%=!<>|&?".contains(ch) {
            tokens.push(Token::Symbol(ch));
        }
    }

    tokens
}

fn is_identifier_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_identifier_continue(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn is_keyword(value: &str) -> bool {
    matches!(
        value,
        "as" | "async"
            | "await"
            | "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "def"
            | "else"
            | "enum"
            | "false"
            | "fn"
            | "for"
            | "function"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "match"
            | "module"
            | "mut"
            | "return"
            | "static"
            | "struct"
            | "switch"
            | "true"
            | "try"
            | "type"
            | "var"
            | "while"
    )
}

fn is_control_flow(value: &str) -> bool {
    matches!(
        value,
        "break"
            | "case"
            | "catch"
            | "continue"
            | "else"
            | "for"
            | "if"
            | "match"
            | "return"
            | "switch"
            | "try"
            | "while"
    )
}

fn call_like_count(tokens: &[Token]) -> usize {
    tokens
        .windows(2)
        .filter(|window| {
            matches!(window[0], Token::Identifier(_)) && matches!(window[1], Token::Symbol('('))
        })
        .count()
}

fn max_nesting_depth(tokens: &[Token]) -> usize {
    let mut current = 0usize;
    let mut max = 0usize;

    for token in tokens {
        match token {
            Token::Symbol('{') | Token::Symbol('[') | Token::Symbol('(') => {
                current += 1;
                max = max.max(current);
            }
            Token::Symbol('}') | Token::Symbol(']') | Token::Symbol(')') => {
                current = current.saturating_sub(1);
            }
            _ => {}
        }
    }

    max
}

fn identifier_value(token: &Token) -> Option<String> {
    match token {
        Token::Identifier(value) => Some(value.clone()),
        _ => None,
    }
}

fn literal_value(token: &Token) -> Option<String> {
    match token {
        Token::Literal(value) => Some(value.clone()),
        _ => None,
    }
}

fn multiset_similarity(
    before: impl Iterator<Item = String>,
    after: impl Iterator<Item = String>,
) -> f32 {
    let before_counts = counts(before);
    let after_counts = counts(after);
    let keys: HashSet<&String> = before_counts.keys().chain(after_counts.keys()).collect();
    let mut intersection = 0usize;
    let mut union = 0usize;

    for key in keys {
        let before_count = before_counts.get(key).copied().unwrap_or(0);
        let after_count = after_counts.get(key).copied().unwrap_or(0);
        intersection += before_count.min(after_count);
        union += before_count.max(after_count);
    }

    if union == 0 {
        1.0
    } else {
        intersection as f32 / union as f32
    }
}

fn value_churn(before: impl Iterator<Item = String>, after: impl Iterator<Item = String>) -> f32 {
    1.0 - multiset_similarity(before, after)
}

fn counts(values: impl Iterator<Item = String>) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for value in values {
        *counts.entry(value).or_insert(0) += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_tracks_structural_metrics() {
        let source = r#"
            fn score(value: i32) -> i32 {
                if value > 10 {
                    return normalize(value);
                }
                value
            }
        "#;

        let fingerprint = MetamorphicAnalyzer::fingerprint(source);

        assert!(fingerprint.normalized_token_count > 10);
        assert_eq!(fingerprint.control_flow_count, 2);
        assert_eq!(fingerprint.call_like_count, 2);
        assert!(fingerprint.max_nesting_depth >= 2);
    }

    #[test]
    fn detects_identifier_churn_on_preserved_structure() {
        let analyzer = MetamorphicAnalyzer::new();
        let before = "fn add(left: i32, right: i32) -> i32 { left + right }";
        let after = "fn sum(alpha: i32, beta: i32) -> i32 { alpha + beta }";

        let result = analyzer.analyze_pair(before, after, Some("src/math.rs"));

        assert!(result
            .findings
            .iter()
            .any(|finding| finding.kind == SuspiciousPatternKind::IdentifierChurn));
        assert_eq!(result.findings[0].file_path.as_deref(), Some("src/math.rs"));
    }

    #[test]
    fn detects_literal_churn_on_preserved_structure() {
        let analyzer = MetamorphicAnalyzer::new();
        let before = r#"const LIMIT: usize = 10; const MODE: &str = "safe";"#;
        let after = r#"const LIMIT: usize = 99; const MODE: &str = "shadow";"#;

        let result = analyzer.analyze_pair(before, after, None::<String>);

        assert!(result
            .findings
            .iter()
            .any(|finding| finding.kind == SuspiciousPatternKind::LiteralChurn));
    }

    #[test]
    fn detects_wrapper_indirection_growth() {
        let analyzer = MetamorphicAnalyzer::new();
        let before = "fn run() { execute(); }";
        let after = "fn run() { guard(|| { audit(|| { execute(); }); }); }";

        let result = analyzer.analyze_pair(before, after, None::<String>);

        assert!(result
            .findings
            .iter()
            .any(|finding| finding.kind == SuspiciousPatternKind::WrapperIndirection));
    }
}
