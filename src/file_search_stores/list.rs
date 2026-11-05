//! The list builders: [`ListFileSearchStores`] and
//! [`ListFileSearchDocuments`].

use super::types::{
    DocumentListResponse, FileSearchDocument, FileSearchStore, FileSearchStoreListResponse,
};
use crate::client::Client;
use crate::errors::GenaiError;
use crate::paging;
use futures_util::stream::BoxStream;

/// A `GET /v1beta/fileSearchStores` request, from
/// [`FileSearchStores::list`](super::FileSearchStores::list): your file
/// search stores, oldest first.
///
/// End it with [`send`](Self::send) for one page, or
/// [`pages`](Self::pages) / [`items`](Self::items) to follow
/// `nextPageToken` to the end of the list. The page size is sent with every
/// page.
///
/// # Example
///
/// ```no_run
/// use futures_util::TryStreamExt;
/// use genai_rs::FileSearchStore;
///
/// # async fn example(client: genai_rs::Client) -> Result<(), genai_rs::GenaiError> {
/// // One page
/// let page = client.file_search_stores().list().with_page_size(10).send().await?;
/// for store in &page.stores {
///     println!("{}: {:?}", store.name, store.display_name);
/// }
///
/// // Every store, across pages
/// let all: Vec<FileSearchStore> = client.file_search_stores().list().items().try_collect().await?;
/// # let _ = all;
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
#[must_use = "a list request does nothing until .send(), .pages() or .items()"]
pub struct ListFileSearchStores<'a> {
    pub(super) client: &'a Client,
    pub(super) page_size: Option<u32>,
    pub(super) page_token: Option<String>,
}

impl<'a> ListFileSearchStores<'a> {
    /// Sets the maximum number of stores per page, from 1 to 20 (a larger
    /// value is a 400, verified live 2026-09-27). Sent with every page.
    pub fn with_page_size(mut self, page_size: u32) -> Self {
        self.page_size = Some(page_size);
        self
    }

    /// Starts from this page token, from a previous page's
    /// `next_page_token`.
    pub fn with_page_token(mut self, page_token: impl Into<String>) -> Self {
        self.page_token = Some(page_token.into());
        self
    }

    /// Sends the request and returns one page.
    ///
    /// With no stores, the page is empty and has no `next_page_token`.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn send(self) -> Result<FileSearchStoreListResponse, GenaiError> {
        crate::http::file_search_stores::list_file_search_stores(
            &self.client.http,
            self.page_size,
            self.page_token.as_deref(),
        )
        .await
    }

    /// Streams every page, starting at [`with_page_token`](Self::with_page_token)
    /// or the first page.
    ///
    /// Nothing is sent until the stream is polled. It ends after a page
    /// without a `next_page_token`; an error is yielded once and ends it. A
    /// page whose token was already requested (the starting token included)
    /// is yielded, then [`GenaiError::MalformedResponse`]. The stream owns a
    /// clone of the client, so it can be stored or spawned.
    #[must_use = "streams do nothing unless polled"]
    pub fn pages(self) -> BoxStream<'static, Result<FileSearchStoreListResponse, GenaiError>> {
        let Self {
            client,
            page_size,
            page_token,
        } = self;
        let client = client.clone();
        paging::pages("file search stores", page_token, move |token| {
            let client = client.clone();
            async move {
                crate::http::file_search_stores::list_file_search_stores(
                    &client.http,
                    page_size,
                    token.as_deref(),
                )
                .await
            }
        })
    }

    /// Streams every store across pages, oldest first. Same rules as
    /// [`pages`](Self::pages).
    #[must_use = "streams do nothing unless polled"]
    pub fn items(self) -> BoxStream<'static, Result<FileSearchStore, GenaiError>> {
        paging::items(self.pages())
    }
}

/// A `GET /v1beta/fileSearchStores/{store}/documents` request, from
/// [`FileSearchDocuments::list`](super::FileSearchDocuments::list): the
/// documents in one store, oldest first.
///
/// End it with [`send`](Self::send) for one page, or
/// [`pages`](Self::pages) / [`items`](Self::items) to follow
/// `nextPageToken` to the end of the list. The page size is sent with every
/// page. A store name that is not `fileSearchStores/<id>` fails with
/// [`GenaiError::InvalidInput`] before any request (from `.send()`, or as
/// the streams' only item).
///
/// # Example
///
/// ```no_run
/// use futures_util::TryStreamExt;
/// use genai_rs::FileSearchDocument;
///
/// # async fn example(client: genai_rs::Client, store_name: &str) -> Result<(), genai_rs::GenaiError> {
/// // One page
/// let page = client
///     .file_search_stores()
///     .documents()
///     .list(store_name)
///     .with_page_size(10)
///     .send()
///     .await?;
/// for document in &page.documents {
///     println!("{}: {:?}", document.name, document.state);
/// }
///
/// // Every document, across pages
/// let all: Vec<FileSearchDocument> = client
///     .file_search_stores()
///     .documents()
///     .list(store_name)
///     .items()
///     .try_collect()
///     .await?;
/// # let _ = all;
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
#[must_use = "a list request does nothing until .send(), .pages() or .items()"]
pub struct ListFileSearchDocuments<'a> {
    pub(super) client: &'a Client,
    pub(super) store_name: String,
    pub(super) page_size: Option<u32>,
    pub(super) page_token: Option<String>,
}

impl<'a> ListFileSearchDocuments<'a> {
    /// Sets the maximum number of documents per page, from 1 to 20 (a larger
    /// value is a 400, verified live 2026-09-27). Sent with every page.
    pub fn with_page_size(mut self, page_size: u32) -> Self {
        self.page_size = Some(page_size);
        self
    }

    /// Starts from this page token, from a previous page's
    /// `next_page_token` for the same store (another store's token lists
    /// nothing, verified live 2026-09-27).
    pub fn with_page_token(mut self, page_token: impl Into<String>) -> Self {
        self.page_token = Some(page_token.into());
        self
    }

    /// Sends the request and returns one page.
    ///
    /// An empty store's page is empty and has no `next_page_token`.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a malformed store name, and
    /// an API or network error if the request fails or response parsing
    /// fails.
    pub async fn send(self) -> Result<DocumentListResponse, GenaiError> {
        crate::http::file_search_stores::list_documents(
            &self.client.http,
            &self.store_name,
            self.page_size,
            self.page_token.as_deref(),
        )
        .await
    }

    /// Streams every page, starting at [`with_page_token`](Self::with_page_token)
    /// or the first page.
    ///
    /// Nothing is sent until the stream is polled. It ends after a page
    /// without a `next_page_token`; an error is yielded once and ends it. A
    /// page whose token was already requested (the starting token included)
    /// is yielded, then [`GenaiError::MalformedResponse`]. The stream owns a
    /// clone of the client, so it can be stored or spawned.
    #[must_use = "streams do nothing unless polled"]
    pub fn pages(self) -> BoxStream<'static, Result<DocumentListResponse, GenaiError>> {
        let Self {
            client,
            store_name,
            page_size,
            page_token,
        } = self;
        let client = client.clone();
        paging::pages("file search documents", page_token, move |token| {
            let (client, store_name) = (client.clone(), store_name.clone());
            async move {
                crate::http::file_search_stores::list_documents(
                    &client.http,
                    &store_name,
                    page_size,
                    token.as_deref(),
                )
                .await
            }
        })
    }

    /// Streams every document across pages, oldest first. Same rules as
    /// [`pages`](Self::pages).
    #[must_use = "streams do nothing unless polled"]
    pub fn items(self) -> BoxStream<'static, Result<FileSearchDocument, GenaiError>> {
        paging::items(self.pages())
    }
}

#[cfg(test)]
#[path = "list_tests.rs"]
mod tests;
