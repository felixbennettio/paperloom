//! Library catalogues over Z39.50, the protocol library catalogues have
//! long answered and that EndNote searches them by: national and
//! university libraries that offer no SRU, the National Library of China
//! among them. A search connects, initialises, searches, asks for a page
//! of records (MARC 21 or UNIMARC / CNMARC in ISO 2709, or MARCXML) and
//! hangs up.
//!
//! A catalogue's address is `z3950://host:port/database`, with
//! `user:password@` when it asks for them and `?charset=gbk` when its
//! records are in a character set they do not say. Its index is the Bib-1
//! use attribute words are looked for in (`1016`, any, when empty; or
//! `title`, `author`, `subject`, `isbn`), and its schema the record syntax
//! (`usmarc`, the default, `unimarc`, `cnmarc` or `xml`).

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use encoding_rs::Encoding;

use crate::marc::{read_iso2709, unimarc_hit};
use crate::sru::{self, Catalogue, Record};
use crate::xml;
use crate::{Error, Hit, Page, Query, Result};

// BER, as Z39.50 encodes its messages.

const UNIVERSAL: u8 = 0;
const CONTEXT: u8 = 2;

/// An element's tag and length, before its contents.
fn header(class: u8, constructed: bool, tag: u32, len: usize, out: &mut Vec<u8>) {
    let first = (class << 6) | if constructed { 0x20 } else { 0 };
    if tag < 31 {
        out.push(first | tag as u8);
    } else {
        out.push(first | 0x1f);
        let mut groups = vec![(tag & 0x7f) as u8];
        let mut rest = tag >> 7;
        while rest > 0 {
            groups.push(0x80 | (rest & 0x7f) as u8);
            rest >>= 7;
        }
        out.extend(groups.iter().rev());
    }
    if len < 0x80 {
        out.push(len as u8);
    } else {
        let bytes: Vec<u8> = len.to_be_bytes().into_iter().skip_while(|&b| b == 0).collect();
        out.push(0x80 | bytes.len() as u8);
        out.extend(bytes);
    }
}

fn element(class: u8, constructed: bool, tag: u32, content: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    header(class, constructed, tag, content.len(), &mut out);
    out.extend_from_slice(content);
    out
}

/// `[tag]` holding these elements.
fn tagged(tag: u32, parts: &[Vec<u8>]) -> Vec<u8> {
    element(CONTEXT, true, tag, &parts.concat())
}

fn sequence(parts: &[Vec<u8>]) -> Vec<u8> {
    element(UNIVERSAL, true, 16, &parts.concat())
}

/// An integer's shortest two's complement bytes.
fn integer_bytes(value: i64) -> Vec<u8> {
    let bytes = value.to_be_bytes();
    let mut start = 0;
    while start < 7
        && ((bytes[start] == 0 && bytes[start + 1] & 0x80 == 0)
            || (bytes[start] == 0xff && bytes[start + 1] & 0x80 != 0))
    {
        start += 1;
    }
    bytes[start..].to_vec()
}

fn integer(tag: u32, value: i64) -> Vec<u8> {
    element(CONTEXT, false, tag, &integer_bytes(value))
}

fn string(tag: u32, value: &[u8]) -> Vec<u8> {
    element(CONTEXT, false, tag, value)
}

fn boolean(tag: u32, value: bool) -> Vec<u8> {
    element(CONTEXT, false, tag, &[if value { 0xff } else { 0 }])
}

/// A bit string of `count` bits, those in `set` on.
fn bits(tag: u32, set: &[usize], count: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; count.div_ceil(8)];
    for &bit in set {
        bytes[bit / 8] |= 0x80 >> (bit % 8);
    }
    let unused = (bytes.len() * 8 - count) as u8;
    element(CONTEXT, false, tag, &[vec![unused], bytes].concat())
}

/// An object identifier's contents: `1.2.840.10003.5.10`.
fn oid_bytes(arcs: &[u32]) -> Vec<u8> {
    let mut out = vec![(arcs[0] * 40 + arcs[1]) as u8];
    for &arc in &arcs[2..] {
        let mut groups = vec![(arc & 0x7f) as u8];
        let mut rest = arc >> 7;
        while rest > 0 {
            groups.push(0x80 | (rest & 0x7f) as u8);
            rest >>= 7;
        }
        out.extend(groups.iter().rev());
    }
    out
}

/// An element as read: its class, tag and contents.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Element {
    class: u8,
    tag: u32,
    constructed: bool,
    content: Vec<u8>,
}

/// The element at the start of `data`, and what follows it.
fn read(data: &[u8]) -> Option<(Element, &[u8])> {
    let (&first, mut rest) = data.split_first()?;
    let mut tag = u32::from(first & 0x1f);
    if tag == 0x1f {
        tag = 0;
        loop {
            let (&b, after) = rest.split_first()?;
            rest = after;
            tag = tag.checked_mul(128)? | u32::from(b & 0x7f);
            if b & 0x80 == 0 {
                break;
            }
        }
    }
    let (&length, after) = rest.split_first()?;
    rest = after;
    let class = first >> 6;
    let constructed = first & 0x20 != 0;
    if length == 0x80 {
        // An indefinite length: the contents run, past any element in
        // them, to the end-of-contents marker.
        if !constructed {
            return None;
        }
        let mut inner = rest;
        while !inner.starts_with(&[0, 0]) {
            inner = read(inner)?.1;
        }
        let content = &rest[..rest.len() - inner.len()];
        return Some((Element { class, tag, constructed, content: content.to_vec() }, &inner[2..]));
    }
    let len = if length & 0x80 == 0 {
        usize::from(length)
    } else {
        let count = usize::from(length & 0x7f);
        if count == 0 || count > 4 || rest.len() < count {
            return None;
        }
        let (bytes, after) = rest.split_at(count);
        rest = after;
        bytes.iter().fold(0, |len, &b| (len << 8) | usize::from(b))
    };
    if rest.len() < len {
        return None;
    }
    let (content, rest) = rest.split_at(len);
    Some((Element { class, tag, constructed, content: content.to_vec() }, rest))
}

impl Element {
    fn children(&self) -> Vec<Element> {
        let mut out = Vec::new();
        let mut rest = self.content.as_slice();
        while let Some((child, after)) = read(rest) {
            out.push(child);
            rest = after;
        }
        out
    }

    /// The first `[tag]` in it.
    fn child(&self, tag: u32) -> Option<Element> {
        self.children().into_iter().find(|c| c.class == CONTEXT && c.tag == tag)
    }

    fn integer(&self) -> i64 {
        let negative = self.content.first().is_some_and(|&b| b & 0x80 != 0);
        self.content.iter().take(8).fold(if negative { -1 } else { 0 }, |n, &b| (n << 8) | i64::from(b))
    }

    fn boolean(&self) -> bool {
        self.content.first().is_some_and(|&b| b != 0)
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.content).into_owned()
    }

    /// Every element in it, at any depth, that `wanted` picks.
    fn find_all(&self, wanted: &impl Fn(&Element) -> bool, out: &mut Vec<Element>) {
        for child in self.children() {
            if wanted(&child) {
                out.push(child);
            } else if child.constructed {
                child.find_all(wanted, out);
            }
        }
    }
}

// The messages.

const BIB1: [u32; 6] = [1, 2, 840, 10003, 3, 1];
const USMARC: [u32; 6] = [1, 2, 840, 10003, 5, 10];
const UNIMARC: [u32; 6] = [1, 2, 840, 10003, 5, 1];
const XML: [u32; 7] = [1, 2, 840, 10003, 5, 109, 10];

/// An InitializeRequest: versions 1 to 3, search and present, and who
/// asks (with a user and password when the catalogue wants them).
fn init_request(user: &str, password: &str) -> Vec<u8> {
    let mut parts =
        vec![bits(3, &[0, 1, 2], 3), bits(4, &[0, 1], 15), integer(5, 1 << 20), integer(6, 1 << 20)];
    if !user.is_empty() {
        let pass = sequence(&[string(1, user.as_bytes()), string(2, password.as_bytes())]);
        parts.push(tagged(7, &[pass]));
    }
    parts.push(string(110, b"Paperloom"));
    parts.push(string(111, b"Paperloom"));
    parts.push(string(112, env!("CARGO_PKG_VERSION").as_bytes()));
    tagged(20, &parts)
}

/// A type-1 query: each word looked for in the Bib-1 use attribute
/// `field`, all of them together.
fn rpn_query(words: &[Vec<u8>], field: i64) -> Vec<u8> {
    let operand = |word: &[u8]| {
        let attribute = sequence(&[integer(120, 1), integer(121, field)]);
        tagged(0, &[tagged(102, &[tagged(44, &[attribute]), string(45, word)])])
    };
    let mut tree = operand(words[0].as_slice());
    for word in &words[1..] {
        let and = tagged(46, &[string(0, &[])]);
        tree = tagged(1, &[tree, operand(word.as_slice()), and]);
    }
    tagged(1, &[element(UNIVERSAL, false, 6, &oid_bytes(&BIB1)), tree])
}

fn search_request(database: &str, query: Vec<u8>) -> Vec<u8> {
    tagged(
        22,
        &[
            integer(13, 0),
            integer(14, 1),
            integer(15, 0),
            boolean(16, true),
            string(17, b"default"),
            tagged(18, &[string(105, database.as_bytes())]),
            tagged(21, &[query]),
        ],
    )
}

fn present_request(start: i64, count: i64, syntax: &[u32]) -> Vec<u8> {
    tagged(
        24,
        &[
            string(31, b"default"),
            integer(30, start),
            integer(29, count),
            tagged(19, &[string(0, b"F")]),
            string(104, &oid_bytes(syntax)),
        ],
    )
}

/// What a Bib-1 diagnostic says, for those people meet.
fn diagnostic_text(condition: i64) -> &'static str {
    match condition {
        1 => "the catalogue failed",
        2 | 3 => "the catalogue is not taking searches now",
        13 | 100 => "records cannot be sent",
        108 | 123 => "the search is not one it understands",
        109 | 235 | 236 => "it has no database of that name",
        114 => "it does not search in that field",
        238 | 239 => "it does not send records in that form",
        1005 | 1007 => "it does not let this user in",
        _ => "it answered with an error",
    }
}

/// A diagnostic in an answer: its condition, said, and what came with it.
fn diagnostic(answer: &Element) -> Option<String> {
    let mut found = Vec::new();
    // DefaultDiagFormat: an OID, the condition and its added information.
    answer.find_all(
        &|e: &Element| {
            e.constructed && {
                let parts = e.children();
                parts.len() >= 2
                    && parts[0].class == UNIVERSAL
                    && parts[0].tag == 6
                    && parts[1].class == UNIVERSAL
                    && parts[1].tag == 2
            }
        },
        &mut found,
    );
    let parts = found.first()?.children();
    let condition = parts[1].integer();
    let added = parts.get(2).map(Element::text).filter(|t| !t.trim().is_empty());
    Some(match added {
        Some(added) => format!("{} ({condition}: {})", diagnostic_text(condition), added.trim()),
        None => format!("{} ({condition})", diagnostic_text(condition)),
    })
}

// The connection.

/// Where a catalogue is, and how to read it.
struct Address {
    host: String,
    port: u16,
    database: String,
    user: String,
    password: String,
    charset: Option<&'static Encoding>,
}

/// `%xx` escapes in part of an address, decoded.
fn unescape(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok());
        match (bytes[i], hex.and_then(|h| u8::from_str_radix(h, 16).ok())) {
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub(crate) fn is_z3950(url: &str) -> bool {
    let url = url.trim().to_ascii_lowercase();
    url.starts_with("z3950:") || url.starts_with("z39.50:") || url.starts_with("tcp:")
}

fn address(catalogue: &Catalogue) -> Option<Address> {
    let url = url::Url::parse(catalogue.url.trim()).ok()?;
    if !is_z3950(url.as_str()) {
        return None;
    }
    let charset = url
        .query_pairs()
        .find(|(k, _)| k == "charset" || k == "encoding")
        .and_then(|(_, v)| Encoding::for_label(v.trim().as_bytes()));
    Some(Address {
        host: url.host_str()?.to_string(),
        port: url.port().unwrap_or(210),
        database: Some(unescape(url.path().trim_start_matches('/'))).filter(|d| !d.is_empty())?,
        user: unescape(url.username()),
        password: url.password().map(unescape).unwrap_or_default(),
        charset,
    })
}

/// The Bib-1 use attribute a catalogue's index names.
fn use_attribute(index: &str) -> i64 {
    match index.trim().to_lowercase().as_str() {
        "" | "any" | "all" | "keyword" => 1016,
        "title" => 4,
        "author" | "name" => 1003,
        "subject" => 21,
        "isbn" => 7,
        "issn" => 8,
        number => number.parse().unwrap_or(1016),
    }
}

fn record_syntax(schema: &str) -> &'static [u32] {
    match schema.trim().to_lowercase().as_str() {
        "unimarc" | "cnmarc" => &UNIMARC,
        "xml" | "marcxml" => &XML,
        _ => &USMARC,
    }
}

fn invalid(what: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, what.to_string())
}

/// One element's bytes as they come, its parts too when its length is
/// indefinite (they then end with an end-of-contents marker).
fn receive(stream: &mut impl Read, depth: usize, budget: &mut usize) -> std::io::Result<Vec<u8>> {
    let mut byte = [0u8; 1];
    let mut next = |raw: &mut Vec<u8>| -> std::io::Result<u8> {
        stream.read_exact(&mut byte)?;
        raw.push(byte[0]);
        Ok(byte[0])
    };
    let mut raw = Vec::new();
    let first = next(&mut raw)?;
    if first & 0x1f == 0x1f {
        while next(&mut raw)? & 0x80 != 0 {}
    }
    let length = next(&mut raw)?;
    if length == 0x80 {
        if first & 0x20 == 0 || depth > 64 {
            return Err(invalid("an answer Paperloom cannot read"));
        }
        loop {
            let part = receive(stream, depth + 1, budget)?;
            let end = part == [0, 0];
            raw.extend(part);
            if end {
                return Ok(raw);
            }
        }
    }
    let len = if length & 0x80 == 0 {
        usize::from(length)
    } else {
        let count = usize::from(length & 0x7f);
        if count > 4 {
            return Err(invalid("an answer Paperloom cannot read"));
        }
        let mut bytes = vec![0u8; count];
        stream.read_exact(&mut bytes)?;
        raw.extend(&bytes);
        bytes.iter().fold(0, |len, &b| (len << 8) | usize::from(b))
    };
    *budget = budget.checked_sub(len).ok_or_else(|| invalid("an answer too large"))?;
    let start = raw.len();
    raw.resize(start + len, 0);
    stream.read_exact(&mut raw[start..])?;
    Ok(raw)
}

/// Sends a message and reads the answer, one BER element.
fn exchange(stream: &mut TcpStream, message: &[u8]) -> std::io::Result<Element> {
    stream.write_all(message)?;
    let raw = receive(stream, 0, &mut (64 << 20))?;
    read(&raw).map(|(element, _)| element).ok_or_else(|| invalid("an answer Paperloom cannot read"))
}

/// The records in a present answer: each with its syntax and bytes.
fn records(answer: &Element) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut externals = Vec::new();
    answer.find_all(&|e: &Element| e.class == UNIVERSAL && e.tag == 8 && e.constructed, &mut externals);
    externals
        .iter()
        .filter_map(|external| {
            let parts = external.children();
            let syntax = parts.iter().find(|p| p.class == UNIVERSAL && p.tag == 6).map(|p| p.content.clone());
            let data = parts.iter().find(|p| p.class == CONTEXT && p.tag == 1)?;
            // A segmented octet string comes as its pieces.
            let bytes = if data.constructed {
                data.children().into_iter().flat_map(|piece| piece.content).collect()
            } else {
                data.content.clone()
            };
            Some((syntax.unwrap_or_default(), bytes))
        })
        .collect()
}

/// A record as a search result, read by its syntax.
fn hit(catalogue: &Catalogue, syntax: &[u8], data: &[u8], charset: Option<&'static Encoding>) -> Option<Hit> {
    if syntax == oid_bytes(&XML).as_slice() || data.first() == Some(&b'<') {
        let text = String::from_utf8_lossy(data).replace("<marc:", "<").replace("</marc:", "</");
        let record = xml::elements(&text, "record").first().map(|r| Record::parse(r.body))?;
        return sru::hit(catalogue, &record);
    }
    let unimarc = syntax == oid_bytes(&UNIMARC).as_slice();
    let record = read_iso2709(data, charset, unimarc)?;
    if unimarc { unimarc_hit(&catalogue.id, &record) } else { sru::hit(catalogue, &record) }
}

pub(crate) fn search(catalogue: &Catalogue, query: &Query) -> Result<Page> {
    let failed = |detail: String| Error::Catalogue { name: catalogue.name.clone(), detail };
    let address = address(catalogue)
        .ok_or_else(|| failed("not a Z39.50 address (z3950://host:port/database)".into()))?;
    let words: Vec<Vec<u8>> = query
        .text
        .split_whitespace()
        .map(|word| match address.charset {
            Some(charset) => charset.encode(word).0.into_owned(),
            None => word.as_bytes().to_vec(),
        })
        .collect();
    if words.is_empty() {
        return Ok(Page::default());
    }
    let unreachable = |e: std::io::Error| failed(format!("could not be reached ({e})"));
    let target = (address.host.as_str(), address.port)
        .to_socket_addrs()
        .map_err(unreachable)?
        .next()
        .ok_or_else(|| failed("could not be found".into()))?;
    let mut stream = TcpStream::connect_timeout(&target, Duration::from_secs(15)).map_err(unreachable)?;
    stream.set_read_timeout(Some(Duration::from_secs(30))).map_err(unreachable)?;
    stream.set_write_timeout(Some(Duration::from_secs(15))).map_err(unreachable)?;

    let init = exchange(&mut stream, &init_request(&address.user, &address.password)).map_err(unreachable)?;
    if init.tag != 21 || !init.child(12).is_some_and(|r| r.boolean()) {
        return Err(failed("refused the connection".into()));
    }
    let words_query = rpn_query(&words, use_attribute(&catalogue.index));
    let searched =
        exchange(&mut stream, &search_request(&address.database, words_query)).map_err(unreachable)?;
    if searched.tag != 23 {
        return Err(failed("did not answer the search".into()));
    }
    if !searched.child(22).is_none_or(|status| status.boolean()) {
        return Err(failed(diagnostic(&searched).unwrap_or_else(|| "could not search".into())));
    }
    let total = searched.child(23).map_or(0, |count| count.integer().max(0));
    let start = i64::from(query.offset()) + 1;
    if total < start {
        return Ok(Page { hits: Vec::new(), total: Some(total as u64) });
    }
    let count = (total - start + 1).min(i64::from(query.per_page));
    let syntax = record_syntax(&catalogue.schema);
    let presented = exchange(&mut stream, &present_request(start, count, syntax)).map_err(unreachable)?;
    let found = records(&presented);
    if found.is_empty()
        && let Some(problem) = diagnostic(&presented)
    {
        return Err(failed(problem));
    }
    let hits =
        found.iter().filter_map(|(syntax, data)| hit(catalogue, syntax, data, address.charset)).collect();
    Ok(Page { hits, total: Some(total as u64) })
}

#[cfg(test)]
mod tests;
