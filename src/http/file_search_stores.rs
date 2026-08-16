//! HTTP endpoints for the `/v1beta/fileSearchStores` resource and its
//! `documents` sub-resource.
//!
//! Two things differ from the other resource modules:
//!
//! - **camelCase on the wire.** Responses use `displayName`/`createTime`/
//!   `sizeBytes`, unlike the Interactions API's snake_case. The types in
//!   `crate::file_search_stores` carry the rename; nothing is needed here.
//! - **A separate upload host path.** Adding a document goes to
//!   `/upload/v1beta/{store}:uploadToFileSearchStore`, not to the regular
//!   resource path, mirroring the Files API. The endpoint accepts both the
//!   `raw` and `multipart` protocols; this module uses `raw` so the crate
//!   need not enable reqwest's `multipart` feature for a single call site.
//!
//! Both paging spellings (`page_size` and `pageSize`) are accepted by this
//! resource, so the shared [`with_paging`] helper works unchanged.

use super::common::{
    API_KEY_HEADER, API_VERSION, BASE_URL_PREFIX, path_segment, require_id, send_and_read, to_body,
    with_paging, with_paging_and,
};
use super::context::HttpContext;
use super::error_helpers::{check_response_wire, deserialize_with_context};
use crate::errors::GenaiError;
use crate::file_search_stores::{
    CreateFileSearchStoreRequest, DocumentListResponse, FileSearchDocument, FileSearchStore,
    FileSearchStoreListResponse,
};
use crate::wire::WireEvent;
use std::path::Path;

/// Upload ceiling, matching the Files API's `MAX_FILE_SIZE` (2 GB).
const MAX_UPLOAD_SIZE: u64 = 2_147_483_648;

fn stores_url() -> String {
    format!("{BASE_URL_PREFIX}/{API_VERSION}/fileSearchStores")
}

/// Validates a `fileSearchStores/<id>` resource name and rebuilds it as a
/// URL path fragment with the ID percent-encoded.
///
/// Same positive-shape check as the Files API's validator: prefix present,
/// exactly one non-empty segment after it, ID routed through
/// [`path_segment`]. A name failing this shape could never have addressed a
/// store, so rejecting locally turns a silent misfire into a loud one.
fn store_resource_path(store_name: &str) -> Result<String, GenaiError> {
    let Some(id) = store_name.strip_prefix("fileSearchStores/") else {
        return Err(GenaiError::InvalidInput(format!(
            "store name must be a full `fileSearchStores/<id>` resource name \
             (the form create responses return); got {store_name:?}"
        )));
    };
    require_id(id, "file search store")?;
    if id.contains('/') {
        return Err(GenaiError::InvalidInput(format!(
            "store name must contain exactly one segment after \
             `fileSearchStores/`; got {store_name:?}"
        )));
    }
    Ok(format!("fileSearchStores/{}", path_segment(id)))
}

/// Validates a `fileSearchStores/<store>/documents/<doc>` resource name and
/// rebuilds it with both IDs percent-encoded.
fn document_resource_path(document_name: &str) -> Result<String, GenaiError> {
    let Some(rest) = document_name.strip_prefix("fileSearchStores/") else {
        return Err(GenaiError::InvalidInput(format!(
            "document name must be a full \
             `fileSearchStores/<store>/documents/<id>` resource name; \
             got {document_name:?}"
        )));
    };
    let Some((store_id, doc_id)) = rest.split_once("/documents/") else {
        return Err(GenaiError::InvalidInput(format!(
            "document name must contain a `/documents/` segment; \
             got {document_name:?}"
        )));
    };
    require_id(store_id, "file search store")?;
    require_id(doc_id, "document")?;
    if store_id.contains('/') || doc_id.contains('/') {
        return Err(GenaiError::InvalidInput(format!(
            "document name must have exactly one store segment and one \
             document segment; got {document_name:?}"
        )));
    }
    Ok(format!(
        "fileSearchStores/{}/documents/{}",
        path_segment(store_id),
        path_segment(doc_id)
    ))
}

fn store_url(store_name: &str) -> Result<String, GenaiError> {
    Ok(format!(
        "{BASE_URL_PREFIX}/{API_VERSION}/{}",
        store_resource_path(store_name)?
    ))
}

fn document_url(document_name: &str) -> Result<String, GenaiError> {
    Ok(format!(
        "{BASE_URL_PREFIX}/{API_VERSION}/{}",
        document_resource_path(document_name)?
    ))
}

fn documents_url(store_name: &str) -> Result<String, GenaiError> {
    Ok(format!(
        "{BASE_URL_PREFIX}/{API_VERSION}/{}/documents",
        store_resource_path(store_name)?
    ))
}

/// Creates a file search store (`POST /v1beta/fileSearchStores`).
pub async fn create_file_search_store(
    ctx: &HttpContext,
    request: &CreateFileSearchStoreRequest,
) -> Result<FileSearchStore, GenaiError> {
    tracing::debug!("Creating file search store");
    let text = send_and_read(
        ctx,
        reqwest::Method::POST,
        &stores_url(),
        Some(to_body(request)?),
    )
    .await?;
    deserialize_with_context(&text, "FileSearchStore from create")
}

/// Retrieves a store (`GET /v1beta/fileSearchStores/{id}`).
pub async fn get_file_search_store(
    ctx: &HttpContext,
    store_name: &str,
) -> Result<FileSearchStore, GenaiError> {
    let url = store_url(store_name)?;
    tracing::debug!("Getting file search store: {store_name}");
    let text = send_and_read(ctx, reqwest::Method::GET, &url, None).await?;
    deserialize_with_context(&text, "FileSearchStore from get")
}

/// Lists stores (`GET /v1beta/fileSearchStores`).
pub async fn list_file_search_stores(
    ctx: &HttpContext,
    page_size: Option<u32>,
    page_token: Option<&str>,
) -> Result<FileSearchStoreListResponse, GenaiError> {
    tracing::debug!("Listing file search stores: page_size={page_size:?}");
    let url = with_paging(stores_url(), page_size, page_token);
    let text = send_and_read(ctx, reqwest::Method::GET, &url, None).await?;
    deserialize_with_context(&text, "FileSearchStoreListResponse")
}

/// Deletes a store (`DELETE /v1beta/fileSearchStores/{id}`).
///
/// `force` deletes the store even when it still holds documents; without it
/// a non-empty store is rejected with `FAILED_PRECONDITION`.
pub async fn delete_file_search_store(
    ctx: &HttpContext,
    store_name: &str,
    force: bool,
) -> Result<(), GenaiError> {
    let mut url = store_url(store_name)?;
    if force {
        url = with_paging_and(url, None, None, &[("force", "true")]);
    }
    tracing::debug!("Deleting file search store: {store_name} (force={force})");
    send_and_read(ctx, reqwest::Method::DELETE, &url, None).await?;
    Ok(())
}

/// Lists documents in a store
/// (`GET /v1beta/fileSearchStores/{id}/documents`).
pub async fn list_documents(
    ctx: &HttpContext,
    store_name: &str,
    page_size: Option<u32>,
    page_token: Option<&str>,
) -> Result<DocumentListResponse, GenaiError> {
    tracing::debug!("Listing documents in store: {store_name}");
    let url = with_paging(documents_url(store_name)?, page_size, page_token);
    let text = send_and_read(ctx, reqwest::Method::GET, &url, None).await?;
    deserialize_with_context(&text, "DocumentListResponse")
}

/// Retrieves a document
/// (`GET /v1beta/fileSearchStores/{store}/documents/{id}`).
pub async fn get_document(
    ctx: &HttpContext,
    document_name: &str,
) -> Result<FileSearchDocument, GenaiError> {
    let url = document_url(document_name)?;
    tracing::debug!("Getting document: {document_name}");
    let text = send_and_read(ctx, reqwest::Method::GET, &url, None).await?;
    deserialize_with_context(&text, "FileSearchDocument from get")
}

/// Deletes a document
/// (`DELETE /v1beta/fileSearchStores/{store}/documents/{id}`).
///
/// `force` is required for a document that has been chunked — which is every
/// successfully indexed document. Without it the API responds
/// `400 Cannot delete non-empty Document` (verified live 2026-08-16), so
/// callers deleting an indexed document want `force = true`.
pub async fn delete_document(
    ctx: &HttpContext,
    document_name: &str,
    force: bool,
) -> Result<(), GenaiError> {
    let mut url = document_url(document_name)?;
    if force {
        url = with_paging_and(url, None, None, &[("force", "true")]);
    }
    tracing::debug!("Deleting document: {document_name} (force={force})");
    send_and_read(ctx, reqwest::Method::DELETE, &url, None).await?;
    Ok(())
}

/// Uploads a local file into a store
/// (`POST /upload/v1beta/{store}:uploadToFileSearchStore`).
///
/// Uses the **raw** upload protocol — file bytes as the request body, with
/// `display_name` carried as a query parameter — rather than the multipart
/// form the API also accepts. Both were verified live 2026-08-16; raw is used
/// because multipart would require enabling reqwest's `multipart` feature,
/// and this endpoint is the crate's only would-be user of it.
///
/// The API responds with an operation wrapper whose `response.documentName`
/// names the created document; this function resolves that into a
/// [`FileSearchDocument`] by fetching the document, so callers get the same
/// shape the list and get endpoints return.
///
/// Newly created documents start in
/// [`DocumentState::Pending`](crate::file_search_stores::DocumentState::Pending)
/// and are not queryable until they reach `Active` — see
/// [`wait_for_document_active`](crate::Client::wait_for_document_active).
pub async fn upload_to_file_search_store(
    ctx: &HttpContext,
    store_name: &str,
    file_path: &Path,
    display_name: Option<&str>,
    mime_type: &str,
) -> Result<FileSearchDocument, GenaiError> {
    let store_path = store_resource_path(store_name)?;
    let mut url =
        format!("{BASE_URL_PREFIX}/upload/{API_VERSION}/{store_path}:uploadToFileSearchStore");
    if let Some(name) = display_name {
        url = with_paging_and(url, None, None, &[("display_name", name)]);
    }

    // Same two degenerate-size guards the Files API upload applies, so the
    // failure surface matches the path this module mirrors: without them a
    // zero-byte file becomes an opaque server-side error, and an oversized
    // one is read fully into memory here before anything rejects it.
    let metadata = tokio::fs::metadata(file_path).await.map_err(|e| {
        GenaiError::InvalidInput(format!("Failed to stat {}: {e}", file_path.display()))
    })?;
    if metadata.len() == 0 {
        return Err(GenaiError::InvalidInput(
            "Cannot upload empty file".to_string(),
        ));
    }
    if metadata.len() > MAX_UPLOAD_SIZE {
        return Err(GenaiError::InvalidInput(format!(
            "File size {} exceeds maximum {}",
            metadata.len(),
            MAX_UPLOAD_SIZE
        )));
    }

    let bytes = tokio::fs::read(file_path).await.map_err(|e| {
        GenaiError::InvalidInput(format!("Failed to read {}: {e}", file_path.display()))
    })?;

    // The API derives a fallback display name from this when the query
    // parameter is absent, so send it either way.
    let file_name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("upload")
        .to_string();

    let request_id = ctx.next_request_id();
    ctx.emit_request(
        request_id,
        "POST",
        &url,
        Some(&serde_json::json!({
            "display_name": display_name,
            "file_name": file_name,
            "mime_type": mime_type,
            "size_bytes": bytes.len(),
        })),
    );

    let response = ctx
        .http_client
        .post(&url)
        .header(API_KEY_HEADER, &ctx.api_key)
        .header("X-Goog-Upload-Protocol", "raw")
        .header("X-Goog-Upload-File-Name", file_name)
        .header(reqwest::header::CONTENT_TYPE, mime_type)
        .body(bytes)
        .send()
        .await?;

    ctx.emit(WireEvent::ResponseStatus {
        id: request_id,
        status: response.status().as_u16(),
    });

    let response = check_response_wire(response, ctx, request_id).await?;
    let text = response.text().await.map_err(GenaiError::Http)?;

    ctx.emit_response_body(request_id, &text);

    // The upload returns an operation wrapper, not the document itself:
    //   {"name": ".../upload/operations/...",
    //    "response": {"documentName": "...", "mimeType": ..., "sizeBytes": ...}}
    let operation: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
        GenaiError::Internal(format!("Failed to parse upload operation response: {e}"))
    })?;

    let document_name = operation
        .get("response")
        .and_then(|r| r.get("documentName"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            // Every upload observed (text and PDF, through ~1 MB, 2026-08-16)
            // came back already resolved, with `response.documentName`
            // present — no polling was needed. This arm therefore covers an
            // unobserved shape, so it surfaces the whole raw response
            // including `name`, which is the operation handle a caller would
            // need to resolve it out-of-band.
            GenaiError::Internal(format!(
                "Upload operation did not report a documentName (never observed \
                 through ~1 MB as of 2026-08-16; the operation may be unresolved). \
                 Raw response: {text}"
            ))
        })?
        .to_string();

    // The bytes are already accepted by this point, so a failure here — a
    // transient 5xx, or the document not being readable yet on this
    // read-after-write — means the upload succeeded but the caller is holding
    // an error. Without the name in it they cannot wait on the document,
    // delete it, or tell a failed upload from a landed one; recovery would
    // mean listing the store and guessing by display name, which is not
    // unique. Same reasoning as the unresolved-operation arm above: keep the
    // handle attached to the error.
    get_document(ctx, &document_name).await.map_err(|e| {
        // Logged unconditionally, so the name survives even on the arms
        // below that return the error untouched.
        tracing::error!(
            "Upload succeeded and created '{}', but reading it back failed: {}. \
             The document exists — use that name to wait on or delete it.",
            document_name,
            e
        );

        let context = format!(
            "Upload succeeded and created '{document_name}', but reading it \
             back failed. The document exists — use that name to wait on or \
             delete it. Cause: "
        );

        // Never collapse a retryable error into `Internal`. The two failures
        // this arm exists for — a transient 5xx, and the document not being
        // readable yet on this read-after-write — are exactly the ones
        // `GenaiError::is_retryable()` reports true for, while `Internal`
        // reports false. Wrapping them would tell a caller following
        // `examples/retry_with_backoff.rs` to give up on a read-back that
        // would have succeeded on the next attempt.
        match e {
            // Rebuildable, so it carries the name and stays classifiable.
            GenaiError::Api {
                status_code,
                message,
                request_id,
                retry_after,
            } => GenaiError::Api {
                status_code,
                message: format!("{context}{message}"),
                request_id,
                retry_after,
            },
            // Not rebuildable with added context. Retryability matters more
            // than the name here, and the name is in the log above.
            other if other.is_retryable() => other,
            other => GenaiError::Internal(format!("{context}{other}")),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_url_construction() {
        assert_eq!(
            stores_url(),
            "https://generativelanguage.googleapis.com/v1beta/fileSearchStores"
        );
        assert_eq!(
            store_url("fileSearchStores/abc123").unwrap(),
            "https://generativelanguage.googleapis.com/v1beta/fileSearchStores/abc123"
        );
        assert_eq!(
            documents_url("fileSearchStores/abc123").unwrap(),
            "https://generativelanguage.googleapis.com/v1beta/fileSearchStores/abc123/documents"
        );
    }

    #[test]
    fn document_url_construction() {
        assert_eq!(
            document_url("fileSearchStores/abc123/documents/doc1").unwrap(),
            "https://generativelanguage.googleapis.com/v1beta/fileSearchStores/abc123/documents/doc1"
        );
    }

    #[test]
    fn store_name_must_be_a_full_resource_name() {
        // A bare ID would address the collection URL, not a store.
        let err = store_url("abc123").unwrap_err();
        assert!(matches!(err, GenaiError::InvalidInput(_)));
        assert!(err.to_string().contains("fileSearchStores/<id>"));
    }

    #[test]
    fn store_name_rejects_extra_segments() {
        let err = store_url("fileSearchStores/abc/extra").unwrap_err();
        assert!(matches!(err, GenaiError::InvalidInput(_)));
    }

    #[test]
    fn store_name_rejects_empty_and_dot_segments() {
        assert!(store_url("fileSearchStores/").is_err());
        assert!(store_url("fileSearchStores/..").is_err());
        assert!(store_url("fileSearchStores/%2e%2e").is_err());
    }

    #[test]
    fn store_id_metacharacters_are_encoded_not_interpolated() {
        assert_eq!(
            store_url("fileSearchStores/a?b#c").unwrap(),
            "https://generativelanguage.googleapis.com/v1beta/fileSearchStores/a%3Fb%23c"
        );
    }

    #[test]
    fn document_name_requires_documents_segment() {
        let err = document_url("fileSearchStores/abc123").unwrap_err();
        assert!(err.to_string().contains("/documents/"));
    }

    #[test]
    fn document_name_must_start_with_store_prefix() {
        let err = document_url("documents/doc1").unwrap_err();
        assert!(matches!(err, GenaiError::InvalidInput(_)));
    }

    #[test]
    fn document_ids_are_encoded() {
        assert_eq!(
            document_url("fileSearchStores/a b/documents/c d").unwrap(),
            "https://generativelanguage.googleapis.com/v1beta/fileSearchStores/a%20b/documents/c%20d"
        );
    }
}
