//! Validate algebra, including nested EXISTS/subqueries: never trust keyword scanning.
use super::{RDF, SchemaCard};
use spargebra::{Query, SparqlParser, algebra::*, term::*};

/// After parsing, inspect canonical SSE IRI tokens (quoted literals are skipped). Discovered
/// entities remain variables; constants in a model query must come from the linking boundary.
pub fn linked_entities(query: &str, linked: &[super::LinkedEntity]) -> Result<(), String> {
    let query = SparqlParser::new().parse_query(query).map_err(|e| e.to_string())?;
    let sse = query.to_sse();
    let bytes = sse.as_bytes();
    let mut i = 0;
    let allowed: std::collections::BTreeSet<_> = linked
        .iter()
        .flat_map(|e| [super::iri("id", &e.id), super::iri("edge", &e.id)])
        .collect();
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i += 2;
                    } else if bytes[i] == b'"' {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
            }
            b'<' if bytes.get(i + 1).is_some_and(|b| !b.is_ascii_whitespace() && *b != b'=') => {
                let start = i;
                i += 1;
                while i < bytes.len() && bytes[i] != b'>' {
                    i += 1;
                }
                if i < bytes.len() {
                    i += 1;
                }
                let token = &sse[start..i];
                if (token.starts_with(&format!("<{}id/", super::BASE))
                    || token.starts_with(&format!("<{}edge/", super::BASE)))
                    && !allowed.contains(token)
                {
                    return Err(format!("model query constant was not pre-linked: {token}"));
                }
            }
            _ => i += 1,
        }
    }
    if !allowed.is_empty() {
        let pattern = match &query {
            Query::Select { pattern, .. } | Query::Ask { pattern, .. } => pattern,
            _ => return Err("model answers require SELECT or ASK".into()),
        };
        for branch in mandatory_branches(pattern, &allowed)? {
            branch.validate(&std::collections::BTreeSet::new())?;
        }
    }
    Ok(())
}

/// Positive joins must establish relevance before OPTIONAL fields are considered.
/// Each UNION alternative is checked separately; a disconnected VALUES table is
/// not evidence that an otherwise global result concerns the user's entity.
#[derive(Clone, Default)]
struct Grounding {
    links: Vec<(String, String)>,
    required: std::collections::BTreeSet<String>,
    anchors: std::collections::BTreeSet<String>,
    optional: Vec<Vec<Grounding>>,
    data: bool,
}
impl Grounding {
    fn validate(&self, inherited: &std::collections::BTreeSet<String>) -> Result<(), String> {
        let mut reached = self.anchors.clone();
        reached.extend(inherited.iter().cloned());
        loop {
            let size = reached.len();
            for (a, b) in &self.links {
                if reached.contains(a) {
                    reached.insert(b.clone());
                }
                if reached.contains(b) {
                    reached.insert(a.clone());
                }
            }
            if reached.len() == size {
                break;
            }
        }
        if !self.required.is_subset(&reached) || (self.data && self.required.intersection(&reached).next().is_none()) {
            return Err("results are not joined to a pre-linked target in every mandatory UNION branch; join the target to result rows before OPTIONAL fields".into());
        }
        // Only mandatory bindings of the enclosing group can ground an optional group.
        for alternatives in &self.optional {
            for branch in alternatives {
                branch.validate(&reached)?;
            }
        }
        Ok(())
    }
    fn add(&mut self, other: Self) {
        self.links.extend(other.links);
        self.required.extend(other.required);
        self.anchors.extend(other.anchors);
        self.optional.extend(other.optional);
        self.data |= other.data;
    }
    fn term(&mut self, term: &TermPattern, allowed: &std::collections::BTreeSet<String>) -> Option<String> {
        match term {
            TermPattern::Variable(v) => Some(v.to_string()),
            TermPattern::NamedNode(n) if allowed.contains(&n.to_string()) => {
                let key = n.to_string();
                self.anchors.insert(key.clone());
                Some(key)
            }
            _ => None,
        }
    }
    fn triple(&mut self, subject: &TermPattern, object: &TermPattern, allowed: &std::collections::BTreeSet<String>) {
        self.data = true;
        let a = self.term(subject, allowed);
        let b = self.term(object, allowed);
        self.required.extend(a.iter().chain(b.iter()).cloned());
        if let (Some(a), Some(b)) = (a, b) {
            self.links.push((a, b));
        }
    }
    fn equality(&mut self, expression: &Expression, allowed: &std::collections::BTreeSet<String>) {
        match expression {
            Expression::And(a, b) => {
                self.equality(a, allowed);
                self.equality(b, allowed);
            }
            Expression::Equal(a, b) | Expression::SameTerm(a, b) => {
                let mut node = |e: &Expression| match e {
                    Expression::Variable(v) => Some(v.to_string()),
                    Expression::NamedNode(n) if allowed.contains(&n.to_string()) => {
                        let k = n.to_string();
                        self.anchors.insert(k.clone());
                        Some(k)
                    }
                    _ => None,
                };
                if let (Some(a), Some(b)) = (node(a), node(b)) {
                    self.links.push((a, b));
                }
            }
            _ => (),
        }
    }
}
fn mandatory_branches(
    p: &GraphPattern,
    allowed: &std::collections::BTreeSet<String>,
) -> Result<Vec<Grounding>, String> {
    use GraphPattern::*;
    let one = |g| Ok(vec![g]);
    match p {
        Bgp { patterns } => {
            let mut g = Grounding::default();
            for t in patterns {
                g.triple(&t.subject, &t.object, allowed);
            }
            one(g)
        }
        Path { subject, object, .. } => {
            let mut g = Grounding::default();
            g.triple(subject, object, allowed);
            one(g)
        }
        Join { left, right } => {
            let a = mandatory_branches(left, allowed)?;
            let b = mandatory_branches(right, allowed)?;
            if a.len().saturating_mul(b.len()) > 64 {
                return Err("too many query grounding alternatives".into());
            }
            Ok(a.into_iter()
                .flat_map(|a| {
                    b.iter().map(move |b| {
                        let mut g = a.clone();
                        g.add(b.clone());
                        g
                    })
                })
                .collect())
        }
        Union { left, right } => {
            let mut a = mandatory_branches(left, allowed)?;
            a.extend(mandatory_branches(right, allowed)?);
            if a.len() > 64 {
                return Err("too many query grounding alternatives".into());
            }
            Ok(a)
        }
        LeftJoin { left, right, .. } => {
            let mut a = mandatory_branches(left, allowed)?;
            let optional = mandatory_branches(right, allowed)?;
            for branch in &mut a {
                branch.optional.push(optional.clone());
            }
            Ok(a)
        }
        Minus { left, .. } => mandatory_branches(left, allowed),
        Filter { expr, inner } => {
            let mut a = mandatory_branches(inner, allowed)?;
            for g in &mut a {
                g.equality(expr, allowed);
            }
            Ok(a)
        }
        Extend {
            inner,
            variable,
            expression,
        } => {
            let mut a = mandatory_branches(inner, allowed)?;
            for g in &mut a {
                if let Expression::Variable(v) = expression {
                    g.links.push((variable.to_string(), v.to_string()));
                }
                if let Expression::NamedNode(n) = expression
                    && allowed.contains(&n.to_string())
                {
                    g.anchors.insert(variable.to_string());
                }
            }
            Ok(a)
        }
        Values { variables, bindings } => {
            let mut g = Grounding::default();
            for (i, v) in variables.iter().enumerate() {
                if !bindings.is_empty() && bindings.iter().all(|row|matches!(row.get(i),Some(Some(GroundTerm::NamedNode(n))) if allowed.contains(&n.to_string()))) {
                    g.anchors.insert(v.to_string());
                }
            }
            one(g)
        }
        Project { inner, .. }
        | Distinct { inner }
        | Reduced { inner }
        | Slice { inner, .. }
        | OrderBy { inner, .. }
        | Group { inner, .. } => mandatory_branches(inner, allowed),
        Graph { .. } | Service { .. } => Err("named graphs and SERVICE cannot establish target grounding".into()),
    }
}

pub fn checked(query: &str, schema: &SchemaCard, cap: usize) -> Result<String, String> {
    if let Some(query) = compiled_neighborhood(query, schema, cap)? {
        return Ok(query);
    }
    if query.len() > 16 * 1024 || cap == 0 || cap > 100 {
        return Err("query size or row cap exceeded".into());
    }
    let mut q = SparqlParser::new()
        .parse_query(query)
        .map_err(|e| format!("invalid SPARQL: {e}"))?;
    let pattern = match &mut q {
        Query::Select {
            dataset: None, pattern, ..
        }
        | Query::Ask {
            dataset: None, pattern, ..
        } => pattern,
        _ => return Err("only SELECT/ASK over the default dataset are allowed".into()),
    };
    check_pattern(pattern, schema)?;
    if let Query::Select { pattern, .. } = &mut q {
        // The serializer flattens consecutive slices; wrapping an existing LIMIT would
        // let its inner value overwrite our cap. Clamp its root modifier instead.
        if !clamp_root_slice(pattern, cap + 1) {
            *pattern = GraphPattern::Slice {
                inner: Box::new(pattern.clone()),
                start: 0,
                length: Some(cap + 1),
            };
        }
    }
    Ok(q.to_string())
}

/// Narrow authorization for the server's deterministic indexed-neighborhood
/// compiler. Its variable predicate is finite, schema-known and bound before
/// each constant-subject/object triple. Reconstructing the entire template
/// excludes SERVICE, updates, OPTIONAL scans, extra branches and hidden clauses.
/// General edited queries still use the algebra guard and its 100-row limit.
fn compiled_neighborhood(query: &str, schema: &SchemaCard, cap: usize) -> Result<Option<String>, String> {
    const PREFIX: &str = "PREFIX ra: <https://w3id.org/rare-disease-atlas/vocab#>\nPREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>\nSELECT DISTINCT ?s ?p ?o WHERE { ";
    let text = query.trim();
    if !text.to_ascii_uppercase().contains("VALUES") || !text.contains("?p") {
        return Ok(None);
    }
    if query.len() > 16 * 1024 || cap == 0 || cap > 160 {
        return Err("query size or row cap exceeded".into());
    }
    let parsed = SparqlParser::new()
        .parse_query(query)
        .map_err(|e| format!("invalid SPARQL: {e}"))?;
    fn root_limit(pattern: &GraphPattern) -> Option<usize> {
        match pattern {
            GraphPattern::Slice {
                start: 0,
                length: Some(limit),
                ..
            } => Some(*limit),
            GraphPattern::Project { inner, .. } | GraphPattern::Distinct { inner } => root_limit(inner),
            _ => None,
        }
    }
    let limit = match &parsed {
        Query::Select {
            dataset: None, pattern, ..
        } => match root_limit(pattern) {
            Some(limit) => limit,
            None => return Ok(None),
        },
        _ => return Ok(None),
    };
    if limit == 0 || limit > cap || limit > 160 {
        return Err("compiled neighborhood limit exceeds the requested row cap".into());
    }
    let algebra = parsed.to_sse();
    let mut seeds = std::collections::BTreeSet::new();
    let mut predicates = std::collections::BTreeSet::new();
    for token in algebra.split('<').skip(1) {
        let Some((iri, _)) = token.split_once('>') else {
            return Ok(None);
        };
        if let Some(encoded) = iri.strip_prefix(&format!("{}id/", super::BASE)) {
            let mut bytes = Vec::new();
            let source = encoded.as_bytes();
            let mut index = 0;
            while index < source.len() {
                if source[index] == b'%' {
                    let Some(hex) = encoded.get(index + 1..index + 3) else {
                        return Ok(None);
                    };
                    let Ok(byte) = u8::from_str_radix(hex, 16) else {
                        return Ok(None);
                    };
                    bytes.push(byte);
                    index += 3;
                } else {
                    bytes.push(source[index]);
                    index += 1;
                }
            }
            let Ok(id) = String::from_utf8(bytes) else {
                return Ok(None);
            };
            if id.is_empty() || id.len() > 256 || super::iri("id", &id) != format!("<{iri}>") {
                return Ok(None);
            }
            seeds.insert(format!("<{iri}>"));
        } else if let Some(relation) = iri.strip_prefix(super::RA) {
            if relation.is_empty() {
                continue;
            } // Declared prefix, not a predicate.
            if atlas_core::graph::Relation::parse(relation).is_none()
                && ![atlas_journeys::GENE_RELATION, atlas_journeys::phenotype_relation(true)].contains(&relation)
            {
                return Ok(None);
            }
            if !schema.allows_predicate(iri) {
                return Err(format!("unknown predicate: <{iri}>"));
            }
            predicates.insert(format!("<{iri}>"));
        } else if iri != super::RDFS {
            return Ok(None);
        }
    }
    if seeds.is_empty() || seeds.len() > 16 || predicates.is_empty() {
        return Ok(None);
    }
    let values = predicates.into_iter().collect::<Vec<_>>().join(" ");
    let branches = seeds
        .into_iter()
        .flat_map(|seed| {
            [
                format!("{{ VALUES ?p {{ {values} }} {seed} ?p ?o . BIND({seed} AS ?s) }}"),
                format!("{{ VALUES ?p {{ {values} }} ?s ?p {seed} . BIND({seed} AS ?o) }}"),
            ]
        })
        .collect::<Vec<_>>()
        .join(" UNION ");
    let expected = format!("{PREFIX}{branches} }} LIMIT {limit}");
    let expected = SparqlParser::new()
        .parse_query(&expected)
        .map_err(|e| format!("invalid compiled SPARQL: {e}"))?;
    if algebra == expected.to_sse() {
        return Ok(Some(query.to_owned()));
    }
    // The compact compiler factors this one finite predicate table outside the
    // same literal-seed UNION. Only exact algebra equivalence is authorized;
    // variable predicates in arbitrary edited SELECTs remain prohibited.
    let factored_branches = branches.replace(&format!("VALUES ?p {{ {values} }} "), "");
    let factored = format!("{PREFIX}VALUES ?p {{ {values} }} {{ {factored_branches} }} }} LIMIT {limit}");
    let factored = SparqlParser::new()
        .parse_query(&factored)
        .map_err(|e| format!("invalid factored compiled SPARQL: {e}"))?;
    if algebra != factored.to_sse() {
        return Ok(None);
    }
    Ok(Some(query.to_owned()))
}

fn clamp_root_slice(p: &mut GraphPattern, cap: usize) -> bool {
    match p {
        GraphPattern::Slice { length, .. } => {
            *length = Some(length.unwrap_or(cap).min(cap));
            true
        }
        GraphPattern::Project { inner, .. }
        | GraphPattern::Distinct { inner }
        | GraphPattern::Reduced { inner }
        | GraphPattern::OrderBy { inner, .. }
        | GraphPattern::Extend { inner, .. } => clamp_root_slice(inner, cap),
        _ => false,
    }
}

fn predicate(p: &NamedNode, s: &SchemaCard) -> Result<(), String> {
    if s.allows_predicate(p.as_str()) {
        Ok(())
    } else {
        Err(format!("unknown predicate: {p}"))
    }
}
fn path(p: &PropertyPathExpression, s: &SchemaCard) -> Result<(), String> {
    match p {
        PropertyPathExpression::NamedNode(n) => predicate(n, s),
        PropertyPathExpression::Reverse(p)
        | PropertyPathExpression::ZeroOrMore(p)
        | PropertyPathExpression::OneOrMore(p)
        | PropertyPathExpression::ZeroOrOne(p) => path(p, s),
        PropertyPathExpression::Sequence(a, b) | PropertyPathExpression::Alternative(a, b) => {
            path(a, s)?;
            path(b, s)
        }
        PropertyPathExpression::NegatedPropertySet(_) => Err("negated property sets are not schema constrained".into()),
    }
}
fn typed_object(p: &str, o: &TermPattern, s: &SchemaCard) -> Result<(), String> {
    if p == format!("{RDF}type")
        && let TermPattern::NamedNode(n) = o
        && !s.classes.contains_key(n.as_str())
    {
        return Err(format!("unknown class: {n}"));
    }
    Ok(())
}
fn triple(t: &TriplePattern, s: &SchemaCard) -> Result<(), String> {
    let NamedNodePattern::NamedNode(n) = &t.predicate else {
        return Err("variable predicates are disabled; use describe_schema".into());
    };
    predicate(n, s)?;
    typed_object(n.as_str(), &t.object, s)?;
    if let (TermPattern::Variable(a), TermPattern::Variable(b)) = (&t.subject, &t.object)
        && a == b
        && (n.as_str() == format!("{}identifier", super::DCT)
            || s.predicates.get(n.as_str()).is_some_and(|p| {
                !p.observed_range.is_empty()
                    && p.observed_range
                        .iter()
                        .all(|r| r.starts_with("http://www.w3.org/2001/XMLSchema#") || r == &format!("{RDF}langString"))
            }))
    {
        return Err("a resource cannot also be its literal identifier/value; use a separate object variable such as ?identifier".into());
    }
    for term in [&t.subject, &t.object] {
        if let TermPattern::Triple(t) = term {
            triple(t, s)?;
        }
    }
    Ok(())
}
fn expression(e: &Expression, s: &SchemaCard) -> Result<(), String> {
    use Expression::*;
    match e {
        Exists(p) => check_pattern(p, s),
        Or(a, b)
        | And(a, b)
        | Equal(a, b)
        | SameTerm(a, b)
        | Greater(a, b)
        | GreaterOrEqual(a, b)
        | Less(a, b)
        | LessOrEqual(a, b)
        | Add(a, b)
        | Subtract(a, b)
        | Multiply(a, b)
        | Divide(a, b) => {
            expression(a, s)?;
            expression(b, s)
        }
        UnaryPlus(a) | UnaryMinus(a) | Not(a) => expression(a, s),
        If(a, b, c) => {
            expression(a, s)?;
            expression(b, s)?;
            expression(c, s)
        }
        In(a, args) => {
            expression(a, s)?;
            for e in args {
                expression(e, s)?;
            }
            Ok(())
        }
        Coalesce(args) | FunctionCall(_, args) => {
            if matches!(e, FunctionCall(Function::Custom(_), _)) {
                return Err("custom functions are disabled".into());
            }
            for e in args {
                expression(e, s)?;
            }
            Ok(())
        }
        NamedNode(_) | Literal(_) | Variable(_) | Bound(_) => Ok(()),
    }
}
fn check_pattern(p: &GraphPattern, s: &SchemaCard) -> Result<(), String> {
    use GraphPattern::*;
    match p {
        Bgp { patterns } => {
            for t in patterns {
                triple(t, s)?;
            }
            Ok(())
        }
        Path { path: p, object, .. } => {
            path(p, s)?;
            if let PropertyPathExpression::NamedNode(n) = p {
                typed_object(n.as_str(), object, s)?;
            }
            Ok(())
        }
        Join { left, right } | Union { left, right } | Minus { left, right } => {
            check_pattern(left, s)?;
            check_pattern(right, s)
        }
        LeftJoin {
            left,
            right,
            expression: e,
        } => {
            check_pattern(left, s)?;
            check_pattern(right, s)?;
            if let Some(e) = e {
                expression(e, s)?;
            }
            Ok(())
        }
        Filter { expr, inner }
        | Extend {
            expression: expr,
            inner,
            ..
        } => {
            expression(expr, s)?;
            check_pattern(inner, s)
        }
        OrderBy {
            inner,
            expression: exprs,
        } => {
            for e in exprs {
                let (OrderExpression::Asc(e) | OrderExpression::Desc(e)) = e;
                expression(e, s)?;
            }
            check_pattern(inner, s)
        }
        Group { inner, aggregates, .. } => {
            for (_, a) in aggregates {
                if let AggregateExpression::FunctionCall { name, expr, .. } = a {
                    if matches!(name, AggregateFunction::Custom(_)) {
                        return Err("custom aggregate disabled".into());
                    }
                    expression(expr, s)?;
                }
            }
            check_pattern(inner, s)
        }
        Project { inner, variables } => {
            check_pattern(inner, s)?;
            let mut scope = std::collections::BTreeSet::new();
            inner.on_in_scope_variable(|v| {
                scope.insert(v.clone());
            });
            if let Some(variable) = variables.iter().find(|v| !scope.contains(*v)) {
                return Err(format!(
                    "projected variable {variable} is never bound in WHERE; bind it to the actual entity variable (for example BIND(?study AS ?resource)) or remove it"
                ));
            }
            Ok(())
        }
        Distinct { inner } | Reduced { inner } | Slice { inner, .. } => check_pattern(inner, s),
        Values { .. } => Ok(()),
        Service { .. } | Graph { .. } => Err("SERVICE and named graphs are disabled".into()),
    }
}
