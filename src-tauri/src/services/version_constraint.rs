//! Version constraints as projects write them — Composer's (`require.php`)
//! and npm's (`engines.node`) — reduced to "does this version fit?".
//!
//! Parsed here rather than pulled in as crates: only runtime requirements
//! are read, so the subset that matters is small — `^`, `~`, wildcards,
//! comparisons, hyphen ranges, `,`/space for "and", `|`/`||` for "or". The
//! two dialects differ in two places, both handled by [`Dialect`]: what `~`
//! may move, and whether a bare partial version (`20`) is a range or exact.
//! Anything that doesn't parse is `None` — callers report it as unreadable,
//! never as a mismatch: a wrong warning is worse than none.

/// A version as three numbers. PHP's and Node's versions never need more.
pub type Version = (u64, u64, u64);

/// Whose rules a constraint follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// `~8.1` is `>=8.1 <9`; a bare `8.2` is exactly 8.2.0.
    Composer,
    /// `~8.1` is `>=8.1 <8.2`; a bare `20` is any 20.x.
    Npm,
}

/// A parsed constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Constraint {
    /// Any one group may match; within a group, every term must.
    any_of: Vec<Vec<Term>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Term {
    Any,
    Ge(Version),
    Gt(Version),
    Le(Version),
    Lt(Version),
    Eq(Version),
    Ne(Version),
}

impl Term {
    fn allows(&self, v: Version) -> bool {
        match *self {
            Term::Any => true,
            Term::Ge(b) => v >= b,
            Term::Gt(b) => v > b,
            Term::Le(b) => v <= b,
            Term::Lt(b) => v < b,
            Term::Eq(b) => v == b,
            Term::Ne(b) => v != b,
        }
    }
}

impl Constraint {
    /// `None` for anything outside the supported syntax.
    pub fn parse(text: &str, dialect: Dialect) -> Option<Constraint> {
        let mut any_of = Vec::new();
        for group in text.split('|').filter(|g| !g.trim().is_empty()) {
            let group = group.trim();
            // `8.1 - 8.3`: a hyphen range, inclusive of a fully given upper end.
            if let Some((low, high)) = group.split_once(" - ") {
                let low = parse_parts(low)?;
                let high = parse_parts(high)?;
                let upper = if high.len() >= 3 {
                    Term::Le(version_of(&high))
                } else {
                    Term::Lt(bump_last(&high))
                };
                any_of.push(vec![Term::Ge(version_of(&low)), upper]);
                continue;
            }
            let mut terms = Vec::new();
            for token in and_tokens(group) {
                terms.extend(parse_term(&token, dialect)?);
            }
            if terms.is_empty() {
                return None;
            }
            any_of.push(terms);
        }
        (!any_of.is_empty()).then_some(Constraint { any_of })
    }

    pub fn allows(&self, v: Version) -> bool {
        self.any_of
            .iter()
            .any(|group| group.iter().all(|term| term.allows(v)))
    }
}

/// `8.2.29` → `[8, 2, 29]`. A `v` prefix and any suffix (`-dev`, `RC1`,
/// `+build`) are dropped; anything else non-numeric fails.
pub fn parse_parts(text: &str) -> Option<Vec<u64>> {
    let text = text.trim().trim_start_matches(['v', 'V']);
    let text = text.split(['-', '+', '@']).next()?;
    let parts: Vec<u64> = text
        .split('.')
        .map(|part| {
            let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        })
        .collect::<Option<_>>()?;
    (!parts.is_empty() && parts.len() <= 4).then_some(parts)
}

/// The parts as a [`Version`], missing ones as zero.
pub fn version_of(parts: &[u64]) -> Version {
    (
        parts.first().copied().unwrap_or(0),
        parts.get(1).copied().unwrap_or(0),
        parts.get(2).copied().unwrap_or(0),
    )
}

/// `"8.2.29"` → `(8, 2, 29)`.
pub fn parse_version(text: &str) -> Option<Version> {
    parse_parts(text).map(|parts| version_of(&parts))
}

/// The first version past the last part given: `8.2` → `8.3.0`, `8` → `9.0.0`.
fn bump_last(parts: &[u64]) -> Version {
    match parts.len() {
        1 => (parts[0] + 1, 0, 0),
        2 => (parts[0], parts[1] + 1, 0),
        _ => (parts[0], parts[1], parts[2] + 1),
    }
}

/// One term of an "and" group, possibly expanding to two (`^8.2` is
/// `>= 8.2.0, < 9.0.0`).
fn parse_term(token: &str, dialect: Dialect) -> Option<Vec<Term>> {
    let token = token.trim();
    if token == "*" || token.eq_ignore_ascii_case("x") {
        return Some(vec![Term::Any]);
    }
    if let Some(rest) = token.strip_prefix('^') {
        let parts = parse_parts(rest)?;
        let low = version_of(&parts);
        // The first non-zero part is the one that may not change.
        let high = match (low, parts.len()) {
            ((0, 0, p), n) if n >= 3 => (0, 0, p + 1),
            ((0, 0, _), _) => (0, 1, 0),
            ((0, m, _), _) => (0, m + 1, 0),
            ((major, _, _), _) => (major + 1, 0, 0),
        };
        return Some(vec![Term::Ge(low), Term::Lt(high)]);
    }
    if let Some(rest) = token.strip_prefix('~') {
        let rest = rest.trim_start_matches('>'); // npm also accepts `~>`
        let parts = parse_parts(rest)?;
        let low = version_of(&parts);
        let high = match (dialect, parts.len()) {
            (_, 1) => (parts[0] + 1, 0, 0),
            // Composer: `~8.2` may move the minor. npm: only the patch.
            (Dialect::Composer, 2) => (parts[0] + 1, 0, 0),
            _ => (parts[0], parts[1] + 1, 0),
        };
        return Some(vec![Term::Ge(low), Term::Lt(high)]);
    }
    for (op, make) in [
        (">=", Term::Ge as fn(Version) -> Term),
        ("<=", Term::Le),
        ("!=", Term::Ne),
        ("<>", Term::Ne),
        ("==", Term::Eq),
        (">", Term::Gt),
        ("<", Term::Lt),
        ("=", Term::Eq),
    ] {
        if let Some(rest) = token.strip_prefix(op) {
            return Some(vec![make(version_of(&parse_parts(rest)?))]);
        }
    }
    // `8.2.*`, `8.*`, `20.x`
    let wildcard = ["*", "x", "X"].iter().find_map(|w| {
        token
            .strip_suffix(&format!(".{w}.{w}"))
            .or_else(|| token.strip_suffix(&format!(".{w}")))
    });
    if let Some(rest) = wildcard {
        let parts = parse_parts(rest)?;
        return Some(vec![
            Term::Ge(version_of(&parts)),
            Term::Lt(bump_last(&parts)),
        ]);
    }
    let parts = parse_parts(token)?;
    // npm reads a partial version as a range (`20` is 20.x); Composer as
    // exact (`8.2` is 8.2.0 and nothing else).
    if dialect == Dialect::Npm && parts.len() < 3 {
        return Some(vec![
            Term::Ge(version_of(&parts)),
            Term::Lt(bump_last(&parts)),
        ]);
    }
    Some(vec![Term::Eq(version_of(&parts))])
}

/// `>= 8.2 <9` → `[">=8.2", "<9"]`: an operator standing alone belongs to
/// the version after it.
fn and_tokens(group: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut pending = String::new();
    for raw in group.split([',', ' ']).filter(|t| !t.trim().is_empty()) {
        let raw = raw.trim();
        if raw.chars().all(|c| "<>=!^~".contains(c)) {
            pending.push_str(raw);
            continue;
        }
        tokens.push(format!("{pending}{raw}"));
        pending.clear();
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allows(dialect: Dialect, constraint: &str, version: &str) -> bool {
        Constraint::parse(constraint, dialect)
            .unwrap_or_else(|| panic!("{constraint}"))
            .allows(parse_version(version).unwrap())
    }

    fn composer(constraint: &str, version: &str) -> bool {
        allows(Dialect::Composer, constraint, version)
    }

    fn npm(constraint: &str, version: &str) -> bool {
        allows(Dialect::Npm, constraint, version)
    }

    #[test]
    fn caret_and_tilde_follow_composer() {
        assert!(composer("^8.2", "8.2.0"));
        assert!(composer("^8.2", "8.4.5"));
        assert!(!composer("^8.2", "8.1.30"));
        assert!(!composer("^8.2", "9.0.0"));
        assert!(composer("~8.1", "8.3.1"));
        assert!(!composer("~8.1", "9.0.0"));
        assert!(composer("~8.1.2", "8.1.9"));
        assert!(!composer("~8.1.2", "8.2.0"));
    }

    #[test]
    fn or_and_and_combine() {
        assert!(composer("^7.3|^8.0", "7.4.33"));
        assert!(composer("^7.3 || ^8.0", "8.3.1"));
        assert!(!composer("^7.3|^8.0", "7.2.34"));
        assert!(composer(">=7.4 <8.3", "8.2.29"));
        assert!(!composer(">= 7.4, < 8.3", "8.3.0"));
        assert!(composer(">= 8.1", "8.4.0"));
    }

    #[test]
    fn wildcards_ranges_and_exact_versions() {
        assert!(composer("8.2.*", "8.2.29"));
        assert!(!composer("8.2.*", "8.3.0"));
        assert!(composer("8.*", "8.4.0"));
        assert!(composer("*", "5.6.40"));
        assert!(composer("8.1 - 8.3", "8.3.9"));
        assert!(!composer("8.1 - 8.3", "8.4.0"));
        assert!(composer("8.1.0 - 8.3.0", "8.3.0"));
        assert!(!composer("8.1.0 - 8.3.0", "8.3.1"));
        assert!(composer("8.2.0", "8.2.0"));
        assert!(!composer("8.2", "8.2.29"));
        assert!(!composer("!=8.2.0", "8.2.0"));
    }

    /// The `engines.node` lines real packages ship.
    #[test]
    fn npm_engines_as_published() {
        // Next.js 15
        assert!(npm("^18.18.0 || ^19.8.0 || >= 20.0.0", "22.11.0"));
        assert!(!npm("^18.18.0 || ^19.8.0 || >= 20.0.0", "18.17.1"));
        // Vite 7
        assert!(npm("^20.19.0 || >=22.12.0", "20.19.2"));
        assert!(!npm("^20.19.0 || >=22.12.0", "22.11.0"));
        // Angular 18
        assert!(npm("^18.19.1 || ^20.11.1 || >=22.0.0", "20.12.0"));
        assert!(npm(">=18.17", "18.17.0"));
        assert!(npm("20.x", "20.11.1"));
        assert!(!npm("20.x", "22.0.0"));
        assert!(npm(">=20.0.0-0", "20.0.0"));
    }

    #[test]
    fn the_dialects_differ_where_they_should() {
        assert!(composer("~8.1", "8.3.0"));
        assert!(!npm("~8.1", "8.3.0"));
        assert!(npm("~8.1", "8.1.9"));
        assert!(npm("20", "20.11.1"));
        assert!(!composer("20", "20.11.1"));
    }

    #[test]
    fn nonsense_is_none() {
        assert_eq!(Constraint::parse("latest", Dialect::Npm), None);
        assert_eq!(Constraint::parse("lts/*", Dialect::Npm), None);
        assert_eq!(Constraint::parse("", Dialect::Composer), None);
    }
}
