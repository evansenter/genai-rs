//! Live tests for environment files and environment forking.
//!
//! ```bash
//! cargo test --test environment_files_tests -- --include-ignored --nocapture
//! ```

mod common;

use common::get_client;
use futures_util::{FutureExt, StreamExt, TryStreamExt};
use genai_rs::{
    Client, CreateEnvironmentRequest, EnvironmentFileType, EnvironmentFileUpload,
    EnvironmentSource, GenaiError,
};
use std::panic::AssertUnwindSafe;

/// Deletes every environment in `ids` after `body`, including when it
/// panics. `body` records forks it creates by pushing onto the vector.
async fn with_environments<F, Fut>(client: &Client, body: F)
where
    F: FnOnce(std::sync::Arc<std::sync::Mutex<Vec<String>>>) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let ids = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let outcome = AssertUnwindSafe(body(ids.clone())).catch_unwind().await;
    let created: Vec<String> = ids.lock().expect("id list poisoned").clone();
    for id in created {
        match client.environments().delete(&id).await {
            Ok(())
            | Err(GenaiError::Api {
                status_code: 404, ..
            }) => {}
            Err(e) => eprintln!("cleanup failed for environment {id}: {e:?}"),
        }
    }
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_upload_list_and_fork() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    with_environments(&client, |ids| {
        let client = client.clone();
        async move {
            let env = client
                .environments()
                .create(
                    &CreateEnvironmentRequest::new()
                        .add_source(EnvironmentSource::inline("/etc/motd", "hello")),
                )
                .await
                .expect("environments.create failed");
            let env_id = env.id.clone().expect("environment has no id");
            ids.lock().unwrap().push(env_id.clone());

            let content = b"hello from genai-rs\n".to_vec();
            let written = client
                .environments()
                .files()
                .upload(
                    &env_id,
                    "genai-rs/hello.txt",
                    EnvironmentFileUpload::new(content.clone(), "text/plain").with_overwrite(true),
                )
                .await
                .expect("upload failed");
            let file = written.files.first().expect("upload listed no file");
            assert_eq!(file.path.as_deref(), Some("genai-rs/hello.txt"));
            assert_eq!(file.file_type, Some(EnvironmentFileType::File));
            assert_eq!(file.size_bytes, Some(content.len() as i64));

            // Without `with_overwrite`, writing over it is a 409 (from the
            // finalizing request, live 2026-09-26).
            let conflict = client
                .environments()
                .files()
                .upload(
                    &env_id,
                    "genai-rs/hello.txt",
                    EnvironmentFileUpload::new(content.clone(), "text/plain"),
                )
                .await;
            assert!(
                matches!(
                    conflict,
                    Err(GenaiError::Api {
                        status_code: 409,
                        ..
                    })
                ),
                "{conflict:?}"
            );

            let root = client
                .environments()
                .files()
                .list(&env_id, "")
                .with_recursive(true)
                .send()
                .await
                .expect("recursive root listing failed");
            assert!(
                root.files
                    .iter()
                    .any(|f| f.file_type == Some(EnvironmentFileType::Directory)
                        && f.path.as_deref() == Some("genai-rs")),
                "directory missing from {:?}",
                root.files
            );

            let single = client
                .environments()
                .files()
                .list(&env_id, "genai-rs/hello.txt")
                .send()
                .await
                .expect("single-file listing failed");
            assert_eq!(single.files.len(), 1);

            let missing = client
                .environments()
                .files()
                .list(&env_id, "no/such/path")
                .send()
                .await;
            assert!(matches!(
                missing,
                Err(GenaiError::Api {
                    status_code: 404,
                    ..
                })
            ));

            let fork = client
                .environments()
                .create(&CreateEnvironmentRequest::from_environment(&env_id))
                .await
                .expect("fork failed");
            let fork_id = fork.id.clone().expect("fork has no id");
            ids.lock().unwrap().push(fork_id.clone());
            assert_ne!(fork_id, env_id);

            let forked = client
                .environments()
                .files()
                .list(&fork_id, "genai-rs/hello.txt")
                .send()
                .await
                .expect("fork does not contain the uploaded file");
            assert_eq!(forked.files.len(), 1);
        }
    })
    .await;
}

#[tokio::test]
#[ignore = "Requires API key"]
async fn test_file_list_streams_follow_every_page() {
    let Some(client) = get_client() else {
        println!("Skipping: GEMINI_API_KEY not set");
        return;
    };

    with_environments(&client, |ids| {
        let client = client.clone();
        async move {
            // A directory and three files: four entries under a recursive
            // root listing.
            let request = ["a", "b", "c"].into_iter().fold(
                CreateEnvironmentRequest::new(),
                |request, name| {
                    request.add_source(EnvironmentSource::inline(
                        format!("/paging/{name}.txt"),
                        name,
                    ))
                },
            );
            let env = client
                .environments()
                .create(&request)
                .await
                .expect("environments.create failed");
            let env_id = env.id.clone().expect("environment has no id");
            ids.lock().unwrap().push(env_id.clone());

            let whole = client
                .environments()
                .files()
                .list(&env_id, "")
                .with_recursive(true)
                .send()
                .await
                .expect("one-page listing failed");
            let expected: Vec<Option<String>> =
                whole.files.iter().map(|f| f.path.clone()).collect();
            println!("One page: {expected:?}");
            assert!(
                expected.len() >= 4,
                "the sources are missing from {expected:?}"
            );
            assert!(
                whole.next_page_token.is_none(),
                "{:?}",
                whole.next_page_token
            );

            // One entry per page. `recursive` must be resent with every page
            // token: live, a token without it lists something else (`{}`).
            let pages: Vec<_> = client
                .environments()
                .files()
                .list(&env_id, "")
                .with_recursive(true)
                .with_page_size(1)
                .pages()
                .take(50)
                .try_collect()
                .await
                .expect("paged listing failed");
            println!("Listed {} page(s)", pages.len());
            assert!(
                pages.iter().all(|p| p.files.len() <= 1),
                "page_size=1 must hold on every page"
            );
            let paged: Vec<Option<String>> = pages
                .iter()
                .flat_map(|p| &p.files)
                .map(|f| f.path.clone())
                .collect();
            assert_eq!(paged, expected);
            let last = pages.last().expect("at least one page");
            assert!(last.next_page_token.is_none(), "{:?}", last.next_page_token);

            let items: Vec<_> = client
                .environments()
                .files()
                .list(&env_id, "")
                .with_recursive(true)
                .with_page_size(2)
                .items()
                .take(50)
                .try_collect()
                .await
                .expect("streamed listing failed");
            let streamed: Vec<Option<String>> = items.into_iter().map(|f| f.path).collect();
            assert_eq!(streamed, expected);
        }
    })
    .await;
}
