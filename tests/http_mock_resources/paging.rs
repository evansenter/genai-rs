//! List paging: `.pages()` / `.items()` over several stub pages (D-016).

use super::files::file_names;
use super::*;

fn agents_page(ids: &[&str], next: Option<&str>) -> Reply {
    let agents: Vec<Value> = ids.iter().map(|id| json!({"id": id})).collect();
    let mut body = json!({"agents": agents});
    if let Some(next) = next {
        body["next_page_token"] = json!(next);
    }
    Reply::json(200, body)
}

fn agent_ids(agents: &[Agent]) -> Vec<&str> {
    agents.iter().filter_map(|a| a.id.as_deref()).collect()
}

#[tokio::test]
async fn list_items_follow_every_page_and_resend_the_query() {
    let stub = Stub::replying(vec![
        agents_page(&["a1", "a2"], Some("p2")),
        // An empty page with a token is followed, not taken as the end.
        agents_page(&[], Some("p/3")),
        agents_page(&["a3"], Some("")),
    ])
    .await;
    let client = stub.client();

    let agents: Vec<Agent> = client
        .agents()
        .list()
        .with_page_size(2)
        .with_parent("projects/p")
        .items()
        .try_collect()
        .await
        .unwrap();

    assert_eq!(agent_ids(&agents), ["a1", "a2", "a3"]);
    // Page size and filter ride on every page; only the token changes.
    assert_eq!(
        targets(&stub),
        [
            "/v1beta/agents?page_size=2&parent=projects%2Fp",
            "/v1beta/agents?page_size=2&page_token=p2&parent=projects%2Fp",
            "/v1beta/agents?page_size=2&page_token=p%2F3&parent=projects%2Fp",
        ]
    );
}

#[tokio::test]
async fn list_pages_resume_from_with_page_token() {
    let stub = Stub::replying(vec![
        agents_page(&["a3"], Some("p3")),
        agents_page(&["a4"], None),
    ])
    .await;
    let client = stub.client();

    let pages: Vec<_> = client
        .agents()
        .list()
        .with_page_token("p2")
        .pages()
        .try_collect()
        .await
        .unwrap();

    assert_eq!(pages.len(), 2);
    assert_eq!(agent_ids(&pages[0].agents), ["a3"]);
    assert_eq!(pages[1].next_page_token, None);
    assert_eq!(
        targets(&stub),
        [
            "/v1beta/agents?page_token=p2",
            "/v1beta/agents?page_token=p3"
        ]
    );
}

#[tokio::test]
async fn list_send_fetches_exactly_one_page() {
    let stub = Stub::replying(vec![agents_page(&["a1"], Some("p2"))]).await;
    let client = stub.client();

    let page = client.agents().list().send().await.unwrap();

    assert_eq!(agent_ids(&page.agents), ["a1"]);
    assert_eq!(page.next_page_token.as_deref(), Some("p2"));
    assert_eq!(targets(&stub), ["/v1beta/agents"]);
}

#[tokio::test]
async fn list_items_stop_on_a_repeated_page_token() {
    let stub = Stub::replying(vec![
        agents_page(&["a1"], Some("same")),
        agents_page(&["a2"], Some("same")),
    ])
    .await;
    let client = stub.client();

    let results: Vec<_> = client.agents().list().items().collect().await;

    assert_eq!(results.len(), 3, "{results:?}");
    assert_eq!(results[0].as_ref().unwrap().id.as_deref(), Some("a1"));
    // The repeating page's items are still delivered, then the error.
    assert_eq!(results[1].as_ref().unwrap().id.as_deref(), Some("a2"));
    assert!(
        matches!(&results[2], Err(GenaiError::MalformedResponse(m)) if m.contains("\"same\"")),
        "{:?}",
        results[2]
    );
    assert_eq!(
        stub.requests().len(),
        2,
        "the repeated token is not fetched"
    );
}

#[tokio::test]
async fn list_items_end_at_the_first_error() {
    // The stub answers a third request with 500, which would show up here.
    let stub = Stub::replying(vec![agents_page(&["a1"], Some("p2")), not_found()]).await;
    let client = stub.client();

    let results: Vec<_> = client.agents().list().items().collect().await;

    assert_eq!(results.len(), 2, "{results:?}");
    assert!(results[0].is_ok());
    assert!(
        matches!(
            results[1],
            Err(GenaiError::Api {
                status_code: 404,
                ..
            })
        ),
        "{:?}",
        results[1]
    );
    assert_eq!(stub.requests().len(), 2);
}

#[tokio::test]
async fn list_streams_send_nothing_until_polled() {
    let stub = Stub::replying(vec![agents_page(&["a1"], None)]).await;
    let client = stub.client();

    let mut items = client.agents().list().items();
    let pages = client.agents().list().pages();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        stub.requests().is_empty(),
        "a request before the first poll"
    );

    drop(pages);
    let first = items.next().await.unwrap().unwrap();
    assert_eq!(first.id.as_deref(), Some("a1"));
    assert!(items.next().await.is_none());
    assert_eq!(stub.requests().len(), 1);
}

/// Compile guard for D-016. Handle methods take `self`, so a call's future
/// holds the client borrow rather than the temporary handle (an `&self`
/// receiver fails these with E0515), and list streams own a client clone,
/// so one built from a temporary client can be spawned.
#[tokio::test]
async fn handle_futures_and_list_streams_outlive_their_temporaries() {
    fn assert_send<T: Send>(_: &T) {}

    let stub = Stub::start(|request, _| {
        if request.target.starts_with("/v1beta/agents?") || request.target == "/v1beta/agents" {
            agents_page(&["listed"], None)
        } else if let Some(id) = request.target.strip_prefix("/v1beta/files/") {
            Reply::json(
                200,
                json!({"name": format!("files/{id}"), "mimeType": "text/plain"}),
            )
        } else {
            let id = request.target.rsplit('/').next().unwrap_or_default();
            Reply::json(200, json!({"id": id}))
        }
    })
    .await;
    let client = stub.client();
    let c = &client;

    let ids = ["a1".to_string(), "a2".to_string()];
    let fetched = futures_util::future::join_all(ids.iter().map(|id| c.agents().get(id))).await;
    let fetched: Vec<Agent> = fetched.into_iter().collect::<Result<_, _>>().unwrap();
    assert_eq!(agent_ids(&fetched), ["a1", "a2"]);

    let stored = c.agents().get("a3");
    assert_send(&stored);
    assert_eq!(stored.await.unwrap().id.as_deref(), Some("a3"));

    let names = ["files/f1".to_string(), "files/f2".to_string()];
    let files = futures_util::future::join_all(names.iter().map(|n| c.files().get(n))).await;
    let files: Vec<FileMetadata> = files.into_iter().collect::<Result<_, _>>().unwrap();
    assert_eq!(file_names(&files), ["files/f1", "files/f2"]);
    let waiting = c.files().wait_until_active("files/f3", PollOptions::new());
    assert_send(&waiting);
    // A nested handle's future holds only the client borrow too.
    let indexing = c
        .file_search_stores()
        .documents()
        .wait_until_active(DOC, PollOptions::new());
    assert_send(&indexing);
    let documents = c.file_search_stores().documents().list(STORE).items();
    assert_send(&documents);

    // The client this stream came from is a temporary, gone before the
    // stream is first polled on another task.
    let items = stub.client().agents().list().items();
    assert_send(&items);
    let listed: Vec<Agent> = tokio::spawn(items.try_collect()).await.unwrap().unwrap();
    assert_eq!(agent_ids(&listed), ["listed"]);

    let pages = c.agents().list().with_page_size(1).pages();
    let spawned = tokio::spawn(async move { pages.try_collect::<Vec<_>>().await });
    assert_eq!(spawned.await.unwrap().unwrap().len(), 1);
}
