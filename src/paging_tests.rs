use super::*;
use std::sync::{Arc, Mutex};

/// A list page as the fake server returns it.
#[derive(Debug)]
struct FakePage {
    values: Vec<u32>,
    next_page_token: Option<String>,
}

impl_list_page!(FakePage, values: u32);

fn page(values: &[u32], next: Option<&str>) -> Result<FakePage, GenaiError> {
    Ok(FakePage {
        values: values.to_vec(),
        next_page_token: next.map(str::to_owned),
    })
}

fn failure() -> Result<FakePage, GenaiError> {
    Err(GenaiError::Internal("boom".to_string()))
}

/// A fake fetch that replays `replies` in order and records the token each
/// call received. A call past the end of the script panics, so a test also
/// fails if the engine fetches more than it should.
type Requested = Arc<Mutex<Vec<Option<String>>>>;

fn scripted(
    first_token: Option<&str>,
    replies: Vec<Result<FakePage, GenaiError>>,
) -> (BoxStream<'static, Result<FakePage, GenaiError>>, Requested) {
    let requested: Requested = Arc::default();
    let log = requested.clone();
    let mut replies = replies.into_iter();
    let stream = pages("fake", first_token.map(str::to_owned), move |token| {
        log.lock().unwrap().push(token);
        let reply = replies.next().expect("fetched past the end of the script");
        std::future::ready(reply)
    });
    (stream, requested)
}

/// Collects a stream into `Ok` pages' values and error messages, in order.
async fn outcomes(stream: BoxStream<'static, Result<FakePage, GenaiError>>) -> Vec<String> {
    stream
        .map(|r| match r {
            Ok(p) => format!("{:?}", p.values),
            Err(e) => format!("error: {e}"),
        })
        .collect()
        .await
}

fn tokens(requested: &Requested) -> Vec<Option<String>> {
    requested.lock().unwrap().clone()
}

#[tokio::test]
async fn a_missing_token_ends_the_stream() {
    let (stream, requested) = scripted(None, vec![page(&[1, 2], None)]);
    assert_eq!(outcomes(stream).await, ["[1, 2]"]);
    assert_eq!(tokens(&requested), [None]);
}

#[tokio::test]
async fn an_empty_token_ends_the_stream() {
    let (stream, requested) = scripted(None, vec![page(&[1], Some(""))]);
    assert_eq!(outcomes(stream).await, ["[1]"]);
    assert_eq!(tokens(&requested), [None]);
}

#[tokio::test]
async fn each_next_token_is_sent_with_the_following_request() {
    let (stream, requested) = scripted(
        None,
        vec![
            page(&[1], Some("t2")),
            page(&[2], Some("t3")),
            page(&[3], None),
        ],
    );
    assert_eq!(outcomes(stream).await, ["[1]", "[2]", "[3]"]);
    assert_eq!(
        tokens(&requested),
        [None, Some("t2".into()), Some("t3".into())]
    );
}

#[tokio::test]
async fn the_first_token_starts_the_stream() {
    let (stream, requested) = scripted(Some("resume"), vec![page(&[7], None)]);
    assert_eq!(outcomes(stream).await, ["[7]"]);
    assert_eq!(tokens(&requested), [Some("resume".into())]);
}

#[tokio::test]
async fn an_empty_page_with_a_token_is_followed() {
    let (stream, requested) = scripted(None, vec![page(&[], Some("t2")), page(&[5], None)]);
    assert_eq!(outcomes(stream).await, ["[]", "[5]"]);
    assert_eq!(tokens(&requested), [None, Some("t2".into())]);
}

#[tokio::test]
async fn a_repeated_token_yields_its_page_then_malformed_response() {
    let (stream, requested) = scripted(
        None,
        vec![
            page(&[1], Some("a")),
            page(&[2], Some("b")),
            page(&[3], Some("a")),
        ],
    );
    let results: Vec<_> = stream.collect().await;
    assert_eq!(results.len(), 4, "three pages, then the error: {results:?}");
    assert_eq!(results[2].as_ref().unwrap().values, [3]);
    match &results[3] {
        Err(GenaiError::MalformedResponse(message)) => {
            assert!(message.contains("fake"), "names the list: {message}");
            assert!(message.contains("\"a\""), "names the token: {message}");
        }
        other => panic!("expected MalformedResponse, got {other:?}"),
    }
    assert_eq!(
        tokens(&requested),
        [None, Some("a".into()), Some("b".into())],
        "the repeated token is not requested again"
    );
}

#[tokio::test]
async fn a_repeated_starting_token_is_detected() {
    let (stream, requested) = scripted(Some("start"), vec![page(&[1], Some("start"))]);
    let results: Vec<_> = stream.collect().await;
    assert_eq!(results.len(), 2, "{results:?}");
    assert!(results[0].is_ok());
    assert!(
        matches!(results[1], Err(GenaiError::MalformedResponse(_))),
        "{:?}",
        results[1]
    );
    assert_eq!(tokens(&requested), [Some("start".into())]);
}

#[tokio::test]
async fn a_token_returned_by_its_own_page_is_a_repeat() {
    let (stream, _) = scripted(None, vec![page(&[1], Some("t")), page(&[2], Some("t"))]);
    assert_eq!(
        outcomes(stream).await,
        [
            "[1]",
            "[2]",
            "error: Malformed API response: fake: server returned page token \"t\" twice"
        ]
    );
}

#[tokio::test]
async fn an_error_is_yielded_once_and_ends_the_stream() {
    // A third fetch would panic on the exhausted script.
    let (stream, requested) = scripted(None, vec![page(&[1], Some("t2")), failure()]);
    assert_eq!(
        outcomes(stream).await,
        ["[1]", "error: Internal client error: boom"]
    );
    assert_eq!(tokens(&requested), [None, Some("t2".into())]);
}

#[tokio::test]
async fn nothing_is_fetched_until_the_first_poll() {
    let (mut stream, requested) = scripted(None, vec![page(&[1], Some("t2")), page(&[2], None)]);
    assert!(tokens(&requested).is_empty(), "fetched before any poll");

    assert!(stream.next().await.is_some());
    assert_eq!(tokens(&requested), [None], "one page per poll");
    assert!(stream.next().await.is_some());
    assert!(stream.next().await.is_none());
    assert_eq!(tokens(&requested).len(), 2);
}

#[tokio::test]
async fn items_keep_server_order_across_pages() {
    let (stream, _) = scripted(
        None,
        vec![
            page(&[1, 2], Some("t2")),
            page(&[], Some("t3")),
            page(&[3], Some("t4")),
            page(&[4, 5], None),
        ],
    );
    let values: Vec<u32> = items(stream).try_collect().await.unwrap();
    assert_eq!(values, [1, 2, 3, 4, 5]);
}

#[tokio::test]
async fn items_end_with_the_error_after_earlier_pages_items() {
    let (stream, _) = scripted(None, vec![page(&[1, 2], Some("t2")), failure()]);
    let results: Vec<_> = items(stream).collect().await;
    assert_eq!(results.len(), 3, "{results:?}");
    assert_eq!(results[0].as_ref().unwrap(), &1);
    assert_eq!(results[1].as_ref().unwrap(), &2);
    assert!(matches!(results[2], Err(GenaiError::Internal(_))));
}

#[tokio::test]
async fn items_of_an_empty_list_is_empty() {
    let (stream, requested) = scripted(None, vec![page(&[], None)]);
    let values: Vec<u32> = items(stream).try_collect().await.unwrap();
    assert!(values.is_empty());
    assert_eq!(tokens(&requested), [None]);
}
