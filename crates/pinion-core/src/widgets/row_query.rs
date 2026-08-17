//! R1707 §5.40 — **a filter a person can write, an agent can read back, and
//! that says why it dropped a row.**
//!
//! [`GridFilter`] is the predicate: a conjunction of facets, each naming a
//! column by ORDINAL and an operand by string. That is the right shape to
//! evaluate and the wrong shape to *type*, to *show* or to *save* — a reader
//! writes `type in (Data, Declare)`, not `2:Data,Declare`, and a filter stored
//! against ordinals silently changes meaning the day a column moves.
//!
//! This module is the layer between: the query as a person writes it, parsed
//! against a roster of column NAMES, keeping each clause's own source text, and
//! compiling to the [`GridFilter`] that does the work.
//!
//! # Why the person's own words are kept
//!
//! Measured on the reference toolkit at 6.11.1, built as a probe and run rather
//! than read: its row-filtering proxy accepts a wildcard, and asking it back
//! answers the compiled regular expression — `sensors/unit/*` returns
//! `(?s:sensors/unit/[^/]*)`. The pattern a person typed is not recoverable
//! from the object that holds it, so a UI wanting to redisplay the query has to
//! keep a second copy beside the model and hope the two stay equal. Here the
//! clause carries [`Clause::text`], so what is shown and what filters are one
//! fact.
//!
//! The same probe measured the rest of the floor: one filter slot for the whole
//! model ([`FilterOp`] facets are a conjunction), the column addressed by
//! ordinal (`filterKeyColumn` is an `int`, and there is no name-taking peer),
//! three operators with setters and no set membership or inequality among them,
//! and — across 12 properties and 101 methods — **not one member naming a
//! reason** a row was dropped.

use crate::widgets::grid_sort::{Admission, ColumnFacet, FilterOp, GridFilter, members_of};

/// The operators a written query may use, longest token first so `>=` never
/// decodes as `>` and `~=` never as `~`.
///
/// Derived from [`FilterOp::wire_token`] where the tokens coincide, and
/// spelled out here where the readable form differs: set membership is written
/// `in (a, b)` rather than `:a,b`. A test asserts every [`FilterOp`] appears,
/// so an operator added to the framework and forgotten here fails rather than
/// becoming quietly unwritable.
const WRITTEN_OPS: &[(&str, FilterOp)] = &[
    ("<=", FilterOp::Le),
    (">=", FilterOp::Ge),
    ("!=", FilterOp::Ne),
    ("~=", FilterOp::Glob),
    ("in", FilterOp::In),
    ("~", FilterOp::Contains),
    ("=", FilterOp::Eq),
    ("<", FilterOp::Lt),
    (">", FilterOp::Gt),
];

/// One written clause: a named column, an operator, an operand — and the source
/// text it was written as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clause {
    /// The column name as written, resolved against the roster.
    pub column: String,
    /// The column's index in the roster.
    pub col: usize,
    /// The operator.
    pub op: FilterOp,
    /// The operand with its quoting and, for [`FilterOp::In`], its parentheses
    /// removed: `"sensors/**"` becomes `sensors/**`, `(Data, Declare)` becomes
    /// `Data,Declare`.
    pub operand: String,
    /// The clause exactly as the person wrote it, whitespace-trimmed.
    ///
    /// Kept so that redisplaying a query cannot drift from the query that runs.
    pub text: String,
}

impl Clause {
    /// The facet this clause compiles to.
    #[must_use]
    pub fn facet(&self) -> ColumnFacet {
        ColumnFacet::new(self.col, self.op, self.operand.clone())
    }

    /// The clause rendered in normal form, which is what
    /// [`RowQuery::to_text`] joins.
    ///
    /// Not the same string as [`text`](Self::text) in general: this is what the
    /// query looks like written canonically, and `text` is what was typed. A
    /// consumer showing the person their own query wants `text`; one saving a
    /// query for later comparison wants this.
    #[must_use]
    pub fn normal_form(&self) -> String {
        match self.op {
            FilterOp::In => format!(
                "{} in ({})",
                self.column,
                members_of(&self.operand).collect::<Vec<_>>().join(", ")
            ),
            op => {
                let operand = if self.operand.contains(' ') || self.operand.is_empty() {
                    format!("\"{}\"", self.operand)
                } else {
                    self.operand.clone()
                };
                format!("{} {} {operand}", self.column, op.wire_token())
            }
        }
    }
}

/// Why a written query could not be understood, naming the offending text.
///
/// A refusal that only said "invalid" would put the reader in the position the
/// reference floor's silent-empty-result puts them in: a screen showing nothing
/// with no way to tell a wrong column name from a genuinely empty match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryError {
    /// A clause named a column the roster does not have.
    UnknownColumn {
        /// The name as written.
        name: String,
        /// The names that would have worked, in roster order.
        known: Vec<String>,
    },
    /// A clause had a column and no recognisable operator after it.
    NoOperator {
        /// The clause as written.
        clause: String,
    },
    /// A clause had an operator and nothing after it.
    NoOperand {
        /// The clause as written.
        clause: String,
    },
    /// A clause was empty — two `and`s in a row, or a trailing one.
    EmptyClause,
    /// A double-quoted operand was never closed.
    UnclosedQuote {
        /// The clause as written.
        clause: String,
    },
    /// A set operand was never closed.
    UnclosedGroup {
        /// The clause as written.
        clause: String,
    },
}

impl core::fmt::Display for QueryError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            QueryError::UnknownColumn { name, known } => write!(
                f,
                "no column is called {name:?} — this list has {}",
                known.join(", ")
            ),
            QueryError::NoOperator { clause } => {
                write!(f, "{clause:?} names a column and then no comparison")
            }
            QueryError::NoOperand { clause } => {
                write!(f, "{clause:?} compares against nothing")
            }
            QueryError::EmptyClause => write!(f, "a clause is empty"),
            QueryError::UnclosedQuote { clause } => {
                write!(f, "{clause:?} opens a quote it never closes")
            }
            QueryError::UnclosedGroup { clause } => {
                write!(f, "{clause:?} opens a set it never closes")
            }
        }
    }
}

/// A written row filter: the clauses as typed, and the predicate they compile
/// to.
///
/// Empty is a legitimate value and means *keep everything* — the state a
/// cleared query bar is in — so a consumer never has to model "no query" and
/// "a query matching all" as two things.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RowQuery {
    clauses: Vec<Clause>,
    source: String,
}

impl RowQuery {
    /// The query that keeps every row.
    #[must_use]
    pub fn everything() -> Self {
        Self::default()
    }

    /// Parse `text` against a roster of column names.
    ///
    /// Clauses are separated by the word `and` at the top level — inside quotes
    /// and inside a set's parentheses the word is ordinary text. Column names
    /// are matched case-insensitively against `columns`, because a person types
    /// what the header shows and a header is title-cased as often as not.
    ///
    /// # Errors
    ///
    /// [`QueryError`], naming the clause at fault.
    pub fn parse(text: &str, columns: &[&str]) -> Result<Self, QueryError> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Ok(Self {
                clauses: Vec::new(),
                source: String::new(),
            });
        }
        let mut clauses = Vec::new();
        for raw in split_clauses(trimmed)? {
            clauses.push(parse_clause(raw, columns)?);
        }
        Ok(Self {
            clauses,
            source: trimmed.to_owned(),
        })
    }

    /// The clauses, in the order written.
    #[must_use]
    pub fn clauses(&self) -> &[Clause] {
        &self.clauses
    }

    /// Whether this query keeps every row.
    #[must_use]
    pub fn is_everything(&self) -> bool {
        self.clauses.is_empty()
    }

    /// The text this query was parsed from, verbatim.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The predicate, or `None` when the query keeps everything.
    ///
    /// Facet `i` is clause `i` — the correspondence [`rejecting_clause`] relies
    /// on, and which a test asserts rather than assumes.
    ///
    /// [`rejecting_clause`]: Self::rejecting_clause
    #[must_use]
    pub fn filter(&self) -> Option<GridFilter> {
        (!self.clauses.is_empty())
            .then(|| GridFilter::all(self.clauses.iter().map(Clause::facet).collect()))
    }

    /// Whether a row is kept, and when it is not, which clause dropped it.
    #[must_use]
    pub fn admit<'a, F: Fn(usize) -> &'a str>(&self, cell: F) -> Admission {
        match self.filter() {
            Some(filter) => filter.admit(cell),
            None => Admission::Admitted,
        }
    }

    /// The clause that dropped this row, or `None` when it was kept.
    ///
    /// The question the reference floor cannot answer at all: there a dropped
    /// row maps to an invalid index, which is the same answer for every reason
    /// a row can be absent.
    #[must_use]
    pub fn rejecting_clause<'a, F: Fn(usize) -> &'a str>(&self, cell: F) -> Option<&Clause> {
        self.admit(cell)
            .rejected_by()
            .and_then(|i| self.clauses.get(i))
    }

    /// The query in normal form — every clause canonically written, joined by
    /// `and`.
    #[must_use]
    pub fn to_text(&self) -> String {
        self.clauses
            .iter()
            .map(Clause::normal_form)
            .collect::<Vec<_>>()
            .join(" and ")
    }
}

/// Split a query into clause texts on the top-level word `and`.
///
/// Top level means: not inside a double-quoted operand and not inside a set's
/// parentheses. The separator has to be the whole word, so a column called
/// `band` or an operand containing `and` is not a split point.
fn split_clauses(text: &str) -> Result<Vec<&str>, QueryError> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let (mut start, mut i) = (0usize, 0usize);
    let (mut quoted, mut depth) = (false, 0i32);
    while i < bytes.len() {
        match bytes[i] {
            b'"' => quoted = !quoted,
            b'(' if !quoted => depth += 1,
            b')' if !quoted => depth -= 1,
            b'a' if !quoted && depth == 0 && is_word_at(bytes, i, b"and") => {
                out.push(text[start..i].trim());
                start = i + 3;
                i += 3;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(text[start..].trim());
    if out.iter().any(|c| c.is_empty()) {
        return Err(QueryError::EmptyClause);
    }
    if quoted {
        return Err(QueryError::UnclosedQuote {
            clause: text.to_owned(),
        });
    }
    if depth != 0 {
        return Err(QueryError::UnclosedGroup {
            clause: text.to_owned(),
        });
    }
    Ok(out)
}

/// Whether `word` sits at `i` with a non-word byte on each side.
fn is_word_at(bytes: &[u8], i: usize, word: &[u8]) -> bool {
    if bytes.len() < i + word.len() || &bytes[i..i + word.len()] != word {
        return false;
    }
    let before = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
    let after = bytes
        .get(i + word.len())
        .is_none_or(|b| !b.is_ascii_alphanumeric());
    before && after
}

/// Parse one clause: a column name, an operator, an operand.
fn parse_clause(raw: &str, columns: &[&str]) -> Result<Clause, QueryError> {
    let (name, op, rest) = split_operator(raw)?;
    let col = columns
        .iter()
        .position(|c| c.eq_ignore_ascii_case(name))
        .ok_or_else(|| QueryError::UnknownColumn {
            name: name.to_owned(),
            known: columns.iter().map(|c| (*c).to_string()).collect(),
        })?;
    let operand = parse_operand(rest, op, raw)?;
    Ok(Clause {
        column: columns[col].to_owned(),
        col,
        op,
        operand,
        text: raw.to_owned(),
    })
}

/// Find the operator token in a clause, returning what is on each side of it.
///
/// Searched left to right so the FIRST operator wins, and the alternatives are
/// tried longest-token-first at each position so `>=` is never read as `>`.
/// The word `in` only counts as an operator when it stands as a whole word,
/// which is what keeps a column named `interface` from being read as `i` + `n`.
fn split_operator(raw: &str) -> Result<(&str, FilterOp, &str), QueryError> {
    let bytes = raw.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] == b'"' {
            break;
        }
        for &(token, op) in WRITTEN_OPS {
            let t = token.as_bytes();
            if op == FilterOp::In {
                if !is_word_at(bytes, i, t) {
                    continue;
                }
            } else if bytes.len() < i + t.len() || &bytes[i..i + t.len()] != t {
                continue;
            }
            let name = raw[..i].trim();
            if name.is_empty() {
                continue;
            }
            return Ok((name, op, raw[i + t.len()..].trim()));
        }
    }
    Err(QueryError::NoOperator {
        clause: raw.to_owned(),
    })
}

/// Strip an operand's quoting or parentheses.
fn parse_operand(rest: &str, op: FilterOp, raw: &str) -> Result<String, QueryError> {
    if rest.is_empty() {
        return Err(QueryError::NoOperand {
            clause: raw.to_owned(),
        });
    }
    if op == FilterOp::In {
        let inner = rest
            .strip_prefix('(')
            .and_then(|r| r.strip_suffix(')'))
            .ok_or_else(|| QueryError::UnclosedGroup {
                clause: raw.to_owned(),
            })?;
        let joined = members_of(inner).collect::<Vec<_>>().join(",");
        if joined.is_empty() {
            return Err(QueryError::NoOperand {
                clause: raw.to_owned(),
            });
        }
        return Ok(joined);
    }
    if let Some(inner) = rest.strip_prefix('"') {
        return inner
            .strip_suffix('"')
            .map(ToOwned::to_owned)
            .ok_or_else(|| QueryError::UnclosedQuote {
                clause: raw.to_owned(),
            });
    }
    Ok(rest.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::grid_sort::glob_matches;

    const COLUMNS: &[&str] = &["time", "name", "type", "node", "sn"];

    fn row<'a>(cells: &'a [&'a str]) -> impl Fn(usize) -> &'a str {
        move |i| cells.get(i).copied().unwrap_or("")
    }

    #[test]
    fn every_framework_operator_is_writable() {
        for op in FilterOp::ALL {
            assert!(
                WRITTEN_OPS.iter().any(|&(_, o)| o == op),
                "★ {op:?} exists in the framework and no written query can \
                 express it — an operator nobody can type is one the screen \
                 offers and the person cannot reach"
            );
        }
    }

    #[test]
    fn the_canon_query_parses_into_its_three_clauses() {
        let q = RowQuery::parse(
            "name ~= \"sensors/unit/**\" and type in (Data, Declare) and node != n3",
            COLUMNS,
        )
        .expect("the reference's own opening query is well formed");
        assert_eq!(q.clauses().len(), 3);
        assert_eq!(q.clauses()[0].op, FilterOp::Glob);
        assert_eq!(q.clauses()[0].operand, "sensors/unit/**");
        assert_eq!(q.clauses()[1].op, FilterOp::In);
        assert_eq!(q.clauses()[1].operand, "Data,Declare");
        assert_eq!(q.clauses()[2].op, FilterOp::Ne);
        assert_eq!(q.clauses()[2].operand, "n3");
    }

    #[test]
    fn a_clause_keeps_the_text_it_was_written_as() {
        let q = RowQuery::parse("name ~=   \"a/**\"", COLUMNS).expect("well formed");
        assert_eq!(
            q.clauses()[0].text,
            "name ~=   \"a/**\"",
            "★ the words the person typed survive — the fact the reference \
             floor loses when it compiles a wildcard to a regexp"
        );
        assert_eq!(q.clauses()[0].normal_form(), "name ~= a/**");
    }

    #[test]
    fn normal_form_round_trips_through_the_parser() {
        let source = "name ~= \"sensors/unit/**\" and type in (Data, Declare) and node != n3";
        let once = RowQuery::parse(source, COLUMNS).expect("well formed");
        let twice = RowQuery::parse(&once.to_text(), COLUMNS).expect("normal form re-parses");
        assert_eq!(once.to_text(), twice.to_text());
        assert_eq!(
            once.filter(),
            twice.filter(),
            "the predicate survives a round trip through the readable form"
        );
    }

    #[test]
    fn facet_index_is_clause_index() {
        let q = RowQuery::parse("type = Data and node = n1", COLUMNS).expect("well formed");
        let filter = q.filter().expect("two clauses");
        for (i, clause) in q.clauses().iter().enumerate() {
            assert_eq!(
                filter.facets[i],
                clause.facet(),
                "★ attribution reads the clause at the facet's index, so the \
                 two orders have to be one order"
            );
        }
    }

    #[test]
    fn a_dropped_row_names_the_clause_that_dropped_it() {
        let q = RowQuery::parse(
            "name ~= \"sensors/**\" and type in (Data, Declare) and node != n3",
            COLUMNS,
        )
        .expect("well formed");
        let kept = row(&["00.1", "sensors/unit/3", "Data", "n1"]);
        assert_eq!(q.admit(&kept), Admission::Admitted);
        assert!(q.rejecting_clause(&kept).is_none());

        let wrong_type = row(&["00.2", "sensors/unit/3", "Response", "n1"]);
        assert_eq!(
            q.rejecting_clause(&wrong_type).map(|c| c.column.as_str()),
            Some("type"),
            "★ the reason, not merely the absence"
        );

        let wrong_node = row(&["00.3", "sensors/unit/3", "Data", "n3"]);
        assert_eq!(
            q.rejecting_clause(&wrong_node).map(|c| c.column.as_str()),
            Some("node")
        );

        let wrong_name = row(&["00.4", "admin/health", "Data", "n1"]);
        assert_eq!(
            q.rejecting_clause(&wrong_name).map(|c| c.column.as_str()),
            Some("name"),
            "and the FIRST clause that refused, so the answer is stable"
        );
    }

    /// ★★★★★ R1707 — **a counterfactual found this missing**, which is what
    /// counterfactuals are for.
    ///
    /// Making `*` cross `/` left every test in the tree green: the queries
    /// exercised above happen not to discriminate (`sensors/**` is deep by
    /// design, and `sensors/unit-*/**` is saved by the literal `-` rather than
    /// by the star's depth). So the ONE property that distinguishes this
    /// operator from a substring test had nothing asserting it — and a `Glob`
    /// that crossed separators would be `Contains` with extra steps.
    #[test]
    fn a_shallow_star_does_not_cross_a_separator_and_a_deep_one_does() {
        assert!(glob_matches("sensors/*/temp", "sensors/a/temp"));
        assert!(
            !glob_matches("sensors/*/temp", "sensors/a/b/temp"),
            "★ one `*` may not span a `/` — the whole difference from a \
             substring match"
        );
        assert!(glob_matches("sensors/**/temp", "sensors/a/b/temp"));
        assert!(glob_matches("sensors/**", "sensors/a/b/c"));
        assert!(!glob_matches("sensors/*", "sensors/a/b"));
        // `?` is one character and is not the separator either.
        assert!(glob_matches("a?c", "abc"));
        assert!(!glob_matches("a?c", "a/c"));
        // A trailing star may match nothing at all.
        assert!(glob_matches("abc*", "abc"));
        assert!(glob_matches("*", ""));
    }

    #[test]
    fn an_empty_query_keeps_everything() {
        let q = RowQuery::parse("   ", COLUMNS).expect("empty is a query");
        assert!(q.is_everything());
        assert!(q.filter().is_none());
        assert_eq!(q.admit(row(&["anything"])), Admission::Admitted);
    }

    #[test]
    fn an_unknown_column_says_which_names_would_have_worked() {
        let err = RowQuery::parse("nod = n1", COLUMNS).expect_err("no such column");
        match err {
            QueryError::UnknownColumn {
                ref name,
                ref known,
            } => {
                assert_eq!(name, "nod");
                assert_eq!(known.len(), COLUMNS.len());
            }
            other => panic!("expected an unknown column, got {other:?}"),
        }
        assert!(err.to_string().contains("time"), "{err}");
    }

    #[test]
    fn the_malformed_shapes_each_get_their_own_refusal() {
        assert!(matches!(
            RowQuery::parse("name", COLUMNS),
            Err(QueryError::NoOperator { .. })
        ));
        assert!(matches!(
            RowQuery::parse("name =", COLUMNS),
            Err(QueryError::NoOperand { .. })
        ));
        assert!(matches!(
            RowQuery::parse("name = \"abc", COLUMNS),
            Err(QueryError::UnclosedQuote { .. })
        ));
        assert!(matches!(
            RowQuery::parse("type in (Data", COLUMNS),
            Err(QueryError::UnclosedGroup { .. })
        ));
        assert!(matches!(
            RowQuery::parse("type = Data and", COLUMNS),
            Err(QueryError::EmptyClause)
        ));
    }

    #[test]
    fn and_inside_an_operand_is_not_a_separator() {
        let q = RowQuery::parse("name = \"a and b\"", COLUMNS).expect("well formed");
        assert_eq!(q.clauses().len(), 1);
        assert_eq!(q.clauses()[0].operand, "a and b");

        let set = RowQuery::parse("type in (Data, Command)", COLUMNS).expect("well formed");
        assert_eq!(set.clauses().len(), 1);
    }

    #[test]
    fn a_column_name_containing_an_operator_word_still_resolves() {
        // `in` is a whole word, so `interface` is a name and not `i` + `in`.
        let q = RowQuery::parse("interface = if0", &["interface", "rate"]).expect("well formed");
        assert_eq!(q.clauses()[0].column, "interface");
        assert_eq!(q.clauses()[0].op, FilterOp::Eq);
    }

    #[test]
    fn a_header_is_matched_however_it_is_cased() {
        let q = RowQuery::parse("Type = Data", COLUMNS).expect("well formed");
        assert_eq!(q.clauses()[0].col, 2);
        assert_eq!(
            q.clauses()[0].column,
            "type",
            "the roster's spelling is what is stored, so normal form is stable"
        );
    }
}
