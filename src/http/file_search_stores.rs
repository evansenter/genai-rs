//! HTTP endpoints for the `/v1beta/fileSearchStores` resource and its
//! `documents` sub-resource.
//!
//! Responses are camelCase (`displayName`, `createTime`); the types in
//! `crate::file_search_stores` carry the rename. Uploading a document goes
//! to the upload path (`/upload/v1beta/{store}:uploadToFileSearchStore`)
//! with the resumable protocol the Files API uses, whose start request
//! carries the document's settings as JSON, so the crate need not enable
//! reqwest's `multipart` feature. Importing a file is a JSON `POST
//! {store}:importFile`. The resource accepts `page_size` as well as
//! `pageSize`, so the shared [`with_paging`] helper works unchanged.

use super::common::{
    NO_BODY, file_body, mime_type_header, path_segment, require_id, send_and_read, with_paging,
    with_query,
};
use super::context::HttpContext;
use super::error_helpers::deserialize_with_context;
use super::files::{ResumableUpload, check_upload_size, finish_resumable_upload};
use crate::errors::GenaiError;
use crate::file_search_stores::{
    CreateFileSearchStoreRequest, DocumentImport, DocumentListResponse, DocumentOptions,
    FileSearchDocument, FileSearchStore, FileSearchStoreListResponse,
};
use crate::files::{FileError, UploadSource};
use crate::poll::PollStep;
use reqwest::header::HeaderValue;
use serde::{Deserialize, Serialize};
use std::time::Duration;

fn stores_url(ctx: &HttpContext) -> String {
    ctx.api_url("fileSearchStores")
}

/// Validates a `fileSearchStores/<id>` resource name and rebuilds it as a
/// URL path fragment with the ID percent-encoded.
///
/// A name failing this shape could never have addressed a store, so
/// rejecting locally turns a silent misfire into a loud one.
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

fn store_url(ctx: &HttpContext, store_name: &str) -> Result<String, GenaiError> {
    Ok(ctx.api_url(&store_resource_path(store_name)?))
}

fn document_url(ctx: &HttpContext, document_name: &str) -> Result<String, GenaiError> {
    Ok(ctx.api_url(&document_resource_path(document_name)?))
}

fn documents_url(ctx: &HttpContext, store_name: &str) -> Result<String, GenaiError> {
    Ok(ctx.api_url(&format!("{}/documents", store_resource_path(store_name)?)))
}

/// Creates a file search store (`POST /v1beta/fileSearchStores`).
pub async fn create_file_search_store(
    ctx: &HttpContext,
    request: &CreateFileSearchStoreRequest,
) -> Result<FileSearchStore, GenaiError> {
    tracing::debug!("Creating file search store");
    let text = send_and_read(ctx, reqwest::Method::POST, &stores_url(ctx), Some(request)).await?;
    deserialize_with_context(&text, "FileSearchStore from create")
}

/// Retrieves a store (`GET /v1beta/fileSearchStores/{id}`).
pub async fn get_file_search_store(
    ctx: &HttpContext,
    store_name: &str,
) -> Result<FileSearchStore, GenaiError> {
    let url = store_url(ctx, store_name)?;
    tracing::debug!("Getting file search store: {store_name}");
    let text = send_and_read(ctx, reqwest::Method::GET, &url, NO_BODY).await?;
    deserialize_with_context(&text, "FileSearchStore from get")
}

/// Lists stores (`GET /v1beta/fileSearchStores`).
pub async fn list_file_search_stores(
    ctx: &HttpContext,
    page_size: Option<u32>,
    page_token: Option<&str>,
) -> Result<FileSearchStoreListResponse, GenaiError> {
    tracing::debug!("Listing file search stores: page_size={page_size:?}");
    let url = with_paging(stores_url(ctx), page_size, page_token);
    let text = send_and_read(ctx, reqwest::Method::GET, &url, NO_BODY).await?;
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
    let url = with_query(
        store_url(ctx, store_name)?,
        &[("force", force.then_some("true"))],
    );
    tracing::debug!("Deleting file search store: {store_name} (force={force})");
    send_and_read(ctx, reqwest::Method::DELETE, &url, NO_BODY).await?;
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
    let url = with_paging(documents_url(ctx, store_name)?, page_size, page_token);
    let text = send_and_read(ctx, reqwest::Method::GET, &url, NO_BODY).await?;
    deserialize_with_context(&text, "DocumentListResponse")
}

/// Retrieves a document
/// (`GET /v1beta/fileSearchStores/{store}/documents/{id}`).
pub async fn get_document(
    ctx: &HttpContext,
    document_name: &str,
) -> Result<FileSearchDocument, GenaiError> {
    let url = document_url(ctx, document_name)?;
    tracing::debug!("Getting document: {document_name}");
    let text = send_and_read(ctx, reqwest::Method::GET, &url, NO_BODY).await?;
    deserialize_with_context(&text, "FileSearchDocument from get")
}

/// Deletes a document
/// (`DELETE /v1beta/fileSearchStores/{store}/documents/{id}`).
///
/// `force` is required for a document that has been chunked — which is every
/// successfully indexed document; without it the API responds
/// `400 Cannot delete non-empty Document`.
pub async fn delete_document(
    ctx: &HttpContext,
    document_name: &str,
    force: bool,
) -> Result<(), GenaiError> {
    let url = with_query(
        document_url(ctx, document_name)?,
        &[("force", force.then_some("true"))],
    );
    tracing::debug!("Deleting document: {document_name} (force={force})");
    send_and_read(ctx, reqwest::Method::DELETE, &url, NO_BODY).await?;
    Ok(())
}

/// The JSON body that starts a document upload: the bindings'
/// `UploadToFileSearchStoreConfig` (camelCase on the wire).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UploadDocumentBody<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) display_name: Option<&'a str>,
    /// Also sent as the session's `X-Goog-Upload-Header-Content-Type`; the
    /// body's wins when they differ (live 2026-11-05).
    pub(crate) mime_type: &'a str,
    #[serde(flatten)]
    pub(crate) options: &'a DocumentOptions,
}

/// The JSON body of `POST {store}:importFile`: the file's name beside the
/// bindings' `ImportFileConfig`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImportFileBody<'a> {
    pub(crate) file_name: &'a str,
    #[serde(flatten)]
    pub(crate) options: &'a DocumentOptions,
}

/// Uploads a file from disk or memory into a store
/// (`POST /upload/v1beta/{store}:uploadToFileSearchStore`, resumable
/// protocol).
///
/// The start request carries the document's settings
/// ([`UploadDocumentBody`]); the finalizing request carries the file and is
/// answered with an operation naming the created document, which this
/// fetches so callers get the shape the list and get endpoints return. New
/// documents start `Pending`; see
/// [`FileSearchDocuments::wait_until_active`](crate::FileSearchDocuments::wait_until_active).
pub async fn upload_to_file_search_store(
    ctx: &HttpContext,
    store_name: &str,
    source: UploadSource,
    display_name: Option<&str>,
    mime_type: &str,
    options: &DocumentOptions,
) -> Result<FileSearchDocument, GenaiError> {
    let store_path = store_resource_path(store_name)?;
    let url = ctx.upload_url(&format!("{store_path}:uploadToFileSearchStore"));
    let mime_type_value = mime_type_header(mime_type)?;

    // Without `displayName`, the API names the document after the
    // `X-Goog-Upload-File-Name` header, and leaves it unnamed when the header
    // is absent too (verified live 2026-09-27 on the raw protocol and
    // 2026-11-05 on the resumable one). A path sends its file name, as a
    // Files API path upload does; bytes send none, so an in-memory upload
    // without a display name stays unnamed, as in the Files API.
    let (body, file_size, file_name) = match source {
        UploadSource::Path(file_path) => {
            let read_error = |e: std::io::Error| {
                GenaiError::InvalidInput(format!("Failed to read {}: {e}", file_path.display()))
            };
            // Size the upload from the open handle, so it describes the file
            // sent; the body streams from disk.
            let file = tokio::fs::File::open(&file_path)
                .await
                .map_err(read_error)?;
            let size = file.metadata().await.map_err(read_error)?.len();
            check_upload_size(size)?;
            let file_name = file_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("upload");
            (
                file_body(file),
                size,
                Some(upload_file_name_header(file_name)),
            )
        }
        UploadSource::Bytes(data) => {
            let size = data.len() as u64;
            check_upload_size(size)?;
            (reqwest::Body::from(data), size, None)
        }
    };

    let metadata = UploadDocumentBody {
        display_name,
        mime_type,
        options,
    };
    let request_id = ctx.next_request_id();
    let wire_body = ctx.serialize_wire_body(&metadata);
    ctx.emit_request(request_id, "POST", &url, wire_body.as_ref());
    let upload_url = ResumableUpload {
        url,
        metadata: &metadata,
        file_size,
        mime_type: mime_type_value,
        file_name,
    }
    .start(ctx, request_id)
    .await?;
    let text = finish_resumable_upload(ctx, request_id, &upload_url, file_size, body).await?;
    ctx.emit_response_body(request_id, &text);

    // {"name": ".../upload/operations/...",
    //  "response": {"documentName": "...", "mimeType": ..., "sizeBytes": ...}}
    let operation: Operation =
        deserialize_with_context(&text, "uploadToFileSearchStore operation")?;
    let document_name = operation.document_name(&store_path).ok_or_else(|| {
        // Every observed upload came back resolved. The raw response
        // carries the operation `name`, the only handle left to a caller.
        GenaiError::MalformedResponse(format!(
            "Upload operation did not report a documentName (the operation \
             may be unresolved). Raw response: {text}"
        ))
    })?;
    read_back(ctx, &document_name, "Upload succeeded and created").await
}

/// Imports a Files API file into a store (`POST /v1beta/{store}:importFile`)
/// and waits for the operation it answers with to name the document, then
/// reads the document back.
///
/// The first answer is an unfinished operation with no document; `GET` on
/// the operation's name reports `done` and the document's ID, bare (live
/// 2026-11-05).
pub async fn import_file(
    ctx: &HttpContext,
    store_name: &str,
    import: &DocumentImport,
) -> Result<FileSearchDocument, GenaiError> {
    let store_path = store_resource_path(store_name)?;
    let url = ctx.api_url(&format!("{store_path}:importFile"));
    let body = ImportFileBody {
        file_name: &import.file_name,
        options: &import.options,
    };
    tracing::debug!("Importing {} into {store_name}", import.file_name);
    let text = send_and_read(ctx, reqwest::Method::POST, &url, Some(&body)).await?;
    let operation: Operation = deserialize_with_context(&text, "importFile operation")?;

    let what = format!("the import of '{}' into '{store_name}'", import.file_name);
    let operation = if operation.is_finished(&store_path) {
        operation
    } else {
        let operation_url = operation_url(ctx, &store_path, &operation.name)?;
        import
            .poll
            .wait(
                &what,
                (IMPORT_WAIT_TIMEOUT, IMPORT_POLL_INTERVAL),
                || get_operation(ctx, &operation_url),
                |current| {
                    if current.is_finished(&store_path) {
                        Ok(PollStep::Ready(current))
                    } else {
                        Ok(PollStep::Pending("running".to_string()))
                    }
                },
            )
            .await?
    };
    if let Some(error) = &operation.error {
        return Err(GenaiError::Internal(format!(
            "{what} failed (operation '{}'): {error}",
            operation.name
        )));
    }
    let document_name = operation.document_name(&store_path).ok_or_else(|| {
        GenaiError::MalformedResponse(format!(
            "The import operation '{}' finished without naming a document",
            operation.name
        ))
    })?;
    read_back(ctx, &document_name, "Import succeeded and created").await
}

/// How long [`import_file`] waits for its operation by default.
const IMPORT_WAIT_TIMEOUT: Duration = Duration::from_secs(60);
/// How often [`import_file`] polls its operation by default.
const IMPORT_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// A long-running operation, as an upload or an import answers with it.
#[derive(Debug, Deserialize)]
struct Operation {
    #[serde(default)]
    name: String,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    response: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<FileError>,
}

impl Operation {
    /// Finished: done, failed, or already naming its document.
    fn is_finished(&self, store_path: &str) -> bool {
        self.done || self.error.is_some() || self.document_name(store_path).is_some()
    }

    /// The full name of the document the operation reports. An upload
    /// reports the full name and an import the bare ID (live 2026-11-05).
    fn document_name(&self, store_path: &str) -> Option<String> {
        let name = self.response.as_ref()?.get("documentName")?.as_str()?;
        if name.is_empty() {
            return None;
        }
        Some(if name.contains('/') {
            name.to_string()
        } else {
            format!("{store_path}/documents/{name}")
        })
    }
}

/// The URL of an operation of the store at `store_path`, from the name the
/// API gave it, with its ID percent-encoded.
fn operation_url(ctx: &HttpContext, store_path: &str, name: &str) -> Result<String, GenaiError> {
    let id = name
        .strip_prefix(store_path)
        .and_then(|rest| rest.strip_prefix("/operations/"))
        .filter(|id| !id.is_empty() && !id.contains('/') && require_id(id, "operation").is_ok())
        .ok_or_else(|| {
            GenaiError::MalformedResponse(format!(
                "The import operation is unfinished, and its name {name:?} is not \
                 an operation of '{store_path}'"
            ))
        })?;
    Ok(ctx.api_url(&format!("{store_path}/operations/{}", path_segment(id))))
}

async fn get_operation(ctx: &HttpContext, url: &str) -> Result<Operation, GenaiError> {
    let text = send_and_read(ctx, reqwest::Method::GET, url, NO_BODY).await?;
    deserialize_with_context(&text, "file search store operation")
}

/// Reads back the document an upload or import created. Its bytes are
/// already accepted, so a failed read must still hand the caller the
/// document name, and must not turn a retryable error into a non-retryable
/// one.
async fn read_back(
    ctx: &HttpContext,
    document_name: &str,
    what: &str,
) -> Result<FileSearchDocument, GenaiError> {
    get_document(ctx, document_name).await.map_err(|e| {
        tracing::error!("{what} '{document_name}', but reading it back failed: {e}");
        let context = format!(
            "{what} '{document_name}', but reading it back failed. The \
             document exists — use that name to wait on or delete it. Cause: "
        );
        match e {
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
            other if other.is_retryable() => other,
            other => GenaiError::Internal(format!("{context}{other}")),
        }
    })
}

/// The `X-Goog-Upload-File-Name` value for a filename.
///
/// A filename with a control character (legal on Linux) cannot ride in a
/// header. The header is only a fallback display name, so it degrades to
/// `"upload"` with a warning instead of failing the upload. Non-ASCII is fine.
fn upload_file_name_header(file_name: &str) -> HeaderValue {
    HeaderValue::try_from(file_name).unwrap_or_else(|_| {
        tracing::warn!(
            file_name,
            "filename cannot ride in a header (control character); sending \"upload\" instead"
        );
        HeaderValue::from_static("upload")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> HttpContext {
        HttpContext::new(reqwest::Client::new(), "k".to_string(), vec![])
    }

    #[test]
    fn upload_file_name_header_passes_non_ascii_through() {
        // Only control characters degrade. Compared as bytes because
        // `HeaderValue::to_str` refuses obs-text.
        for name in ["resume.txt", "résumé.txt", "文件.txt", "a b.txt"] {
            assert_eq!(
                upload_file_name_header(name).as_bytes(),
                name.as_bytes(),
                "{name} should ride in the header unchanged"
            );
        }
    }

    #[test]
    fn upload_file_name_header_degrades_on_control_characters() {
        for name in ["a\nb.txt", "a\rb.txt", "a\0b.txt"] {
            assert_eq!(upload_file_name_header(name).as_bytes(), b"upload");
        }
    }

    #[test]
    fn stores_url_construction() {
        assert_eq!(
            stores_url(&ctx()),
            "https://generativelanguage.googleapis.com/v1beta/fileSearchStores"
        );
        assert_eq!(
            store_url(&ctx(), "fileSearchStores/abc123").unwrap(),
            "https://generativelanguage.googleapis.com/v1beta/fileSearchStores/abc123"
        );
        assert_eq!(
            documents_url(&ctx(), "fileSearchStores/abc123").unwrap(),
            "https://generativelanguage.googleapis.com/v1beta/fileSearchStores/abc123/documents"
        );
    }

    #[test]
    fn document_url_construction() {
        assert_eq!(
            document_url(&ctx(), "fileSearchStores/abc123/documents/doc1").unwrap(),
            "https://generativelanguage.googleapis.com/v1beta/fileSearchStores/abc123/documents/doc1"
        );
    }

    #[test]
    fn store_name_must_be_a_full_resource_name() {
        // A bare ID would address the collection URL, not a store.
        let err = store_url(&ctx(), "abc123").unwrap_err();
        assert!(matches!(err, GenaiError::InvalidInput(_)));
        assert!(err.to_string().contains("fileSearchStores/<id>"));
    }

    #[test]
    fn store_name_rejects_extra_segments() {
        let err = store_url(&ctx(), "fileSearchStores/abc/extra").unwrap_err();
        assert!(matches!(err, GenaiError::InvalidInput(_)));
    }

    #[test]
    fn store_name_rejects_empty_and_dot_segments() {
        assert!(store_url(&ctx(), "fileSearchStores/").is_err());
        assert!(store_url(&ctx(), "fileSearchStores/..").is_err());
        assert!(store_url(&ctx(), "fileSearchStores/%2e%2e").is_err());
    }

    #[test]
    fn store_id_metacharacters_are_encoded_not_interpolated() {
        assert_eq!(
            store_url(&ctx(), "fileSearchStores/a?b#c").unwrap(),
            "https://generativelanguage.googleapis.com/v1beta/fileSearchStores/a%3Fb%23c"
        );
    }

    #[test]
    fn document_name_requires_documents_segment() {
        let err = document_url(&ctx(), "fileSearchStores/abc123").unwrap_err();
        assert!(err.to_string().contains("/documents/"));
    }

    #[test]
    fn document_name_must_start_with_store_prefix() {
        let err = document_url(&ctx(), "documents/doc1").unwrap_err();
        assert!(matches!(err, GenaiError::InvalidInput(_)));
    }

    #[test]
    fn document_ids_are_encoded() {
        assert_eq!(
            document_url(&ctx(), "fileSearchStores/a b/documents/c d").unwrap(),
            "https://generativelanguage.googleapis.com/v1beta/fileSearchStores/a%20b/documents/c%20d"
        );
    }
}
