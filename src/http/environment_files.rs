//! HTTP endpoints for files inside an environment:
//! `GET /v1beta/environments/{id}/files/{path}` and the resumable
//! `PUT /upload/v1beta/environments/{id}/files/{path}`.

use super::common::{
    NO_BODY, api_request, path_segment, require_id, send_and_read, send_checked, with_paging,
    with_query,
};
use super::context::HttpContext;
use super::error_helpers::deserialize_with_context;
use crate::environments::{EnvironmentFileList, EnvironmentFileUpload};
use crate::errors::GenaiError;

/// Percent-encodes each segment of a relative file path, keeping the `/`
/// separators. `.`/`..` segments are rejected: URL normalization would
/// resolve them before the request leaves the client.
fn encode_file_path(path: &str) -> Result<String, GenaiError> {
    let segments = path.trim_start_matches('/').split('/');
    let mut encoded = Vec::new();
    for segment in segments {
        if segment == "." || segment == ".." {
            return Err(GenaiError::InvalidInput(format!(
                "environment file path must not contain '.' or '..' segments: {path:?}"
            )));
        }
        encoded.push(path_segment(segment));
    }
    Ok(encoded.join("/"))
}

fn files_path(environment_id: &str, path: &str) -> Result<String, GenaiError> {
    require_id(environment_id, "environment")?;
    Ok(format!(
        "environments/{}/files/{}",
        path_segment(environment_id),
        encode_file_path(path)?
    ))
}

fn list_url(
    ctx: &HttpContext,
    environment_id: &str,
    path: &str,
    recursive: bool,
    page_size: Option<u32>,
    page_token: Option<&str>,
) -> Result<String, GenaiError> {
    let url = with_paging(
        ctx.api_url(&files_path(environment_id, path)?),
        page_size,
        page_token,
    );
    Ok(with_query(
        url,
        &[("recursive", recursive.then_some("true"))],
    ))
}

fn upload_start_url(
    ctx: &HttpContext,
    environment_id: &str,
    path: &str,
    options: EnvironmentFileUpload,
) -> Result<String, GenaiError> {
    Ok(with_query(
        ctx.upload_url(&files_path(environment_id, path)?),
        &[
            ("overwrite", options.overwrite.then_some("true")),
            ("extract", options.extract.then_some("true")),
        ],
    ))
}

/// Lists entries at a path (`GET /v1beta/environments/{id}/files/{path}`).
pub async fn list_files(
    ctx: &HttpContext,
    environment_id: &str,
    path: &str,
    recursive: bool,
    page_size: Option<u32>,
    page_token: Option<&str>,
) -> Result<EnvironmentFileList, GenaiError> {
    tracing::debug!("Listing environment files: env={environment_id}, path={path:?}");
    let url = list_url(ctx, environment_id, path, recursive, page_size, page_token)?;
    let text = send_and_read(ctx, reqwest::Method::GET, &url, NO_BODY).await?;
    deserialize_with_context(&text, "EnvironmentFileList")
}

/// Uploads one file: starts a resumable session, then sends the bytes and
/// finalizes in a single request.
pub async fn upload_file(
    ctx: &HttpContext,
    environment_id: &str,
    path: &str,
    data: Vec<u8>,
    mime_type: &str,
    options: EnvironmentFileUpload,
) -> Result<EnvironmentFileList, GenaiError> {
    if data.is_empty() {
        return Err(GenaiError::InvalidInput(
            "Cannot upload an empty environment file".to_string(),
        ));
    }
    let size = data.len().to_string();
    let start_url = upload_start_url(ctx, environment_id, path, options)?;
    tracing::debug!(
        "Uploading environment file: env={environment_id}, path={path:?}, {size} bytes"
    );

    let request_id = ctx.next_request_id();
    ctx.emit_request(request_id, "PUT", &start_url, None);
    let start = api_request(ctx, reqwest::Method::PUT, &start_url)
        .header("X-Goog-Upload-Protocol", "resumable")
        .header("X-Goog-Upload-Command", "start")
        .header("X-Goog-Upload-Header-Content-Length", &size)
        .header("X-Goog-Upload-Header-Content-Type", mime_type);
    let response = send_checked(ctx, request_id, start).await?;
    let session_url = response
        .headers()
        .get("x-goog-upload-url")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .ok_or_else(|| {
            GenaiError::MalformedResponse(
                "Upload session response is missing the x-goog-upload-url header".to_string(),
            )
        })?;

    let request_id = ctx.next_request_id();
    ctx.emit_request(request_id, "POST", &session_url, None);
    let finish = ctx
        .http_client
        .post(&session_url)
        .header("X-Goog-Upload-Offset", "0")
        .header("X-Goog-Upload-Command", "upload, finalize")
        .header("Content-Length", size)
        .body(data);
    let response = send_checked(ctx, request_id, finish).await?;
    let text = response.text().await?;
    ctx.emit_response_body(request_id, &text);
    deserialize_with_context(&text, "EnvironmentFileList from upload")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> HttpContext {
        HttpContext::new(reqwest::Client::new(), "k".to_string(), vec![])
    }

    #[test]
    fn list_urls() {
        let ctx = ctx();
        assert_eq!(
            list_url(&ctx, "env1", "", false, None, None).unwrap(),
            "https://generativelanguage.googleapis.com/v1beta/environments/env1/files/"
        );
        assert_eq!(
            list_url(&ctx, "env1", "src/my file.py", true, Some(5), None).unwrap(),
            "https://generativelanguage.googleapis.com/v1beta/environments/env1/files/\
             src/my%20file.py?page_size=5&recursive=true"
        );
    }

    #[test]
    fn upload_url_carries_options() {
        assert_eq!(
            upload_start_url(
                &ctx(),
                "env1",
                "/data/a.tar",
                EnvironmentFileUpload {
                    overwrite: true,
                    extract: true
                }
            )
            .unwrap(),
            "https://generativelanguage.googleapis.com/upload/v1beta/environments/env1/files/\
             data/a.tar?overwrite=true&extract=true"
        );
    }

    #[test]
    fn dot_segments_and_bad_ids_are_rejected() {
        assert!(encode_file_path("a/../b").is_err());
        assert!(encode_file_path("./a").is_err());
        assert!(files_path("", "a").is_err());
        assert_eq!(encode_file_path("a?b/c#d").unwrap(), "a%3Fb/c%23d");
    }
}
