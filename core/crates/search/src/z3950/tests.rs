use std::net::TcpListener;

use super::*;

/// An ISO 2709 record of these fields (`$` starts a subfield).
fn iso2709(leader: &str, fields: &[(&str, &str)]) -> Vec<u8> {
    let (mut directory, mut data) = (Vec::new(), Vec::new());
    for (tag, body) in fields {
        let mut field: Vec<u8> = body.bytes().map(|b| if b == b'$' { 0x1f } else { b }).collect();
        field.push(0x1e);
        directory.extend(format!("{tag}{:04}{:05}", field.len(), data.len()).into_bytes());
        data.extend(field);
    }
    directory.push(0x1e);
    data.push(0x1d);
    let base = 24 + directory.len();
    let mut record =
        format!("{:05}{}{:05}{}", base + data.len(), &leader[5..12], base, &leader[17..24]).into_bytes();
    record.extend(directory);
    record.extend(data);
    record
}

fn book() -> Vec<u8> {
    iso2709(
        "00000cam a2200000 a 4500",
        &[
            ("001", "4768949"),
            ("008", "860303s1986    flua     b    001 0 eng  "),
            ("020", "  $a9780849358333"),
            ("100", "1 $aSmith, Jane A.,$eauthor."),
            ("245", "10$aMachine learning :$ban introduction /$cJane A. Smith."),
            ("264", " 1$aBoca Raton :$bCRC Press,$c1986."),
        ],
    )
}

/// A present answer as YAZ sends it: every length indefinite.
fn present_answer(record: &[u8]) -> Vec<u8> {
    let mut out = vec![0xb9, 0x80];
    out.extend(integer(24, 1));
    out.extend(integer(25, 2));
    out.extend(integer(27, 0));
    out.extend([0xbc, 0x80, 0x30, 0x80]);
    out.extend(string(0, b"LCDB"));
    out.extend([0xa1, 0x80, 0xa1, 0x80, 0x28, 0x80]);
    out.extend(element(UNIVERSAL, false, 6, &oid_bytes(&USMARC)));
    out.extend(string(1, record));
    out.extend([0; 12]);
    out
}

/// A catalogue at a port on this machine that answers each message with
/// the next answer.
fn serve(answers: Vec<Vec<u8>>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        for answer in answers {
            if receive(&mut stream, 0, &mut (1 << 20)).is_err() {
                return;
            }
            stream.write_all(&answer).unwrap();
        }
    });
    port
}

fn catalogue(url: String) -> Catalogue {
    Catalogue { id: "z".into(), name: "Test".into(), url, index: String::new(), schema: String::new() }
}

#[test]
fn encodes_and_reads_ber() {
    assert_eq!(integer_bytes(0), [0]);
    assert_eq!(integer_bytes(128), [0, 128]);
    assert_eq!(integer_bytes(-1), [0xff]);
    assert_eq!(integer_bytes(1 << 20), [0x10, 0, 0]);
    assert_eq!(oid_bytes(&USMARC), [0x2a, 0x86, 0x48, 0xce, 0x13, 0x05, 0x0a]);
    // Tags past 30 take more bytes; long contents a long length.
    assert_eq!(string(104, b"x"), [0x9f, 0x68, 0x01, b'x']);
    assert_eq!(&string(1, &[7; 300])[..4], [0x81, 0x82, 0x01, 0x2c]);
    assert_eq!(bits(4, &[0, 1], 15), [0x84, 0x03, 0x01, 0xc0, 0x00]);
    assert_eq!(init_request("", "")[0], 0xb4, "an InitializeRequest is [20]");

    let request = search_request("LCDB", rpn_query(&[b"deep".to_vec(), b"learning".to_vec()], 1016));
    let (read_back, rest) = read(&request).unwrap();
    assert!(rest.is_empty());
    assert_eq!((read_back.tag, read_back.child(18).unwrap().child(105).unwrap().text()), (22, "LCDB".into()));
    let query = read_back.child(21).unwrap().child(1).unwrap();
    let both = query.child(1).unwrap();
    assert_eq!(both.child(46).unwrap().child(0).unwrap().content, Vec::<u8>::new(), "the words are anded");

    let answer = present_answer(&book());
    let (element, rest) = read(&answer).unwrap();
    assert!(rest.is_empty() && element.tag == 25);
    let found = records(&element);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, oid_bytes(&USMARC));
    assert_eq!(found[0].1, book());
    let mut stream = answer.as_slice();
    assert_eq!(receive(&mut stream, 0, &mut (1 << 20)).unwrap(), answer, "read as it comes");
    assert!(receive(&mut present_answer(&book()).as_slice(), 0, &mut 100).is_err(), "too large");
}

#[test]
fn reads_addresses() {
    let at = address(&catalogue("z3950://reader:p%40ss@opac.example.edu:9991/UCS%2001?charset=gbk".into()))
        .unwrap();
    assert_eq!((at.host.as_str(), at.port, at.database.as_str()), ("opac.example.edu", 9991, "UCS 01"));
    assert_eq!((at.user.as_str(), at.password.as_str()), ("reader", "p@ss"));
    assert_eq!(at.charset, Some(encoding_rs::GBK));
    let plain = address(&catalogue("Z39.50://lx2.loc.gov/LCDB".into())).unwrap();
    assert_eq!((plain.port, plain.charset), (210, None));
    assert!(address(&catalogue("https://example.org/sru".into())).is_none());
    assert!(address(&catalogue("z3950://example.org:210/".into())).is_none(), "a database is needed");
    assert!(is_z3950(" z3950://x/y") && !is_z3950("http://x/y"));
    assert_eq!((use_attribute(""), use_attribute("Title"), use_attribute("1003")), (1016, 4, 1003));
    assert_eq!(record_syntax("CNMARC"), &UNIMARC);
}

#[test]
fn searches_a_catalogue() {
    let init = tagged(21, &[bits(3, &[0, 1, 2], 3), bits(4, &[0, 1], 15), boolean(12, true)]);
    let searched = tagged(23, &[integer(23, 2), integer(24, 0), integer(25, 1), boolean(22, true)]);
    let port = serve(vec![init.clone(), searched, present_answer(&book())]);
    let page = search(&catalogue(format!("z3950://127.0.0.1:{port}/LCDB")), &Query::new("machine learning"))
        .unwrap();
    assert_eq!(page.total, Some(2));
    assert_eq!(page.hits.len(), 1);
    let item = &page.hits[0].item;
    assert_eq!(page.hits[0].key, "z:4768949");
    assert_eq!(item.title, "Machine learning: an introduction");
    assert_eq!(item.creators[0].family, "Smith");
    assert_eq!((item.publisher.as_deref(), item.year), (Some("CRC Press"), Some(1986)));
    assert_eq!(item.isbn.as_deref(), Some("9780849358333"));

    // A database the catalogue does not have.
    let diagnostic = sequence(&[
        element(UNIVERSAL, false, 6, &oid_bytes(&[1, 2, 840, 10003, 4, 1])),
        element(UNIVERSAL, false, 2, &integer_bytes(235)),
        element(UNIVERSAL, false, 26, b"NOPE"),
    ]);
    let refused = tagged(
        23,
        &[
            integer(23, 0),
            integer(24, 0),
            integer(25, 0),
            boolean(22, false),
            element(CONTEXT, true, 130, &diagnostic.as_slice()[2..]),
        ],
    );
    let port = serve(vec![init, refused]);
    let error = search(&catalogue(format!("z3950://127.0.0.1:{port}/NOPE")), &Query::new("x")).unwrap_err();
    assert!(error.to_string().contains("no database of that name (235: NOPE)"), "{error}");
}
