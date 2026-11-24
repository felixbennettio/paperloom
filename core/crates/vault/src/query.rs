//! `query` blocks: a note asks for notes (or library items) by tag, folder
//! and property, and the reading view shows them as a list, a table or a
//! board.
//!
//! ````text
//! ```query
//! from: #reading
//! where: status = to read
//! where: year >= 2020
//! sort: year desc
//! show: table title, status, year
//! limit: 20
//! ```
//! ````
//!
//! `from:` takes `#tag` and a folder (`Papers/2024`), or `library` for
//! the connected library's items, with `#tag` and a collection name too
//! (`library #ml "To Read"`). `where:` lines all hold: `=`, `!=`, `<`, `>`,
//! `<=`, `>=` (numbers compare as numbers) and `contains`. `show:` takes
//! `list`, `table` with its columns, `board` with the field to group by
//! and the fields cards show (`board status, year`), or `calendar` with the
//! date field to place them by (`calendar due`; `date` if none).

use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::Vault;
use crate::render::{encode, escape};

/// What a query block asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub library: bool,
    pub tag: Option<String>,
    /// A folder of notes, or a collection of the library.
    pub folder: Option<String>,
    pub filters: Vec<Filter>,
    /// A field, and whether newest (largest) first.
    pub sort: Option<(String, bool)>,
    pub show: Show,
    pub limit: usize,
}

/// How the results look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Show {
    List,
    /// A table with these columns.
    Table(Vec<String>),
    /// A column for each value of `by`, its cards showing `fields`.
    Board {
        by: String,
        fields: Vec<String>,
    },
    /// Months with each row on its day, by a `YYYY-MM-DD` field.
    Calendar {
        by: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    Contains,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    pub field: String,
    pub op: Op,
    pub value: String,
}

/// One result: where it leads (`paperloom:note/…`, `paperloom:item/…`),
/// its title, and its fields by lower-cased name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub href: String,
    pub title: String,
    pub fields: BTreeMap<String, String>,
}

/// Reads a query block; the message says what is wrong.
pub fn parse_query(body: &str) -> Result<Query, String> {
    let mut query = Query {
        library: false,
        tag: None,
        folder: None,
        filters: Vec::new(),
        sort: None,
        show: Show::List,
        limit: 50,
    };
    for line in body.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("//")) {
        let Some((key, value)) = line.split_once(':') else {
            return Err(format!("“{line}” is not “key: value”."));
        };
        let value = value.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            "from" => {
                for part in words(value) {
                    if part.eq_ignore_ascii_case("library") {
                        query.library = true;
                    } else if let Some(tag) = part.strip_prefix('#') {
                        query.tag = Some(tag.to_string());
                    } else {
                        query.folder = Some(part.trim_matches('/').to_string());
                    }
                }
            }
            "where" => query.filters.push(filter(value)?),
            "sort" => {
                let mut parts = value.split_whitespace();
                let field = parts.next().ok_or("“sort:” needs a field.")?.to_ascii_lowercase();
                let descending = parts.next().is_some_and(|w| w.eq_ignore_ascii_case("desc"));
                query.sort = Some((field, descending));
            }
            "show" => {
                let (kind, rest) = value.split_once(char::is_whitespace).unwrap_or((value, ""));
                let fields: Vec<String> = rest
                    .split(',')
                    .map(|c| c.trim().to_ascii_lowercase())
                    .filter(|c| !c.is_empty())
                    .collect();
                query.show = match kind.to_ascii_lowercase().as_str() {
                    "list" => Show::List,
                    "table" if fields.is_empty() => Show::Table(vec!["title".into()]),
                    "table" => Show::Table(fields),
                    "board" => {
                        let mut fields = fields.into_iter();
                        let by = fields.next().ok_or("“show: board” needs a field to group by.")?;
                        Show::Board { by, fields: fields.collect() }
                    }
                    "calendar" => {
                        Show::Calendar { by: fields.into_iter().next().unwrap_or_else(|| "date".into()) }
                    }
                    _ => {
                        return Err(format!(
                            "“show: {value}” is not “list”, “table …”, “board …” or “calendar …”."
                        ));
                    }
                };
            }
            "limit" => {
                query.limit = value.parse().map_err(|_| format!("“limit: {value}” is not a number."))?
            }
            other => return Err(format!("“{other}:” is not something a query takes.")),
        }
    }
    Ok(query)
}

/// Words, with `"quoted words"` kept together.
fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (n, part) in text.split('"').enumerate() {
        if n % 2 == 1 {
            out.push(part.to_string());
        } else {
            out.extend(part.split_whitespace().map(str::to_string));
        }
    }
    out.retain(|w| !w.is_empty());
    out
}

fn filter(text: &str) -> Result<Filter, String> {
    const OPS: [(&str, Op); 7] = [
        (" contains ", Op::Contains),
        ("!=", Op::Ne),
        ("<=", Op::Le),
        (">=", Op::Ge),
        ("=", Op::Eq),
        ("<", Op::Lt),
        (">", Op::Gt),
    ];
    for (symbol, op) in OPS {
        if let Some((field, value)) = text.split_once(symbol) {
            let field = field.trim().to_ascii_lowercase();
            if !field.is_empty() {
                let value = value.trim().trim_matches('"').to_string();
                return Ok(Filter { field, op, value });
            }
        }
    }
    Err(format!("“where: {text}” has no comparison (=, !=, <, >, <=, >=, contains)."))
}

/// Numbers compare as numbers, anything else as text (ignoring case).
fn compare(a: &str, b: &str) -> Ordering {
    match (a.trim().parse::<f64>(), b.trim().parse::<f64>()) {
        (Ok(x), Ok(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        _ => a.to_lowercase().cmp(&b.to_lowercase()),
    }
}

impl Filter {
    fn holds(&self, row: &Row) -> bool {
        let value = row.fields.get(&self.field).map(String::as_str).unwrap_or_default();
        match self.op {
            Op::Contains => value.to_lowercase().contains(&self.value.to_lowercase()),
            Op::Eq => compare(value, &self.value) == Ordering::Equal,
            Op::Ne => compare(value, &self.value) != Ordering::Equal,
            Op::Lt => !value.is_empty() && compare(value, &self.value) == Ordering::Less,
            Op::Gt => !value.is_empty() && compare(value, &self.value) == Ordering::Greater,
            Op::Le => !value.is_empty() && compare(value, &self.value) != Ordering::Greater,
            Op::Ge => !value.is_empty() && compare(value, &self.value) != Ordering::Less,
        }
    }
}

/// Whether `tags` hold `tag` or a tag nested in it (`#reading/ml` for
/// `#reading`), ignoring case.
pub fn has_tag(tags: &[String], tag: &str) -> bool {
    let tag = tag.to_lowercase();
    tags.iter().any(|t| {
        let t = t.to_lowercase();
        t == tag || t.strip_prefix(&tag).is_some_and(|rest| rest.starts_with('/'))
    })
}

/// Filters, sorts and cuts rows as the query says.
pub fn select(query: &Query, rows: Vec<Row>) -> Vec<Row> {
    let mut rows: Vec<Row> =
        rows.into_iter().filter(|row| query.filters.iter().all(|f| f.holds(row))).collect();
    if let Some((field, descending)) = &query.sort {
        rows.sort_by(|a, b| {
            let order = compare(
                a.fields.get(field).map(String::as_str).unwrap_or_default(),
                b.fields.get(field).map(String::as_str).unwrap_or_default(),
            );
            if *descending { order.reverse() } else { order }
        });
    }
    rows.truncate(query.limit);
    rows
}

/// The rows as HTML: a list of links, a table whose `title` column links,
/// or a board of cards.
pub fn render_rows(query: &Query, rows: &[Row]) -> String {
    let link =
        |row: &Row| format!(r#"<a class="wikilink" href="{}">{}</a>"#, escape(&row.href), escape(&row.title));
    let field = |row: &Row, name: &str| row.fields.get(name).map(String::as_str).unwrap_or("").to_string();
    match &query.show {
        Show::List | Show::Board { .. } | Show::Calendar { .. } if rows.is_empty() => {
            r#"<p class="query-results empty">Nothing matches.</p>"#.to_string()
        }
        Show::List => {
            let items: String = rows.iter().map(|row| format!("<li>{}</li>", link(row))).collect();
            format!(r#"<ul class="query-results">{items}</ul>"#)
        }
        Show::Calendar { by } => calendar(rows, by, &link),
        Show::Board { by, fields } => {
            // Columns in the order their first card comes.
            let mut columns: Vec<(String, Vec<&Row>)> = Vec::new();
            for row in rows {
                let value = field(row, by);
                match columns.iter_mut().find(|(v, _)| *v == value) {
                    Some((_, cards)) => cards.push(row),
                    None => columns.push((value, vec![row])),
                }
            }
            let html: String = columns
                .iter()
                .map(|(value, members)| {
                    let cards: String = members
                        .iter()
                        .map(|row| {
                            let shown: Vec<String> =
                                fields.iter().map(|f| field(row, f)).filter(|v| !v.is_empty()).collect();
                            let details = if shown.is_empty() {
                                String::new()
                            } else {
                                format!("<small>{}</small>", escape(&shown.join(" · ")))
                            };
                            format!("<li>{}{details}</li>", link(row))
                        })
                        .collect();
                    let name = if value.is_empty() { "—" } else { value.as_str() };
                    format!(
                        r#"<section class="query-column"><h4>{} <span>{}</span></h4><ul>{cards}</ul></section>"#,
                        escape(name),
                        members.len()
                    )
                })
                .collect();
            format!(r#"<div class="query-board">{html}</div>"#)
        }
        Show::Table(columns) => {
            let head: String = columns.iter().map(|c| format!("<th>{}</th>", escape(c))).collect();
            let body: String = rows
                .iter()
                .map(|row| {
                    let cells: String = columns
                        .iter()
                        .map(|c| match c.as_str() {
                            "title" => format!("<td>{}</td>", link(row)),
                            _ => format!("<td>{}</td>", escape(&field(row, c))),
                        })
                        .collect();
                    format!("<tr>{cells}</tr>")
                })
                .collect();
            format!(
                r#"<table class="query-results"><thead><tr>{head}</tr></thead><tbody>{body}</tbody></table>"#
            )
        }
    }
}

/// Days from 1970-01-01 to a date (negative before).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The most months a calendar shows.
const MONTHS: usize = 24;

/// A month's rows, by day.
type Month<'a> = BTreeMap<i64, Vec<&'a Row>>;

/// Months as tables of weeks (Monday first), each row on its day.
fn calendar(rows: &[Row], by: &str, link: &dyn Fn(&Row) -> String) -> String {
    let mut days: BTreeMap<(i64, i64), Month> = BTreeMap::new();
    for row in rows {
        let Some(value) = row.fields.get(by) else { continue };
        let parts: Vec<i64> =
            value.get(..10).unwrap_or("").split('-').filter_map(|p| p.parse().ok()).collect();
        if let [year, month, day] = parts[..]
            && (1..=12).contains(&month)
            && (1..=days_in_month(year, month)).contains(&day)
        {
            days.entry((year, month)).or_default().entry(day).or_default().push(row);
        }
    }
    if days.is_empty() {
        return r#"<p class="query-results empty">Nothing matches.</p>"#.into();
    }
    let mut out = String::from(r#"<div class="query-calendar">"#);
    for ((year, month), on) in days.iter().take(MONTHS) {
        // Monday is 0; 1970-01-01 was a Thursday.
        let first = (days_from_civil(*year, *month, 1) + 3).rem_euclid(7);
        let length = days_in_month(*year, *month);
        out.push_str(&format!(
            "<section><h4>{year:04}-{month:02}</h4><table><thead><tr><th>Mo</th><th>Tu</th><th>We</th><th>Th</th><th>Fr</th><th>Sa</th><th>Su</th></tr></thead><tbody><tr>"
        ));
        out.push_str(&"<td></td>".repeat(first as usize));
        for day in 1..=length {
            let column = (first + day - 1) % 7;
            if column == 0 && day > 1 {
                out.push_str("</tr><tr>");
            }
            let entries: String =
                on.get(&day).map(|rows| rows.iter().map(|r| link(r)).collect()).unwrap_or_default();
            out.push_str(&format!(r#"<td><span class="day">{day}</span>{entries}</td>"#));
        }
        let last = (first + length - 1) % 7;
        out.push_str(&"<td></td>".repeat((6 - last) as usize));
        out.push_str("</tr></tbody></table></section>");
    }
    out.push_str("</div>");
    out
}

/// `2026-10-12` (UTC) for milliseconds since 1970.
pub fn date_of(ms: i64) -> String {
    let z = ms.div_euclid(86_400_000) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{m:02}-{d:02}", yoe + era * 400 + i64::from(m <= 2))
}

impl Vault {
    /// The notes a query asks for, as rows: `title`, `path`, `folder`,
    /// `modified`, `tags` and every property.
    pub fn note_rows(&self, query: &Query) -> Vec<Row> {
        let folder = query.folder.as_deref().map(|f| format!("{}/", f.to_lowercase()));
        let rows = self
            .notes()
            .filter(|note| folder.as_deref().is_none_or(|f| note.path.to_lowercase().starts_with(f)))
            .filter(|note| query.tag.as_deref().is_none_or(|tag| has_tag(&note.parsed.tags, tag)))
            .map(|note| {
                let mut fields: BTreeMap<String, String> =
                    note.parsed.properties.iter().map(|(k, v)| (k.to_lowercase(), v.clone())).collect();
                fields.insert("title".into(), note.title.clone());
                fields.insert("path".into(), note.path.clone());
                fields.insert("folder".into(), note.folder().to_string());
                fields.insert("modified".into(), date_of(note.modified));
                fields.insert("tags".into(), note.parsed.tags.join(", "));
                Row {
                    href: format!("paperloom:note/{}", encode(&note.path)),
                    title: note.title.clone(),
                    fields,
                }
            })
            .collect();
        select(query, rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_queries() {
        let q = parse_query(
            "from: #reading Papers/\nwhere: status = to read\nwhere: year >= 2020\nsort: year desc\nshow: table title, Year\nlimit: 5",
        )
        .unwrap();
        assert_eq!(
            (q.tag.as_deref(), q.folder.as_deref(), q.library),
            (Some("reading"), Some("Papers"), false)
        );
        assert_eq!(q.filters[0], Filter { field: "status".into(), op: Op::Eq, value: "to read".into() });
        assert_eq!(q.filters[1].op, Op::Ge);
        assert_eq!(q.sort, Some(("year".into(), true)));
        assert_eq!(q.show, Show::Table(vec!["title".to_string(), "year".into()]));
        let board = parse_query("show: board Status, year").unwrap();
        assert_eq!(board.show, Show::Board { by: "status".into(), fields: vec!["year".into()] });
        assert!(parse_query("show: board").is_err());
        assert_eq!(parse_query("show: calendar").unwrap().show, Show::Calendar { by: "date".into() });
        assert_eq!(parse_query("show: calendar Due").unwrap().show, Show::Calendar { by: "due".into() });
        assert_eq!(q.limit, 5);
        assert!(parse_query("from: library #ml\ntitle contains x").is_err());
        assert!(parse_query("show: chart").is_err());
        let q = parse_query("from: library #ml \"To Read\"").unwrap();
        assert_eq!((q.library, q.tag.as_deref(), q.folder.as_deref()), (true, Some("ml"), Some("To Read")));
        assert!(
            has_tag(&["Reading/ML".to_string()], "reading") && !has_tag(&["readings".to_string()], "reading")
        );
    }

    #[test]
    fn finds_notes_by_tag_folder_and_properties() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = Vault::open(dir.path()).unwrap();
        vault.write("Papers/Old.md", "---\nyear: 2019\nstatus: read\n---\n#reading").unwrap();
        vault.write("Papers/New.md", "---\nyear: 2024\nstatus: to read\n---\n#reading/ml").unwrap();
        vault.write("Papers/Mid.md", "---\nyear: 2021\nstatus: to read\n---\n#reading").unwrap();
        vault.write("Ideas.md", "---\nyear: 2025\n---\n#reading").unwrap();
        let q = parse_query(
            "from: #reading Papers\nwhere: year >= 2020\nsort: year desc\nshow: table title, year",
        )
        .unwrap();
        let rows = vault.note_rows(&q);
        let titles: Vec<&str> = rows.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, ["New", "Mid"], "nested tags count, other folders do not");
        let html = render_rows(&q, &rows);
        assert!(
            html.starts_with(r#"<table class="query-results"><thead><tr><th>title</th><th>year</th>"#),
            "{html}"
        );
        assert!(
            html.contains(
                r#"<a class="wikilink" href="paperloom:note/Papers/New.md">New</a></td><td>2024</td>"#
            ),
            "{html}"
        );
        let list = parse_query("where: status contains READ\nsort: title").unwrap();
        let titles: Vec<String> = vault.note_rows(&list).into_iter().map(|r| r.title).collect();
        assert_eq!(titles, ["Mid", "New", "Old"]);
        let html = vault.render("Index.md", "Before\n\n```query\nfrom: #reading/ml\n```\n\nAfter");
        assert!(html.contains(r#"<ul class="query-results"><li><a class="wikilink" href="paperloom:note/Papers/New.md">New</a></li></ul>"#), "{html}");
        assert!(html.contains("<p>After</p>") && !html.contains("<code"), "{html}");
        let row = |title: &str, date: &str| Row {
            href: format!("paperloom:note/{title}.md"),
            title: title.into(),
            fields: [("date".to_string(), date.to_string())].into(),
        };
        let rows = [row("Kickoff", "2026-10-12"), row("Review", "2026-11-30T09:00"), row("Someday", "soon")];
        let html = render_rows(&parse_query("show: calendar").unwrap(), &rows);
        assert_eq!(html.matches("<section>").count(), 2, "{html}");
        // 2026-10-01 is a Thursday: three empty days, then the 1st.
        assert!(html.contains("<h4>2026-10</h4><table><thead>"), "{html}");
        assert!(
            html.contains("<tbody><tr><td></td><td></td><td></td><td><span class=\"day\">1</span></td>"),
            "{html}"
        );
        assert!(html.contains(r#"<td><span class="day">12</span><a class="wikilink" href="paperloom:note/Kickoff.md">Kickoff</a></td>"#), "{html}");
        assert!(html.contains("Review") && !html.contains("Someday"), "{html}");
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2026, 10, 12), 20_738);
        assert_eq!((days_in_month(2024, 2), days_in_month(2100, 2), days_in_month(2026, 4)), (29, 28, 30));
        let board = parse_query("from: Papers\nsort: title\nshow: board status, year").unwrap();
        let html = render_rows(&board, &vault.note_rows(&board));
        assert!(html.starts_with(r#"<div class="query-board"><section class="query-column"><h4>to read <span>2</span></h4><ul><li><a class="wikilink" href="paperloom:note/Papers/Mid.md">Mid</a><small>2021</small></li>"#), "{html}");
        assert!(html.contains(r#"<h4>read <span>1</span></h4>"#), "{html}");
        let html = vault.render("Index.md", "```query\nfrom: library\n```");
        assert!(html.contains("not connected to a library"), "{html}");
        assert_eq!((date_of(0).as_str(), date_of(1_791_763_200_000).as_str()), ("1970-01-01", "2026-10-12"));
    }
}
