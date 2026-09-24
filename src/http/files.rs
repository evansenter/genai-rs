//! HTTP calls for the Files API; the types live in [`crate::files`].

use super::common::{
    API_KEY_HEADER, NO_BODY, mime_type_header, path_segment, require_id, send_and_read,
    send_checked, with_query,
};
use super::context::HttpContext;
use super::error_helpers::deserialize_with_context;
use crate::errors::GenaiError;
use crate::files::{FileMetadata, FileUploadResponse, ListFilesResponse};
use crate::wire::WireEvent;
use std::path::Path;
use tokio_util::io::ReaderStream;

// --- API Functions ---

/// Maximum file size for uploads (2 GB)
const MAX_FILE_SIZE: u64 = 2_147_483_648;

/// Rejects an empty or oversized upload before any request is made.
fn check_upload_size(file_size: u64) -> Result<(), GenaiError> {
    if file_size == 0 {
        return Err(GenaiError::InvalidInput(
            "Cannot upload empty file".to_string(),
        ));
    }
    if file_size > MAX_FILE_SIZE {
        return Err(GenaiError::InvalidInput(format!(
            "File size {} bytes exceeds maximum allowed size of {} bytes (2 GB)",
            file_size, MAX_FILE_SIZE
        )));
    }
    Ok(())
}

/// Starts a resumable upload session and returns its upload URL.
///
/// Emits [`WireEvent::UploadStart`] under `request_id`.
async fn start_upload_session(
    ctx: &HttpContext,
    request_id: u64,
    file_label: &str,
    file_size: u64,
    mime_type: &str,
    display_name: Option<&str>,
) -> Result<String, GenaiError> {
    let mime_type_value = mime_type_header(mime_type)?;
    if ctx.has_inspectors() {
        ctx.emit(WireEvent::UploadStart {
            id: request_id,
            file_name: file_label.to_string(),
            mime_type: mime_type.to_string(),
            size_bytes: file_size,
        });
    }

    let metadata = match display_name {
        Some(name) => serde_json::json!({ "file": { "displayName": name } }),
        None => serde_json::json!({ "file": {} }),
    };
    let builder = ctx
        .http_client
        .post(ctx.upload_url("files"))
        .header(API_KEY_HEADER, &ctx.api_key)
        .header("X-Goog-Upload-Protocol", "resumable")
        .header("X-Goog-Upload-Command", "start")
        .header("X-Goog-Upload-Header-Content-Length", file_size.to_string())
        .header("X-Goog-Upload-Header-Content-Type", mime_type_value)
        .json(&metadata);
    let response = send_checked(ctx, request_id, builder).await?;

    response
        .headers()
        .get("x-goog-upload-url")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .ok_or_else(|| {
            GenaiError::MalformedResponse(
                "Upload session response is missing the x-goog-upload-url header".to_string(),
            )
        })
}

/// Sends the whole body to an upload session and finalizes it.
///
/// Emits [`WireEvent::UploadComplete`] under `request_id`.
async fn finish_upload(
    ctx: &HttpContext,
    request_id: u64,
    upload_url: &str,
    file_size: u64,
    body: reqwest::Body,
) -> Result<FileMetadata, GenaiError> {
    let builder = ctx
        .http_client
        .post(upload_url)
        .header("X-Goog-Upload-Offset", "0")
        .header("X-Goog-Upload-Command", "upload, finalize")
        .header("Content-Length", file_size.to_string())
        .body(body);
    let response = send_checked(ctx, request_id, builder).await?;
    let response_text = response.text().await?;
    let file =
        deserialize_with_context::<FileUploadResponse>(&response_text, "FileUploadResponse")?.file;

    tracing::debug!("File uploaded: name={}, uri={}", file.name, file.uri);
    if ctx.has_inspectors() {
        ctx.emit(WireEvent::UploadComplete {
            id: request_id,
            uri: file.uri.clone(),
        });
    }
    Ok(file)
}

/// Uploads in-memory bytes to the Files API.
///
/// # Errors
///
/// Returns [`GenaiError::InvalidInput`] for an empty or oversized file or an
/// unheaderable MIME type, or an error if either request of the upload fails.
pub async fn upload_bytes(
    ctx: &HttpContext,
    file_data: Vec<u8>,
    mime_type: &str,
    display_name: Option<&str>,
) -> Result<FileMetadata, GenaiError> {
    let file_size = file_data.len() as u64;
    check_upload_size(file_size)?;
    tracing::debug!(
        "Uploading file: size={} bytes, mime_type={}, display_name={:?}",
        file_size,
        mime_type,
        display_name
    );

    let request_id = ctx.next_request_id();
    let upload_url = start_upload_session(
        ctx,
        request_id,
        display_name.unwrap_or("(unnamed)"),
        file_size,
        mime_type,
        display_name,
    )
    .await?;
    finish_upload(ctx, request_id, &upload_url, file_size, file_data.into()).await
}

/// Read-buffer size for streaming a file from disk, which bounds memory use
/// per upload; the body still goes up as one `upload, finalize` request.
const READ_BUFFER_SIZE: usize = 8 * 1024 * 1024;

/// Uploads a file from disk, streaming it rather than reading it into memory.
///
/// # Errors
///
/// Returns [`GenaiError::InvalidInput`] if the file cannot be read, is empty
/// or oversized, or the MIME type is unheaderable, or an error if the upload
/// fails.
pub async fn upload_path(
    ctx: &HttpContext,
    path: &Path,
    mime_type: &str,
    display_name: Option<&str>,
) -> Result<FileMetadata, GenaiError> {
    let read_error = |e: std::io::Error| {
        GenaiError::InvalidInput(format!("Failed to read file '{}': {e}", path.display()))
    };
    // Size the upload from the open handle, so it describes the file sent.
    let file = tokio::fs::File::open(path).await.map_err(read_error)?;
    let file_size = file.metadata().await.map_err(read_error)?.len();
    check_upload_size(file_size)?;
    tracing::debug!(
        "Uploading file: path={}, size={} bytes, mime_type={}",
        path.display(),
        file_size,
        mime_type,
    );

    let request_id = ctx.next_request_id();
    let file_label = display_name
        .map(str::to_string)
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    let upload_url = start_upload_session(
        ctx,
        request_id,
        &file_label,
        file_size,
        mime_type,
        display_name,
    )
    .await?;

    let body = reqwest::Body::wrap_stream(ReaderStream::with_capacity(file, READ_BUFFER_SIZE));
    finish_upload(ctx, request_id, &upload_url, file_size, body).await
}

/// Validates a `files/<id>` resource name and rebuilds it with the ID
/// percent-encoded.
fn file_resource_path(file_name: &str) -> Result<String, GenaiError> {
    let Some(id) = file_name.strip_prefix("files/") else {
        return Err(GenaiError::InvalidInput(format!(
            "file name must be a full `files/<id>` resource name \
             (the form upload responses return); got {file_name:?}"
        )));
    };
    require_id(id, "file")?;
    if id.contains('/') {
        return Err(GenaiError::InvalidInput(format!(
            "file name must contain exactly one segment after `files/`; \
             got {file_name:?}"
        )));
    }
    Ok(format!("files/{}", path_segment(id)))
}

/// Gets metadata for a file by its full resource name (`files/abc123`).
///
/// # Errors
///
/// Returns [`GenaiError::InvalidInput`] for a name that is not
/// `files/<id>`, or an error if the request fails or the file doesn't exist.
pub async fn get_file(ctx: &HttpContext, file_name: &str) -> Result<FileMetadata, GenaiError> {
    let url = ctx.api_url(&file_resource_path(file_name)?);
    let text = send_and_read(ctx, reqwest::Method::GET, &url, NO_BODY).await?;
    let file: FileMetadata = deserialize_with_context(&text, "FileMetadata")?;
    tracing::debug!("Got file {}: state={:?}", file.name, file.state);
    Ok(file)
}

/// The list URL. The Files API spells its paging params in camelCase.
fn list_files_url(ctx: &HttpContext, page_size: Option<u32>, page_token: Option<&str>) -> String {
    let page_size = page_size.map(|size| size.to_string());
    with_query(
        ctx.api_url("files"),
        &[
            ("pageSize", page_size.as_deref()),
            ("pageToken", page_token),
        ],
    )
}

/// Lists uploaded files.
///
/// # Errors
///
/// Returns an error if the request fails.
pub async fn list_files(
    ctx: &HttpContext,
    page_size: Option<u32>,
    page_token: Option<&str>,
) -> Result<ListFilesResponse, GenaiError> {
    let url = list_files_url(ctx, page_size, page_token);
    let text = send_and_read(ctx, reqwest::Method::GET, &url, NO_BODY).await?;
    let list: ListFilesResponse = deserialize_with_context(&text, "ListFilesResponse")?;
    tracing::debug!("Listed {} files", list.files.len());
    Ok(list)
}

/// Deletes a file by its full resource name (`files/abc123`).
///
/// # Errors
///
/// Returns [`GenaiError::InvalidInput`] for a name that is not
/// `files/<id>`, or an error if the request fails or the file doesn't exist.
pub async fn delete_file(ctx: &HttpContext, file_name: &str) -> Result<(), GenaiError> {
    let url = ctx.api_url(&file_resource_path(file_name)?);
    send_and_read(ctx, reqwest::Method::DELETE, &url, NO_BODY).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_files_url_encodes_paging() {
        let ctx = HttpContext::new(reqwest::Client::new(), "k".to_string(), vec![]);
        let list_files_url = |size, token| super::list_files_url(&ctx, size, token);
        let base = "https://generativelanguage.googleapis.com/v1beta/files";
        assert_eq!(list_files_url(None, None), base);
        assert_eq!(
            list_files_url(Some(10), None),
            format!("{base}?pageSize=10")
        );
        // Token alone takes the leading `?`; with a size it rides `&`.
        assert_eq!(
            list_files_url(None, Some("tok")),
            format!("{base}?pageToken=tok")
        );
        assert_eq!(
            list_files_url(Some(10), Some("tok")),
            format!("{base}?pageSize=10&pageToken=tok")
        );
        // `+` is what a standard-base64 token would hit (it would otherwise
        // decode to a space server-side).
        assert_eq!(
            list_files_url(None, Some("a/b&c=d+e")),
            format!("{base}?pageToken=a%2Fb%26c%3Dd%2Be")
        );
    }

    #[test]
    fn file_resource_path_validates_shape_and_encodes() {
        // The happy path: the exact shape upload responses return, with
        // the opaque ID passing through byte-identical.
        assert_eq!(file_resource_path("files/abc123").unwrap(), "files/abc123");
        // Dots inside an ID are not dot segments and stay verbatim.
        assert_eq!(file_resource_path("files/a.b.c").unwrap(), "files/a.b.c");

        // Shape violations reject locally: missing prefix, empty ID,
        // extra segments (which also covers `files/../x`), and every
        // dot-segment spelling — WHATWG dot-segment removal matches the
        // percent-encoded forms case-insensitively at parse time, so
        // `require_id` rejects them all rather than trusting encoding.
        assert!(file_resource_path("abc123").is_err());
        assert!(file_resource_path("").is_err());
        assert!(file_resource_path("files/").is_err());
        assert!(file_resource_path("files/a/b").is_err());
        assert!(file_resource_path("../../v1beta/files/other").is_err());
        assert!(file_resource_path("files/..").is_err());
        assert!(file_resource_path("files/%2e%2e").is_err());
        assert!(file_resource_path("files/%2E.").is_err());

        // Threats that pass the shape check are defused by encoding, so
        // URL parsing can never treat them structurally: the
        // query/fragment splitters that would silently retarget the
        // request.
        assert_eq!(
            file_resource_path("files/abc?alt=media").unwrap(),
            "files/abc%3Falt%3Dmedia"
        );
        assert_eq!(
            file_resource_path("files/abc#frag").unwrap(),
            "files/abc%23frag"
        );
    }
}
