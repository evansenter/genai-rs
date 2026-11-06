//! The paging engine behind every list builder's `.pages()` and `.items()`.
//!
//! A list builder (for example [`ListAgents`](crate::ListAgents)) hands
//! [`pages`] a `fetch` closure that sends one list request for a given page
//! token. The engine calls it lazily, once per page, and follows
//! `next_page_token` until the list ends:
//!
//! - the stream ends after a page whose token is `None` or `Some("")`;
//! - an empty page with a non-empty token is followed;
//! - a token already requested (including the starting one) yields its page,
//!   then [`GenaiError::MalformedResponse`], then ends, rather than looping
//!   forever on a misbehaving server;
//! - an error is yielded once and ends the stream (no retries).
//!
//! The streams are `'static`: builders move a clone of the
//! [`Client`](crate::Client) and their query state into `fetch`, so a stream
//! can be stored or spawned (D-016).

use std::collections::HashSet;
use std::future::Future;

use futures_util::stream::{self, BoxStream, StreamExt, TryStreamExt};

use crate::errors::GenaiError;

/// One page of a list response: its items and the token for the next page.
pub(crate) trait ListPage: Send + 'static {
    /// The resource type the page lists.
    type Item: Send + 'static;

    /// The token for the next page, as the server sent it.
    fn next_page_token(&self) -> Option<&str>;

    /// The page's items, in server order.
    fn into_items(self) -> Vec<Self::Item>;
}

/// Implements [`ListPage`] for a list response with a
/// `next_page_token: Option<String>` field and a `Vec` of items:
/// `impl_list_page!(AgentListResponse, agents: Agent);`
macro_rules! impl_list_page {
    ($resp:ty, $field:ident: $item:ty) => {
        impl $crate::paging::ListPage for $resp {
            type Item = $item;

            fn next_page_token(&self) -> Option<&str> {
                self.next_page_token.as_deref()
            }

            fn into_items(self) -> Vec<Self::Item> {
                self.$field
            }
        }
    };
}
pub(crate) use impl_list_page;

/// Defines a list builder (D-016): the struct, a crate-private `new`,
/// `with_page_size`, `with_page_token`, and the three terminals `send`,
/// `pages` and `items`. One definition of the paging rules for every
/// resource (#498).
///
/// The caller gives the struct its docs and resource fields, each method its
/// docs, the page and item types, the label `pages` logs and errors with,
/// and how one page is fetched: `fetch: |http, page_size, token| expr`,
/// where `expr` is a future and may name the resource fields as owned
/// values. Resource-specific setters stay in an `impl` of their own.
macro_rules! list_builder {
    (
        $(#[$meta:meta])*
        pub struct $name:ident<'a> {
            $($field:ident: $fty:ty),* $(,)?
        }
        page: $page:ty => $item:ty;
        label: $label:literal;
        $(#[$page_size_doc:meta])+
        fn with_page_size;
        $(#[$page_token_doc:meta])+
        fn with_page_token;
        $(#[$send_doc:meta])+
        fn send;
        $(#[$items_doc:meta])+
        fn items;
        fetch: |$http:ident, $page_size:ident, $token:ident| $fetch:expr $(,)?
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug)]
        #[must_use = "a list request does nothing until .send(), .pages() or .items()"]
        pub struct $name<'a> {
            pub(crate) client: &'a $crate::Client,
            $(pub(crate) $field: $fty,)*
            pub(crate) page_size: Option<u32>,
            pub(crate) page_token: Option<String>,
        }

        impl<'a> $name<'a> {
            pub(crate) fn new(client: &'a $crate::Client $(, $field: $fty)*) -> Self {
                Self {
                    client,
                    $($field,)*
                    page_size: None,
                    page_token: None,
                }
            }

            $(#[$page_size_doc])+
            pub fn with_page_size(mut self, page_size: u32) -> Self {
                self.page_size = Some(page_size);
                self
            }

            $(#[$page_token_doc])+
            pub fn with_page_token(mut self, page_token: impl Into<String>) -> Self {
                self.page_token = Some(page_token.into());
                self
            }

            $(#[$send_doc])+
            pub async fn send(self) -> Result<$page, $crate::GenaiError> {
                let Self {
                    client,
                    $($field,)*
                    page_size: $page_size,
                    page_token: $token,
                } = self;
                let $http = &client.http;
                $fetch.await
            }

            /// Streams every page, starting at
            /// [`with_page_token`](Self::with_page_token) or the first page.
            ///
            /// Nothing is sent until the stream is polled. It ends after a
            /// page without a `next_page_token`; an error is yielded once and
            /// ends it. A page whose token was already requested (the
            /// starting token included) is yielded, then
            /// [`GenaiError::MalformedResponse`](crate::GenaiError::MalformedResponse).
            /// The stream owns a clone of the client, so it can be stored or
            /// spawned.
            #[must_use = "streams do nothing unless polled"]
            pub fn pages(
                self,
            ) -> ::futures_util::stream::BoxStream<'static, Result<$page, $crate::GenaiError>> {
                let Self {
                    client,
                    $($field,)*
                    page_size: $page_size,
                    page_token,
                } = self;
                let client = client.clone();
                $crate::paging::pages($label, page_token, move |$token| {
                    let client = client.clone();
                    $(#[allow(clippy::clone_on_copy)] let $field = $field.clone();)*
                    async move {
                        let $http = &client.http;
                        $fetch.await
                    }
                })
            }

            $(#[$items_doc])+
            #[must_use = "streams do nothing unless polled"]
            pub fn items(
                self,
            ) -> ::futures_util::stream::BoxStream<'static, Result<$item, $crate::GenaiError>> {
                $crate::paging::items(self.pages())
            }
        }
    };
}
pub(crate) use list_builder;

/// Streams every page of a list, starting at `first_token`.
///
/// `label` names the list in logs and errors. `fetch` sends one request for
/// the given page token; nothing is sent until the stream is first polled.
pub(crate) fn pages<P, F, Fut>(
    label: &'static str,
    first_token: Option<String>,
    mut fetch: F,
) -> BoxStream<'static, Result<P, GenaiError>>
where
    P: ListPage,
    F: FnMut(Option<String>) -> Fut + Send + 'static,
    Fut: Future<Output = Result<P, GenaiError>> + Send + 'static,
{
    Box::pin(async_stream::try_stream! {
        // Every token requested so far: a server that hands one back again
        // would otherwise be followed forever.
        let mut seen: HashSet<String> = first_token.iter().cloned().collect();
        let mut token = first_token;
        for page_index in 0usize.. {
            let page = fetch(token.take()).await?;
            let next = page
                .next_page_token()
                .filter(|t| !t.is_empty())
                .map(str::to_owned);
            tracing::debug!(
                list = label,
                page_index,
                has_next = next.is_some(),
                "Fetched list page"
            );
            yield page;
            match next {
                None => break,
                Some(t) if !seen.insert(t.clone()) => {
                    Err::<(), _>(GenaiError::MalformedResponse(format!(
                        "{label}: server returned page token {t:?} twice"
                    )))?;
                }
                Some(t) => token = Some(t),
            }
        }
    })
}

/// Flattens a page stream into its items, in order. An error ends the
/// stream after the items of the pages before it.
pub(crate) fn items<P: ListPage>(
    pages: BoxStream<'static, Result<P, GenaiError>>,
) -> BoxStream<'static, Result<P::Item, GenaiError>> {
    pages
        .map_ok(|page| stream::iter(page.into_items().into_iter().map(Ok)))
        .try_flatten()
        .boxed()
}

#[cfg(test)]
#[path = "paging_tests.rs"]
mod tests;
