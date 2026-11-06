//! The list builders: [`ListFileSearchStores`] and
//! [`ListFileSearchDocuments`].

use super::types::{
    FileSearchDocument, FileSearchDocumentListResponse, FileSearchStore,
    FileSearchStoreListResponse,
};
use crate::paging;

paging::list_builder! {
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
    pub struct ListFileSearchStores<'a> {
    }
    page: FileSearchStoreListResponse => FileSearchStore;
    label: "file search stores";

    /// Sets the maximum number of stores per page, from 1 to 20 (a larger
    /// value is a 400, verified live 2026-09-27). Sent with every page.
    fn with_page_size;

    /// Starts from this page token, from a previous page's
    /// `next_page_token`.
    fn with_page_token;

    /// Sends the request and returns one page.
    ///
    /// With no stores, the page is empty and has no `next_page_token`.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    fn send;

    /// Streams every store across pages, oldest first. Same rules as
    /// [`pages`](Self::pages).
    fn items;

    fetch: |http, page_size, token| crate::http::file_search_stores::list_file_search_stores(
        http,
        page_size,
        token.as_deref(),
    ),
}

paging::list_builder! {
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
    pub struct ListFileSearchDocuments<'a> {
        store_name: String,
    }
    page: FileSearchDocumentListResponse => FileSearchDocument;
    label: "file search documents";

    /// Sets the maximum number of documents per page, from 1 to 20 (a larger
    /// value is a 400, verified live 2026-09-27). Sent with every page.
    fn with_page_size;

    /// Starts from this page token, from a previous page's
    /// `next_page_token` for the same store (another store's token lists
    /// nothing, verified live 2026-09-27).
    fn with_page_token;

    /// Sends the request and returns one page.
    ///
    /// An empty store's page is empty and has no `next_page_token`.
    ///
    /// # Errors
    ///
    /// Returns [`GenaiError::InvalidInput`] for a malformed store name, and
    /// an API or network error if the request fails or response parsing
    /// fails.
    fn send;

    /// Streams every document across pages, oldest first. Same rules as
    /// [`pages`](Self::pages).
    fn items;

    fetch: |http, page_size, token| crate::http::file_search_stores::list_documents(
        http,
        &store_name,
        page_size,
        token.as_deref(),
    ),
}

#[cfg(test)]
#[path = "list_tests.rs"]
mod tests;
