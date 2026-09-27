//! HTTP endpoints for the `/v1beta/voices` resource.

use super::common::{NO_BODY, path_segment, require_id, send_and_read, with_paging, with_query};
use super::context::HttpContext;
use super::error_helpers::deserialize_with_context;
use crate::errors::GenaiError;
use crate::voices::{CreateVoiceRequest, Voice, VoiceFilters, VoiceListResponse};

fn voices_url(ctx: &HttpContext) -> String {
    ctx.api_url("voices")
}

fn voice_url(ctx: &HttpContext, id: &str) -> String {
    ctx.api_url(&format!("voices/{}", path_segment(id)))
}

/// The list URL: paging first, then the filters in wire names.
fn list_url(
    ctx: &HttpContext,
    filters: &VoiceFilters,
    page_size: Option<u32>,
    page_token: Option<&str>,
) -> String {
    let pairs = filters.query_pairs();
    let query: Vec<(&str, Option<&str>)> = pairs
        .iter()
        .map(|(key, value)| (*key, Some(value.as_str())))
        .collect();
    with_query(with_paging(voices_url(ctx), page_size, page_token), &query)
}

/// Lists voices (`GET /v1beta/voices`).
pub async fn list_voices(
    ctx: &HttpContext,
    filters: &VoiceFilters,
    page_size: Option<u32>,
    page_token: Option<&str>,
) -> Result<VoiceListResponse, GenaiError> {
    tracing::debug!(
        "Listing voices: filters={filters:?}, page_size={page_size:?}, page_token={page_token:?}"
    );
    let url = list_url(ctx, filters, page_size, page_token);
    let text = send_and_read(ctx, reqwest::Method::GET, &url, NO_BODY).await?;
    deserialize_with_context(&text, "VoiceListResponse")
}

/// Retrieves a voice (`GET /v1beta/voices/{id}`).
pub async fn get_voice(ctx: &HttpContext, voice_id: &str) -> Result<Voice, GenaiError> {
    require_id(voice_id, "voice")?;
    tracing::debug!("Getting voice: ID={voice_id}");
    let text = send_and_read(
        ctx,
        reqwest::Method::GET,
        &voice_url(ctx, voice_id),
        NO_BODY,
    )
    .await?;
    deserialize_with_context(&text, "Voice from get")
}

/// Creates a voice (`POST /v1beta/voices`).
pub async fn create_voice(
    ctx: &HttpContext,
    request: &CreateVoiceRequest,
) -> Result<Voice, GenaiError> {
    tracing::debug!("Creating voice: type={:?}", request.voice.voice_type);
    let text = send_and_read(ctx, reqwest::Method::POST, &voices_url(ctx), Some(request)).await?;
    deserialize_with_context(&text, "Voice from create")
}

/// Deletes a voice (`DELETE /v1beta/voices/{id}`).
pub async fn delete_voice(ctx: &HttpContext, voice_id: &str) -> Result<(), GenaiError> {
    require_id(voice_id, "voice")?;
    tracing::debug!("Deleting voice: ID={voice_id}");
    send_and_read(
        ctx,
        reqwest::Method::DELETE,
        &voice_url(ctx, voice_id),
        NO_BODY,
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voices::{VoicePitch, VoiceType};

    fn ctx() -> HttpContext {
        HttpContext::new(reqwest::Client::new(), "k".to_string(), vec![])
    }

    #[test]
    fn voice_urls() {
        let ctx = ctx();
        assert_eq!(
            voices_url(&ctx),
            "https://generativelanguage.googleapis.com/v1beta/voices"
        );
        assert_eq!(
            voice_url(&ctx, "voice_abc"),
            "https://generativelanguage.googleapis.com/v1beta/voices/voice_abc"
        );
        assert_eq!(
            voice_url(&ctx, "a/b"),
            "https://generativelanguage.googleapis.com/v1beta/voices/a%2Fb"
        );
    }

    #[test]
    fn list_url_carries_filters_and_paging() {
        let filters = VoiceFilters {
            search: Some("warm voice".into()),
            voice_type: Some(VoiceType::Prebuilt),
            pitch: Some(VoicePitch::High),
            ..Default::default()
        };
        assert_eq!(
            list_url(&ctx(), &filters, Some(5), Some("t/1")),
            "https://generativelanguage.googleapis.com/v1beta/voices\
             ?page_size=5&page_token=t%2F1&search=warm%20voice&type=prebuilt&pitch=high"
        );
        assert_eq!(
            list_url(&ctx(), &VoiceFilters::default(), None, None),
            "https://generativelanguage.googleapis.com/v1beta/voices"
        );
    }
}
