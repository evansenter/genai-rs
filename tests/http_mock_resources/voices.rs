//! Voices: every endpoint's request, the prebuilt and prompted shapes, and
//! list paging with the filters.

use super::*;

#[tokio::test]
async fn voice_endpoints_send_the_documented_requests() {
    let stub = ok_stub().await;
    let client = stub.client();
    let c = &client;
    assert_wire(
        &stub,
        vec![
            wire("GET", "/v1beta/voices", None, c.voices().list().send()),
            wire(
                "GET",
                "/v1beta/voices?page_size=5&page_token=t%2F1&search=warm%20voice&gender=female\
                 &language_code=en-US&type=prebuilt&pitch=high",
                None,
                c.voices()
                    .list()
                    .with_page_size(5)
                    .with_page_token("t/1")
                    .with_search("warm voice")
                    .with_voice_type(VoiceType::Prebuilt)
                    .with_gender("female")
                    .with_language_code("en-US")
                    .with_pitch(VoicePitch::High)
                    .send(),
            ),
            wire(
                "GET",
                "/v1beta/voices?region_code=US&accent=General%20American&persona=Narrator\
                 &context=Content%20%26%20Media",
                None,
                c.voices()
                    .list()
                    .with_region_code("US")
                    .with_accent("General American")
                    .with_persona("Narrator")
                    .with_context("Content & Media")
                    .send(),
            ),
            wire(
                "GET",
                "/v1beta/voices/voice_abc",
                None,
                c.voices().get("voice_abc"),
            ),
            wire(
                "POST",
                "/v1beta/voices",
                Some(json!({
                    "voice": {"type": "prompted", "prompted": {"input": "A warm storyteller."}, "display_name": "teller"},
                    "store": true
                })),
                async move {
                    let request =
                        CreateVoiceRequest::prompted("A warm storyteller.").with_display_name("teller");
                    c.voices().create(&request).await
                },
            ),
            wire(
                "POST",
                "/v1beta/voices",
                Some(json!({
                    "voice": {"type": "replicated", "replicated": {
                        "source_audio": {"data": "c3Jj", "mime_type": "audio/wav"},
                        "consent_audio": {"data": "Y25z", "mime_type": "audio/wav"}
                    }},
                    "store": false
                })),
                async move {
                    let request = CreateVoiceRequest::replicated(
                        VoiceAudio::new("c3Jj", "audio/wav"),
                        VoiceAudio::new("Y25z", "audio/wav"),
                    )
                    .with_store(false);
                    c.voices().create(&request).await
                },
            ),
            wire(
                "DELETE",
                "/v1beta/voices/voice_abc",
                None,
                c.voices().delete("voice_abc"),
            ),
        ],
    )
    .await;
}

#[cfg(not(genai_strict_unknown))]
#[tokio::test]
async fn voice_list_parses_prebuilt_voices_and_preserves_unknowns() {
    let stub = Stub::replying(vec![Reply::json(
        200,
        json!({
            "voices": [
                {"id": "achernar", "type": "prebuilt", "display_name": "Achernar", "language_code": "en-US",
                 "region_code": "US", "accent": "General American", "persona": "Storyteller & Narrator",
                 "context": "Content & Media", "gender": "female", "pitch": "high",
                 "description": "Soft, calm, and soothing voice with a higher pitch."},
                {"id": "voice_x", "type": "cloned", "pitch": "very_high", "quality": "hd"}
            ],
            "next_page_token": "p2"
        }),
    )])
    .await;

    let list = stub.client().voices().list().send().await.unwrap();

    let prebuilt = &list.voices[0];
    assert_eq!(prebuilt.voice_type, Some(VoiceType::Prebuilt));
    assert_eq!(prebuilt.pitch, Some(VoicePitch::High));
    assert_eq!(prebuilt.persona.as_deref(), Some("Storyteller & Narrator"));
    assert!(prebuilt.extra.is_empty());
    let custom = &list.voices[1];
    assert_eq!(
        custom.voice_type.as_ref().unwrap().unknown_voice_type(),
        Some("cloned")
    );
    assert_eq!(
        custom.pitch.as_ref().unwrap().unknown_pitch_type(),
        Some("very_high")
    );
    assert_eq!(custom.extra["quality"], "hd");
    let back = serde_json::to_value(custom).unwrap();
    assert_eq!(back["type"], "cloned");
    assert_eq!(list.next_page_token.as_deref(), Some("p2"));
}

#[tokio::test]
async fn created_voice_parses_the_prompted_shape() {
    let stub = Stub::replying(vec![Reply::json(
        200,
        json!({
            "id": "voice_3d8wi4xx4yxg",
            "model": "models/tts-model",
            "type": "prompted",
            "expire_time": "2027-09-24T01:44:20.854771486Z",
            "display_name": "robot",
            "prompted": {"input": "A calm, low-pitched robot narrator."},
            "usage": {"total_tokens": 1334},
            "sample_audio": {"data": "UklGRg==", "mime_type": "audio/wav"}
        }),
    )])
    .await;

    let voice = stub
        .client()
        .voices()
        .create(&CreateVoiceRequest::prompted(
            "A calm, low-pitched robot narrator.",
        ))
        .await
        .unwrap();

    assert_eq!(voice.id.as_deref(), Some("voice_3d8wi4xx4yxg"));
    assert_eq!(voice.voice_type, Some(VoiceType::Prompted));
    assert!(voice.expire_time.is_some());
    assert_eq!(voice.usage.unwrap().total_tokens, Some(1334));
    assert_eq!(voice.sample_audio.unwrap().data, "UklGRg==");
}

fn voices_page(ids: &[&str], next: Option<&str>) -> Reply {
    let voices: Vec<Value> = ids
        .iter()
        .map(|id| json!({"id": id, "type": "prebuilt", "gender": "female"}))
        .collect();
    let mut body = json!({"voices": voices});
    if let Some(next) = next {
        body["next_page_token"] = json!(next);
    }
    Reply::json(200, body)
}

/// The live API rejects a page token sent without the filters of the
/// request that returned it (400, 2026-09-27), so every page repeats them.
/// The token is the live one for this query at `page_size=3`.
#[tokio::test]
async fn voice_list_items_resend_the_filters_and_page_size_on_every_page() {
    const TOKEN: &str = "ETuWXCXs7pvKGAEiDWF1dG9ub2V8ZW4tVVM";
    let stub = Stub::replying(vec![
        voices_page(&["achernar", "aoede", "autonoe"], Some(TOKEN)),
        voices_page(&["callirrhoe"], None),
    ])
    .await;
    let client = stub.client();

    let voices: Vec<Voice> = client
        .voices()
        .list()
        .with_page_size(3)
        .with_voice_type(VoiceType::Prebuilt)
        .with_language_code("en-US")
        .with_gender("female")
        .items()
        .try_collect()
        .await
        .unwrap();

    let ids: Vec<_> = voices.iter().filter_map(|v| v.id.as_deref()).collect();
    assert_eq!(ids, ["achernar", "aoede", "autonoe", "callirrhoe"]);
    let query = "gender=female&language_code=en-US&type=prebuilt";
    assert_eq!(
        targets(&stub),
        [
            format!("/v1beta/voices?page_size=3&{query}"),
            format!("/v1beta/voices?page_size=3&page_token={TOKEN}&{query}"),
        ]
    );
}

#[tokio::test]
async fn voice_list_pages_resume_from_with_page_token_with_the_filters() {
    let stub = Stub::replying(vec![
        voices_page(&["en-gb-storyteller-5"], Some("p3")),
        voices_page(&[], None),
    ])
    .await;
    let client = stub.client();

    let pages: Vec<_> = client
        .voices()
        .list()
        .with_search("narrator")
        .with_page_token("p2")
        .pages()
        .try_collect()
        .await
        .unwrap();

    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].voices.len(), 1);
    assert!(pages[1].voices.is_empty());
    assert_eq!(
        targets(&stub),
        [
            "/v1beta/voices?page_token=p2&search=narrator",
            "/v1beta/voices?page_token=p3&search=narrator",
        ]
    );
}
