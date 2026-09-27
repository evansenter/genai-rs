//! Live tests for the Voices resource (`/v1beta/voices`).
//!
//! ```bash
//! cargo test --test voices_tests -- --include-ignored --nocapture
//! ```

mod common;

use common::get_client;
use futures_util::{FutureExt, StreamExt, TryStreamExt};
use genai_rs::wire::{WireEvent, WireInspector};
use genai_rs::{Client, CreateVoiceRequest, GenaiError, Voice, VoicePitch, VoiceType};
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};

/// Creates a stored prompted voice, runs `body` with its ID, and deletes the
/// voice afterwards, including when `body` panics.
async fn with_prompted_voice<F, Fut>(client: &Client, body: F)
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let voice = client
        .voices()
        .create(
            &CreateVoiceRequest::prompted("A calm, low-pitched narrator.")
                .with_display_name("genai-rs-test"),
        )
        .await
        .expect("voices.create failed");
    let id = voice.id.clone().expect("stored voice has no id");
    assert_eq!(voice.voice_type, Some(VoiceType::Prompted));

    let outcome = AssertUnwindSafe(body(id.clone())).catch_unwind().await;

    let already_gone = matches!(
        client.voices().get(&id).await,
        Err(GenaiError::Api {
            status_code: 404,
            ..
        })
    );
    if !already_gone && let Err(e) = client.voices().delete(&id).await {
        eprintln!("cleanup failed for voice {id}: {e:?}");
    }
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_voice_list_filters_the_prebuilt_catalog() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let female_en_us = || {
        client
            .voices()
            .list()
            .with_page_size(3)
            .with_voice_type(VoiceType::Prebuilt)
            .with_language_code("en-US")
            .with_gender("female")
    };
    let page = female_en_us().send().await.expect("voices.list failed");
    assert!(!page.voices.is_empty(), "filtered catalog is empty");
    assert!(page.voices.len() <= 3);
    for voice in &page.voices {
        assert_eq!(voice.voice_type, Some(VoiceType::Prebuilt));
        assert_eq!(voice.gender.as_deref(), Some("female"));
        assert!(voice.id.is_some());
    }

    let next = page.next_page_token.expect("expected a second page");
    // The token is only valid with the filters that produced it.
    let second = female_en_us()
        .with_page_token(next)
        .send()
        .await
        .expect("second page failed");
    assert!(!second.voices.is_empty());
    assert_ne!(second.voices[0].id, page.voices[0].id);

    let low = client
        .voices()
        .list()
        .with_pitch(VoicePitch::Low)
        .with_page_size(5)
        .send()
        .await
        .expect("pitch-filtered list failed");
    assert!(low.voices.iter().all(|v| v.pitch == Some(VoicePitch::Low)));
}

/// Records the URL of every request the client sends.
#[derive(Debug, Default)]
struct RequestUrls(Mutex<Vec<String>>);

impl WireInspector for RequestUrls {
    fn on_event(&self, event: &WireEvent) {
        if let WireEvent::Request { method, url, .. } = event
            && method == "GET"
        {
            self.0.lock().unwrap().push(url.clone());
        }
    }
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_voice_list_items_follow_pages_with_the_filters() {
    let Some(api_key) = std::env::var("GEMINI_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
    else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };
    let urls = Arc::new(RequestUrls::default());
    let client = Client::builder(api_key)
        .add_wire_inspector(urls.clone())
        .build()
        .expect("client");

    // Five voices at two per page take three pages. The API rejects a page
    // token without the filters that produced it, so this only passes if
    // every page resends them.
    let voices: Vec<Voice> = client
        .voices()
        .list()
        .with_page_size(2)
        .with_voice_type(VoiceType::Prebuilt)
        .with_language_code("en-US")
        .items()
        .take(5)
        .try_collect()
        .await
        .expect("voices.list items");

    let ids: Vec<&str> = voices.iter().filter_map(|v| v.id.as_deref()).collect();
    println!("Streamed {ids:?}");
    assert_eq!(ids.len(), 5, "{voices:?}");
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), 5, "a voice repeated across pages: {ids:?}");
    for voice in &voices {
        assert_eq!(voice.voice_type, Some(VoiceType::Prebuilt), "{voice:?}");
        assert_eq!(voice.language_code.as_deref(), Some("en-US"), "{voice:?}");
    }

    let urls = urls.0.lock().unwrap().clone();
    let lists: Vec<&String> = urls.iter().filter(|u| u.contains("/voices?")).collect();
    println!("Sent {} list request(s)", lists.len());
    assert!(
        lists.len() >= 3,
        "five voices at page_size=2 need three pages, got {lists:?}"
    );
    for (i, url) in lists.iter().enumerate() {
        assert!(
            url.contains("page_size=2")
                && url.contains("type=prebuilt")
                && url.contains("language_code=en-US"),
            "page {i} dropped the query: {url}"
        );
        assert_eq!(url.contains("page_token="), i > 0, "page {i}: {url}");
    }
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_prompted_voice_lifecycle_and_synthesis() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    with_prompted_voice(&client, |id| {
        let client = client.clone();
        async move {
            let fetched = client.voices().get(&id).await.expect("voices.get failed");
            assert_eq!(fetched.id.as_deref(), Some(id.as_str()));
            assert_eq!(fetched.display_name.as_deref(), Some("genai-rs-test"));
            assert!(fetched.expire_time.is_some());

            let response = client
                .interaction()
                .with_model(genai_rs::DEFAULT_TTS_MODEL)
                .with_text("Testing a custom voice.")
                .with_audio_output()
                .with_voice(&id)
                .with_store_disabled()
                .create()
                .await
                .expect("synthesis with the custom voice failed");
            assert!(response.has_audio());

            client
                .voices()
                .delete(&id)
                .await
                .expect("voices.delete failed");
            let gone = client.voices().get(&id).await;
            assert!(
                matches!(
                    gone,
                    Err(GenaiError::Api {
                        status_code: 404,
                        ..
                    })
                ),
                "voice still readable after delete: {gone:?}"
            );
        }
    })
    .await;
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_prompted_voice_requires_store() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let err = client
        .voices()
        .create(&CreateVoiceRequest::prompted("A bright voice.").with_store(false))
        .await
        .expect_err("prompted voice with store=false was accepted");
    assert!(
        matches!(
            err,
            GenaiError::Api {
                status_code: 400,
                ..
            }
        ),
        "unexpected error: {err:?}"
    );
}
