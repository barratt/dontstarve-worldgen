//! `--where` expressions over a world summary.
//!
//! Grammar: `cond ( '&&' cond | '||' cond )*`, evaluated left to right with
//! `&&` binding tighter than `||`. `cond` is `path op literal`, `has path`
//! or `!has path`. `path` is dotted (`nearest.pigking.distance`, `prefabs.beefalo`);
//! `op` is one of `>= <= > < == !=`; literals are numbers or quoted strings.
//! Counts under `prefabs.` default to 0 when the prefab is absent; any other
//! missing path makes the comparison false. `contains` tests list
//! membership: `task_names contains "Befriend the pigs"`.

use serde_json::Value as Json;

#[derive(Debug, Clone)]
enum Literal {
    Num(f64),
    Str(String),
}

#[derive(Debug, Clone)]
enum Cond {
    Cmp { path: String, op: String, lit: Literal },
    Has { path: String, negate: bool },
    Contains { path: String, lit: Literal },
}

#[derive(Debug, Clone)]
pub struct Filter {
    // Disjunction of conjunctions.
    any: Vec<Vec<Cond>>,
}

fn tokenize(s: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '"' || c == '\'' {
            let q = c;
            let mut j = i + 1;
            while j < chars.len() && chars[j] != q {
                j += 1;
            }
            if j >= chars.len() {
                return Err("unterminated string".into());
            }
            out.push(format!("\"{}", chars[i + 1..j].iter().collect::<String>()));
            i = j + 1;
        } else if "<>=!&|".contains(c) {
            let mut j = i;
            while j < chars.len() && "<>=!&|".contains(chars[j]) {
                j += 1;
            }
            out.push(chars[i..j].iter().collect());
            i = j;
        } else {
            let mut j = i;
            while j < chars.len() && !chars[j].is_whitespace() && !"<>=!&|\"'".contains(chars[j]) {
                j += 1;
            }
            out.push(chars[i..j].iter().collect());
            i = j;
        }
    }
    Ok(out)
}

fn literal(tok: &str) -> Result<Literal, String> {
    if let Some(s) = tok.strip_prefix('"') {
        return Ok(Literal::Str(s.to_string()));
    }
    tok.parse::<f64>().map(Literal::Num).map_err(|_| format!("expected a number or quoted string, got {tok}"))
}

impl Filter {
    pub fn parse(expr: &str) -> Result<Filter, String> {
        let toks = tokenize(expr)?;
        let mut any = vec![vec![]];
        let mut i = 0;
        while i < toks.len() {
            let cond = if toks[i] == "has" || (toks[i] == "!" && toks.get(i + 1).map(String::as_str) == Some("has")) {
                let negate = toks[i] == "!";
                i += if negate { 2 } else { 1 };
                let path = toks.get(i).ok_or("has: path expected")?.clone();
                i += 1;
                Cond::Has { path, negate }
            } else if toks[i] == "!has" {
                let path = toks.get(i + 1).ok_or("!has: path expected")?.clone();
                i += 2;
                Cond::Has { path, negate: true }
            } else {
                let path = toks[i].clone();
                let op = toks.get(i + 1).ok_or_else(|| format!("operator expected after {path}"))?.clone();
                let lit = literal(toks.get(i + 2).ok_or_else(|| format!("value expected after {path} {op}"))?)?;
                i += 3;
                if op == "contains" {
                    Cond::Contains { path, lit }
                } else if [">=", "<=", ">", "<", "==", "!="].contains(&op.as_str()) {
                    Cond::Cmp { path, op, lit }
                } else {
                    return Err(format!("unknown operator {op}"));
                }
            };
            any.last_mut().unwrap().push(cond);
            match toks.get(i).map(String::as_str) {
                None => break,
                Some("&&") | Some("and") => i += 1,
                Some("||") | Some("or") => {
                    any.push(vec![]);
                    i += 1;
                }
                Some(other) => return Err(format!("expected && or ||, got {other}")),
            }
        }
        if any.iter().any(Vec::is_empty) {
            return Err("empty condition".into());
        }
        Ok(Filter { any })
    }

    pub fn matches(&self, doc: &Json) -> bool {
        self.any.iter().any(|all| all.iter().all(|c| eval(c, doc)))
    }
}

fn lookup<'a>(doc: &'a Json, path: &str) -> Option<&'a Json> {
    let mut cur = doc;
    for part in path.split('.') {
        cur = match cur {
            Json::Object(m) => m.get(part)?,
            Json::Array(a) => a.get(part.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cur)
}

fn eval(c: &Cond, doc: &Json) -> bool {
    match c {
        Cond::Has { path, negate } => lookup(doc, path).is_some_and(|v| !v.is_null()) != *negate,
        Cond::Contains { path, lit } => match (lookup(doc, path), lit) {
            (Some(Json::Array(a)), Literal::Str(s)) => a.iter().any(|v| v.as_str() == Some(s)),
            (Some(Json::Array(a)), Literal::Num(n)) => a.iter().any(|v| v.as_f64() == Some(*n)),
            _ => false,
        },
        Cond::Cmp { path, op, lit } => {
            let value = lookup(doc, path).cloned().or_else(|| path.starts_with("prefabs.").then(|| Json::from(0)));
            let Some(value) = value else { return false };
            match lit {
                Literal::Num(n) => {
                    let Some(v) = value.as_f64() else { return false };
                    match op.as_str() {
                        ">=" => v >= *n,
                        "<=" => v <= *n,
                        ">" => v > *n,
                        "<" => v < *n,
                        "==" => v == *n,
                        "!=" => v != *n,
                        _ => false,
                    }
                }
                Literal::Str(s) => {
                    let Some(v) = value.as_str() else { return false };
                    match op.as_str() {
                        "==" => v == s,
                        "!=" => v != s,
                        _ => false,
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn filters() {
        let doc = json!({ "prefabs": { "pigking": 1 }, "nearest": { "pigking": { "distance": 120.5 } },
                          "task_names": ["Befriend the pigs"], "dlc": "rog" });
        assert!(Filter::parse("prefabs.pigking >= 1 && nearest.pigking.distance < 200").unwrap().matches(&doc));
        assert!(Filter::parse("prefabs.walrus_camp == 0").unwrap().matches(&doc));
        assert!(!Filter::parse("nearest.walrus_camp.distance < 100").unwrap().matches(&doc));
        assert!(Filter::parse("task_names contains 'Befriend the pigs' and dlc == \"rog\"").unwrap().matches(&doc));
        assert!(Filter::parse("prefabs.pigking > 5 || has nearest.pigking").unwrap().matches(&doc));
        assert!(Filter::parse("!has nearest.walrus_camp").unwrap().matches(&doc));
    }
}
