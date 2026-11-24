use std::sync::Mutex;

use paperloom_http::{NetworkError, Response};

use super::*;

/// Answers each request with the next canned reply and keeps the requests.
struct Canned {
    replies: Mutex<Vec<(u16, Value)>>,
    seen: Mutex<Vec<Request>>,
}

impl Canned {
    fn new(replies: Vec<(u16, Value)>) -> Self {
        Self { replies: Mutex::new(replies), seen: Mutex::new(Vec::new()) }
    }

    fn sent(&self, n: usize) -> (Request, Value) {
        let request = self.seen.lock().unwrap()[n].clone();
        let body = request.body.as_deref().map(|b| serde_json::from_slice(b).unwrap()).unwrap_or(Value::Null);
        (request, body)
    }
}

impl Http for Canned {
    fn get(&self, _url: &str, _accept: &str) -> Result<Response, NetworkError> {
        Err(NetworkError("not used".into()))
    }

    fn post_form(&self, _url: &str, _form: &[(&str, &str)]) -> Result<Response, NetworkError> {
        Err(NetworkError("not used".into()))
    }

    fn send(&self, request: Request) -> Result<BytesResponse, NetworkError> {
        self.seen.lock().unwrap().push(request);
        let (status, body) = self.replies.lock().unwrap().remove(0);
        Ok(BytesResponse { status, body: body.to_string().into_bytes() })
    }
}

fn header<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

fn anthropic() -> Provider {
    Provider {
        protocol: Protocol::Anthropic,
        base_url: "https://api.anthropic.com/".into(),
        api_key: "key".into(),
        model: "a-model".into(),
    }
}

fn prompt() -> Prompt {
    Prompt { system: "Be brief.".into(), user: "Summarise.".into(), max_tokens: 4000 }
}

#[test]
fn asks_anthropic_and_reads_the_text_blocks() {
    let http = Canned::new(vec![(
        200,
        json!({ "content": [
            { "type": "thinking", "thinking": "" },
            { "type": "text", "text": "Short " },
            { "type": "text", "text": "answer." }
        ], "stop_reason": "end_turn" }),
    )]);
    let answer = complete(&http, &anthropic(), &prompt()).unwrap();
    assert_eq!(answer, Answer { text: "Short answer.".into(), cut_short: false });
    let (request, body) = http.sent(0);
    assert_eq!(request.url, "https://api.anthropic.com/v1/messages");
    assert_eq!(header(&request, "x-api-key"), Some("key"));
    assert_eq!(header(&request, "anthropic-version"), Some("2023-06-01"));
    assert_eq!(body["model"], "a-model");
    assert_eq!(body["max_tokens"], 4000);
    assert_eq!(body["system"], "Be brief.");
    assert_eq!(body["messages"][0], json!({ "role": "user", "content": "Summarise." }));
}

#[test]
fn reports_refusals_cut_short_answers_and_errors() {
    let http = Canned::new(vec![
        (
            200,
            json!({ "content": [], "stop_reason": "refusal", "stop_details": { "type": "refusal", "category": "bio", "explanation": null } }),
        ),
        (200, json!({ "content": [{ "type": "text", "text": "Part" }], "stop_reason": "max_tokens" })),
        (
            401,
            json!({ "type": "error", "error": { "type": "authentication_error", "message": "invalid x-api-key" } }),
        ),
    ]);
    assert_eq!(complete(&http, &anthropic(), &prompt()), Err(AiError::Refused("bio".into())));
    assert!(complete(&http, &anthropic(), &prompt()).unwrap().cut_short);
    assert_eq!(
        complete(&http, &anthropic(), &prompt()),
        Err(AiError::Provider { status: 401, message: "invalid x-api-key".into() })
    );
    let mut unset = anthropic();
    unset.model = " ".into();
    assert!(matches!(complete(&http, &unset, &prompt()), Err(AiError::Setup(_))));
    unset.model = "m".into();
    unset.api_key = String::new();
    assert!(matches!(complete(&http, &unset, &prompt()), Err(AiError::Setup(_))));
}

#[test]
fn asks_an_openai_style_api() {
    let local = Provider {
        protocol: Protocol::OpenAi,
        base_url: "http://localhost:11434/v1".into(),
        api_key: String::new(),
        model: "local".into(),
    };
    let http = Canned::new(vec![
        (
            200,
            json!({ "choices": [{ "message": { "role": "assistant", "content": "你好" }, "finish_reason": "stop" }] }),
        ),
        (200, json!({ "data": [{ "id": "zeta" }, { "id": "alpha" }] })),
    ]);
    assert_eq!(complete(&http, &local, &prompt()).unwrap().text, "你好");
    let (request, body) = http.sent(0);
    assert_eq!(request.url, "http://localhost:11434/v1/chat/completions");
    assert_eq!(header(&request, "Authorization"), None, "no key, no header");
    assert_eq!(body["messages"][0], json!({ "role": "system", "content": "Be brief." }));
    assert!(body.get("max_tokens").is_none());
    let names: Vec<String> = models(&http, &local).unwrap().into_iter().map(|m| m.id).collect();
    assert_eq!(names, ["alpha", "zeta"]);
}

#[test]
fn lists_anthropic_models_page_by_page() {
    let http = Canned::new(vec![
        (
            200,
            json!({ "data": [{ "id": "m1", "display_name": "Model One" }], "has_more": true, "last_id": "m1" }),
        ),
        (
            200,
            json!({ "data": [{ "id": "m2", "display_name": "Model Two" }], "has_more": false, "last_id": "m2" }),
        ),
    ]);
    let found = models(&http, &anthropic()).unwrap();
    assert_eq!(found.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["Model One", "Model Two"]);
    assert_eq!(http.sent(1).0.url, "https://api.anthropic.com/v1/models?limit=100&after_id=m1");
    assert_eq!(Protocol::parse("openai"), Some(Protocol::OpenAi));
}

#[test]
fn embeds_inputs_in_order_through_the_openai_api() {
    let provider =
        Provider { protocol: Protocol::OpenAi, base_url: "https://example.org/v1/".into(), ..anthropic() };
    let http = Canned::new(vec![(
        200,
        json!({ "data": [
        { "index": 1, "embedding": [0.0, 2.0] },
        { "index": 0, "embedding": [3.0, 0.0] }
    ] }),
    )]);
    let inputs = vec!["Nanodiamond thermometry".into(), "Unrelated passage".into()];
    assert_eq!(embed(&http, &provider, " vectors ", &inputs).unwrap(), [vec![3.0, 0.0], vec![0.0, 2.0]]);
    let (request, body) = http.sent(0);
    assert_eq!(request.method, "POST");
    assert_eq!(request.url, "https://example.org/v1/embeddings");
    assert_eq!(header(&request, "Authorization"), Some("Bearer key"));
    assert_eq!(body, json!({ "model": "vectors", "input": inputs }));
    assert!(embed(&http, &provider, "vectors", &[]).unwrap().is_empty());
    assert!(matches!(embed(&http, &anthropic(), "vectors", &inputs), Err(AiError::Setup(_))));
    assert!(matches!(embed(&http, &provider, " ", &inputs), Err(AiError::Setup(_))));
    assert_eq!(http.seen.lock().unwrap().len(), 1);
}

#[test]
fn rejects_incomplete_or_malformed_embeddings() {
    let provider = Provider { protocol: Protocol::OpenAi, ..anthropic() };
    let inputs = vec!["first".into(), "second".into()];
    for data in [
        json!(null),
        json!([]),
        json!([{ "index": 0, "embedding": [1.0] }]),
        json!([{ "index": 0, "embedding": [1.0] }, { "index": 0, "embedding": [1.0] }]),
        json!([{ "index": 0, "embedding": [1.0] }, { "index": 2, "embedding": [1.0] }]),
        json!([{ "index": 0, "embedding": [1.0] }, { "embedding": [1.0] }]),
        json!([{ "index": 0, "embedding": [] }, { "index": 1, "embedding": [] }]),
        json!([{ "index": 0, "embedding": [1.0] }, { "index": 1, "embedding": [1.0, 0.0] }]),
        json!([{ "index": 0, "embedding": [1.0] }, { "index": 1, "embedding": ["bad"] }]),
        json!([{ "index": 0, "embedding": [1.0] }, { "index": 1, "embedding": [1e100] }]),
    ] {
        let http = Canned::new(vec![(200, json!({ "data": data }))]);
        assert!(embed(&http, &provider, "vectors", &inputs).is_err());
    }
    let http = Canned::new(vec![(429, json!({ "error": { "message": "try later" } }))]);
    assert!(matches!(
        embed(&http, &provider, "vectors", &inputs),
        Err(AiError::Provider { status: 429, .. })
    ));
}
