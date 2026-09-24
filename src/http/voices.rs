//! HTTP endpoints for the `/v1beta/voices` resource.

use super::common::{NO_BODY, path_segment, require_id, send_and_read, with_paging, with_query};
use super::context::HttpContext;
use super::error_helpers::deserialize_with_context;
use crate::errors::GenaiError;
use crate::voices::{CreateVoiceRequest, ListVoicesParams, Voice, VoiceListResponse};

fn voices_url(ctx: &HttpContext) -> String {
    ctx.api_url("voices")
}

fn voice_url(ctx: &HttpContext, id: &str) -> String {
    ctx.api_url(&format!("voices/{}", path_segment(id)))
}

fn list_url(ctx: &HttpContext, params: &ListVoicesParams) -> String {
    let filters = params.filters();
    let query: Vec<(&str, Option<&str>)> = filters
        .iter()
        .map(|(key, value)| (*key, Some(value.as_str())))
        .collect();
    let url = with_paging(
        voices_url(ctx),
        params.page_size,
        params.page_token.as_deref(),
    );
    with_query(url, &query)
}

/// Lists voices (`GET /v1beta/voices`).
pub async fn list_voices(
    ctx: &HttpContext,
    params: &ListVoicesParams,
) -> Result<VoiceListResponse, GenaiError> {
    tracing::debug!("Listing voices: {params:?}");
    let text = send_and_read(ctx, reqwest::Method::GET, &list_url(ctx, params), NO_BODY).await?;
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
        let params = ListVoicesParams::new()
            .with_page_size(5)
            .with_search("warm voice")
            .with_voice_type(VoiceType::Prebuilt)
            .with_pitch(VoicePitch::High);
        assert_eq!(
            list_url(&ctx(), &params),
            "https://generativelanguage.googleapis.com/v1beta/voices\
             ?page_size=5&search=warm%20voice&type=prebuilt&pitch=high"
        );
    }
}
