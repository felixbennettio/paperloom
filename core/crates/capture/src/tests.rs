use paperloom_model::{Creator, ItemType};
use serde_json::json;

use super::*;

fn capture(value: serde_json::Value) -> Capture {
    serde_json::from_value(value).expect("a capture")
}

/// From Wanfang's search results (as the rules read them).
#[test]
fn reads_a_wanfang_result_list() {
    let page = capture(json!({
        "url": "https://s.wanfangdata.com.cn/paper?q=石墨烯",
        "title": "万方数据知识服务平台",
        "site": "wanfang",
        "page": "list",
        "records": [
            {
                "title": "吸附-沉淀自组装结合放电等离子烧结法制备石墨烯增强氧化铝复合陶瓷",
                "id": "periodical_wjclxb202604015",
                "kind": "期刊论文",
                "authors": ["程澳芃", "王跃文", "许文涛"],
                "source": ["2026年4期"],
                "container": "《无机材料学报》",
                "keywords": ["石墨烯", "氧化铝", "复合陶瓷"]
            },
            {
                "title": "石墨烯(graphene)声子结构及热学性质",
                "id": "thesis_Y1472157",
                "kind": "硕士论文",
                "authors": ["李远谋"],
                "institution": "湖南师范大学",
                "year": "2008"
            }
        ]
    }));
    let found = read(&page, "2026-10-12");
    assert_eq!(found.len(), 2);
    let a = &found[0];
    assert_eq!(a.key, "capture:wanfang:periodical_wjclxb202604015");
    assert_eq!(a.item.item_type, ItemType::JournalArticle);
    assert_eq!(a.item.creators[0], Creator::literal("程澳芃"));
    assert_eq!(a.item.container_title.as_deref(), Some("无机材料学报"));
    assert_eq!((a.item.year, a.item.issue.as_deref()), (Some(2026), Some("4")));
    assert_eq!(a.item.url.as_deref(), Some("https://d.wanfangdata.com.cn/periodical/wjclxb202604015"));
    assert_eq!(a.item.language.as_deref(), Some("zh"));
    let b = &found[1];
    assert_eq!(b.item.item_type, ItemType::Thesis);
    assert_eq!(b.item.publisher.as_deref(), Some("湖南师范大学"));
    assert_eq!(b.item.year, Some(2008));
}

/// From a Wanfang article page: its Open Graph tags and its details.
#[test]
fn reads_a_wanfang_article() {
    let page = capture(json!({
        "url": "https://d.wanfangdata.com.cn/periodical/wlxb202606030",
        "title": "石墨烯增强铝基纳米复合材料扭转载荷下的微观变形行为-期刊-万方数据知识服务平台",
        "site": "wanfang",
        "page": "detail",
        "meta": [["og:type", "article"], ["og:title", "石墨烯增强铝基纳米复合材料扭转载荷下的微观变形行为"]],
        "records": [{
            "title": "石墨烯增强铝基纳米复合材料扭转载荷下的微观变形行为",
            "authors": "李如峰,李多生,叶寅,余欣秀,岛洋洋,江五贵,秦庆华",
            "keywords": "石墨烯,铝基纳米复合材料,分子动力学,扭转模拟",
            "date": "2026-03-20 00:00:00",
            "url": "https://d.wanfangdata.com.cn/Periodical/wlxb202606030",
            "abstract": "采用分子动力学模拟,构建纯铝……",
            "container": "物理学报",
            "issn": ["1000-3290"],
            "source": "年,卷(期)：2026,75(6)",
            "doi": "10.7498/aps.75.20251374",
            "kind": "期刊",
            "pages": "387-402"
        }]
    }));
    let found = read(&page, "2026-10-12");
    assert_eq!(found.len(), 1);
    let item = &found[0].item;
    assert_eq!(found[0].key, "capture:wanfang:https://d.wanfangdata.com.cn/Periodical/wlxb202606030");
    assert_eq!(item.creators.len(), 7);
    assert_eq!(item.keywords.len(), 4);
    assert_eq!(item.date.as_deref(), Some("2026-03-20"));
    assert_eq!((item.volume.as_deref(), item.issue.as_deref()), (Some("75"), Some("6")));
    assert_eq!(item.pages.as_deref(), Some("387-402"));
    assert_eq!(item.issn.as_deref(), Some("1000-3290"));
    assert_eq!(item.doi.as_ref().map(|d| d.as_str()), Some("10.7498/aps.75.20251374"));
}

/// From a VIP article page, whose link carries a passing signature.
#[test]
fn reads_a_vip_article() {
    let page = capture(json!({
        "url": "https://www.cqvip.com/doc/journal/7202149251?sign=abc&expireTime=1&resourceId=7202149251&type=1",
        "site": "cqvip",
        "page": "detail",
        "records": [{
            "title": "石墨烯/铜钨触头电弧烧蚀的元胞自动机模拟方法与应用",
            "authors": ["李俊科1", "韩智云1", "芦宇峰2"],
            "abstract": "随着电力系统向高等级和大容量发展……",
            "source": "《中国电机工程学报》 • 北大核心CSCDCSTPCD • 2025年19期 7785-7797，I0051，共14页",
            "keywords": "断路器触头; 铜钨合金; 石墨烯",
            "doi": "10.13334/j.0258-8013.pcsee.241294"
        }]
    }));
    let item = &read(&page, "2026-10-12")[0].item;
    assert_eq!(
        item.creators,
        vec![Creator::literal("李俊科"), Creator::literal("韩智云"), Creator::literal("芦宇峰")]
    );
    assert_eq!(item.container_title.as_deref(), Some("中国电机工程学报"));
    assert_eq!((item.year, item.issue.as_deref()), (Some(2025), Some("19")));
    assert_eq!(item.pages.as_deref(), Some("7785-7797"));
    assert_eq!(item.keywords, ["断路器触头", "铜钨合金", "石墨烯"]);
    assert_eq!(item.url.as_deref(), Some("https://www.cqvip.com/doc/journal/7202149251"));
}

/// CNKI's result table: dates, database types and one author cell.
#[test]
fn reads_a_cnki_result_list() {
    let page = capture(json!({
        "url": "https://kns.cnki.net/kns8s/defaultresult/index",
        "site": "cnki",
        "page": "list",
        "records": [
            {
                "title": "基于深度学习的文献分类研究",
                "url": "https://kns.cnki.net/kcms2/article/abstract?v=abc",
                "id": "CMFD202301!1023001234.nh!1!0",
                "authors": ["张三;李四"],
                "container": "某某大学",
                "date": "2023-05-01",
                "kind": "硕士",
                "citedBy": "12"
            },
            { "url": "https://kns.cnki.net/kcms2/article/abstract?v=untitled" }
        ]
    }));
    let found = read(&page, "2026-10-12");
    assert_eq!(found.len(), 1, "a row without a title is no work");
    let thesis = &found[0];
    assert_eq!(thesis.item.item_type, ItemType::Thesis);
    assert_eq!(thesis.item.creators, vec![Creator::literal("张三"), Creator::literal("李四")]);
    assert_eq!((thesis.item.year, thesis.item.date.as_deref()), (Some(2023), Some("2023-05-01")));
    assert_eq!(thesis.cited_by, Some(12));
}

/// A publisher's article page with Highwire Press tags.
#[test]
fn reads_citation_tags() {
    let page = capture(json!({
        "url": "https://journal.example.org/article/123",
        "title": "Nanometre-scale thermometry | Nature",
        "meta": [
            ["citation_title", "Nanometre-scale thermometry in a living cell"],
            ["citation_author", "Kucsko, G."],
            ["citation_author", "Maurer, P. C."],
            ["citation_journal_title", "Nature"],
            ["citation_publication_date", "2013/07/31"],
            ["citation_volume", "500"],
            ["citation_firstpage", "54"],
            ["citation_lastpage", "58"],
            ["citation_doi", "10.1038/nature12373"],
            ["citation_pdf_url", "/articles/nature12373.pdf"],
            ["citation_keywords", "NV centres; thermometry"],
            ["og:title", "Not this one"]
        ]
    }));
    let found = read(&page, "2026-10-12");
    assert_eq!(found.len(), 1);
    let a = &found[0];
    assert_eq!(a.key, "capture:doi:10.1038/nature12373");
    assert_eq!(a.item.item_type, ItemType::JournalArticle);
    assert_eq!(a.item.title, "Nanometre-scale thermometry in a living cell");
    assert_eq!(a.item.creators[1], Creator::person("Maurer", "P. C."));
    assert_eq!(a.item.date.as_deref(), Some("2013-07-31"));
    assert_eq!(a.item.pages.as_deref(), Some("54-58"));
    assert_eq!(a.item.keywords, ["NV centres", "thermometry"]);
    assert_eq!(a.pdf_url.as_deref(), Some("https://journal.example.org/articles/nature12373.pdf"));
}

/// arXiv's abstract pages give the arXiv id but no DOI; the arXiv DOI lets the
/// work be recognised when the library already has it.
#[test]
fn gives_arxiv_pages_their_doi() {
    let page = capture(json!({
        "url": "https://arxiv.org/abs/1706.03762",
        "title": "[1706.03762] Attention Is All You Need",
        "meta": [
            ["citation_title", "Attention Is All You Need"],
            ["citation_author", "Vaswani, Ashish"],
            ["citation_date", "2017/06/12"],
            ["citation_pdf_url", "https://arxiv.org/pdf/1706.03762"],
            ["citation_arxiv_id", "1706.03762v7"]
        ]
    }));
    let found = read(&page, "2026-10-14");
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].item.doi.as_ref().map(|d| d.as_str().to_lowercase()).as_deref(),
        Some("10.48550/arxiv.1706.03762")
    );
    let with_doi = capture(json!({
        "url": "https://example.org/a",
        "meta": [["citation_title", "T"], ["citation_doi", "10.1000/x"], ["citation_arxiv_id", "2101.00001"]]
    }));
    assert_eq!(read(&with_doi, "2026-10-14")[0].item.doi.as_ref().map(|d| d.as_str()), Some("10.1000/x"));
}

#[test]
fn reads_coins_and_json_ld() {
    let book = "ctx_ver=Z39.88-2004&rft_val_fmt=info%3Aofi%2Ffmt%3Akev%3Amtx%3Abook&rft.genre=book\
                &rft.btitle=The+Art+of+Computer+Programming&rft.au=Knuth%2C+Donald&rft.date=1968\
                &rft.isbn=0201038013&rft.pub=Addison-Wesley";
    let ld = r#"{"@context":"https://schema.org","@graph":[
        {"@type":"WebSite","name":"A journal"},
        {"@type":"ScholarlyArticle","headline":"Attention Is All You Need",
         "author":[{"@type":"Person","givenName":"Ashish","familyName":"Vaswani"},{"name":"Noam Shazeer"}],
         "datePublished":"2017-06-12","sameAs":"https://doi.org/10.48550/arXiv.1706.03762",
         "isPartOf":{"@type":"PublicationIssue","issueNumber":"3",
           "isPartOf":{"@type":"PublicationVolume","volumeNumber":"30",
             "isPartOf":{"@type":"Periodical","name":"NeurIPS"}}},
         "keywords":"transformers, attention"}
    ]}"#;
    let page = capture(json!({
        "url": "https://catalog.example.org/search",
        "coins": [book],
        "jsonLd": [ld, "not json"]
    }));
    let found = read(&page, "2026-10-12");
    assert_eq!(found.len(), 2);
    let knuth = &found[0].item;
    assert_eq!(knuth.item_type, ItemType::Book);
    assert_eq!(knuth.creators, vec![Creator::person("Knuth", "Donald")]);
    assert_eq!((knuth.year, knuth.isbn.as_deref()), (Some(1968), Some("0201038013")));
    assert_eq!(found[0].key, "capture:https://catalog.example.org/search#coins0");
    let paper = &found[1].item;
    assert_eq!(paper.creators[0], Creator::person("Vaswani", "Ashish"));
    assert_eq!(paper.container_title.as_deref(), Some("NeurIPS"));
    assert_eq!((paper.volume.as_deref(), paper.issue.as_deref()), (Some("30"), Some("3")));
    assert!(paper.doi.is_some());
    assert_eq!(paper.keywords, ["transformers", "attention"]);
}

#[test]
fn any_other_page_is_a_web_page() {
    let page = capture(json!({
        "url": "https://blog.example.org/post",
        "title": "A post – Example blog",
        "lang": "en",
        "meta": [
            ["og:title", "A post"],
            ["og:site_name", "Example blog"],
            ["description", "What it says."],
            ["article:published_time", "2026-01-02T03:04:05Z"],
            ["author", "Ada Lovelace"]
        ]
    }));
    let found = read(&page, "2026-10-12");
    let item = &found[0].item;
    assert_eq!(item.item_type, ItemType::Webpage);
    assert_eq!(item.title, "A post");
    assert_eq!(item.container_title.as_deref(), Some("Example blog"));
    assert_eq!(item.date.as_deref(), Some("2026-01-02"));
    assert_eq!(item.accessed.as_deref(), Some("2026-10-12"));
    assert_eq!(item.creators, vec![Creator::person("Lovelace", "Ada")]);
    assert_eq!(item.url.as_deref(), Some("https://blog.example.org/post"));
    assert!(read(&Capture::default(), "2026-10-12").is_empty(), "a blank page holds nothing");
}

#[test]
fn reads_the_script_answer() {
    let json = r#"{"url":"https://x.org/","title":"X","lang":null,"site":null,"page":null,
        "meta":[["citation_title","T"]],"coins":[],"jsonLd":[],"records":[]}"#;
    let found = read_json(json, "2026-10-12").unwrap();
    assert_eq!(found[0].item.title, "T");
    assert!(read_json("{", "2026-10-12").is_err());
}

#[test]
fn ships_the_script_and_rules() {
    let script = script();
    assert!(script.contains("(function (rules)"));
    assert!(script.trim_end().ends_with("})"));
    let ids: Vec<String> = sites().into_iter().map(|s| s.id).collect();
    assert_eq!(ids, ["cnki", "wanfang", "cqvip"]);
}

#[test]
fn reads_dates_and_pages() {
    assert_eq!(parse_date("2021年5月20日"), (Some(2021), Some("2021-05-20".into())));
    assert_eq!(parse_date("2008"), (Some(2008), None));
    assert_eq!(parse_date("Spring 2019-13"), (Some(2019), None));
    assert_eq!(page_range("16 ( 387-402 )").as_deref(), Some("387-402"));
    assert_eq!(page_range("e1001").as_deref(), Some("e1001"));
    assert_eq!(page_range("共14页"), None);
    assert_eq!(civil_date(0), (1970, 1, 1));
    assert_eq!(civil_date(20_738), (2026, 10, 12));
    assert_eq!(civil_date(11_016), (2000, 2, 29));
}
