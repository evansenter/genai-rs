//! Live tests for the Voices resource (`/v1beta/voices`).
//!
//! ```bash
//! cargo test --test voices_tests -- --include-ignored --nocapture
//! ```

mod common;

use common::get_client;
use futures_util::FutureExt;
use genai_rs::{Client, CreateVoiceRequest, GenaiError, ListVoicesParams, VoicePitch, VoiceType};
use std::panic::AssertUnwindSafe;

/// Creates a stored prompted voice, runs `body` with its ID, and deletes the
/// voice afterwards, including when `body` panics.
async fn with_prompted_voice<F, Fut>(client: &Client, body: F)
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let voice = client
        .create_voice(
            &CreateVoiceRequest::prompted("A calm, low-pitched narrator.")
                .with_display_name("genai-rs-test"),
        )
        .await
        .expect("create_voice failed");
    let id = voice.id.clone().expect("stored voice has no id");
    assert_eq!(voice.voice_type, Some(VoiceType::Prompted));

    let outcome = AssertUnwindSafe(body(id.clone())).catch_unwind().await;

    let already_gone = matches!(
        client.get_voice(&id).await,
        Err(GenaiError::Api {
            status_code: 404,
            ..
        })
    );
    if !already_gone && let Err(e) = client.delete_voice(&id).await {
        eprintln!("cleanup failed for voice {id}: {e:?}");
    }
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_list_voices_filters_the_prebuilt_catalog() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    let page = client
        .list_voices(
            &ListVoicesParams::new()
                .with_page_size(3)
                .with_voice_type(VoiceType::Prebuilt)
                .with_language_code("en-US")
                .with_gender("female"),
        )
        .await
        .expect("list_voices failed");
    assert!(!page.voices.is_empty(), "filtered catalog is empty");
    assert!(page.voices.len() <= 3);
    for voice in &page.voices {
        assert_eq!(voice.voice_type, Some(VoiceType::Prebuilt));
        assert_eq!(voice.gender.as_deref(), Some("female"));
        assert!(voice.id.is_some());
    }

    let next = page.next_page_token.expect("expected a second page");
    let second = client
        .list_voices(
            &ListVoicesParams::new()
                .with_page_size(3)
                .with_page_token(next)
                .with_voice_type(VoiceType::Prebuilt)
                .with_language_code("en-US")
                .with_gender("female"),
        )
        .await
        .expect("second page failed");
    assert!(!second.voices.is_empty());
    assert_ne!(second.voices[0].id, page.voices[0].id);

    let low = client
        .list_voices(
            &ListVoicesParams::new()
                .with_pitch(VoicePitch::Low)
                .with_page_size(5),
        )
        .await
        .expect("pitch-filtered list failed");
    assert!(low.voices.iter().all(|v| v.pitch == Some(VoicePitch::Low)));
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
            let fetched = client.get_voice(&id).await.expect("get_voice failed");
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

            client.delete_voice(&id).await.expect("delete_voice failed");
            let gone = client.get_voice(&id).await;
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
        .create_voice(&CreateVoiceRequest::prompted("A bright voice.").with_store(false))
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
