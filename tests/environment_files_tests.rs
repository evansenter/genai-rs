//! Live tests for environment files and environment forking.
//!
//! ```bash
//! cargo test --test environment_files_tests -- --include-ignored --nocapture
//! ```

mod common;

use common::get_client;
use futures_util::FutureExt;
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
        match client.delete_environment(&id).await {
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
                .create_environment(
                    &CreateEnvironmentRequest::new()
                        .add_source(EnvironmentSource::inline("/etc/motd", "hello")),
                )
                .await
                .expect("create_environment failed");
            let env_id = env.id.clone().expect("environment has no id");
            ids.lock().unwrap().push(env_id.clone());

            let content = b"hello from genai-rs\n".to_vec();
            let written = client
                .upload_environment_file(
                    &env_id,
                    "genai-rs/hello.txt",
                    content.clone(),
                    "text/plain",
                    EnvironmentFileUpload {
                        overwrite: true,
                        ..Default::default()
                    },
                )
                .await
                .expect("upload failed");
            let file = written.files.first().expect("upload listed no file");
            assert_eq!(file.path.as_deref(), Some("genai-rs/hello.txt"));
            assert_eq!(file.file_type, Some(EnvironmentFileType::File));
            assert_eq!(file.size_bytes, Some(content.len() as i64));

            let root = client
                .list_environment_files(&env_id, "", true, None, None)
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
                .list_environment_files(&env_id, "genai-rs/hello.txt", false, None, None)
                .await
                .expect("single-file listing failed");
            assert_eq!(single.files.len(), 1);

            let missing = client
                .list_environment_files(&env_id, "no/such/path", false, None, None)
                .await;
            assert!(matches!(
                missing,
                Err(GenaiError::Api {
                    status_code: 404,
                    ..
                })
            ));

            let fork = client
                .create_environment(&CreateEnvironmentRequest::from_environment(&env_id))
                .await
                .expect("fork failed");
            let fork_id = fork.id.clone().expect("fork has no id");
            ids.lock().unwrap().push(fork_id.clone());
            assert_ne!(fork_id, env_id);

            let forked = client
                .list_environment_files(&fork_id, "genai-rs/hello.txt", false, None, None)
                .await
                .expect("fork does not contain the uploaded file");
            assert_eq!(forked.files.len(), 1);
        }
    })
    .await;
}
