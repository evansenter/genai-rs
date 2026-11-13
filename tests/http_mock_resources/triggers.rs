//! Triggers and their executions: every endpoint's request, the responses,
//! and list paging.

use super::*;

#[cfg(not(genai_strict_unknown))]
#[tokio::test]
async fn trigger_endpoints_send_the_documented_requests() {
    let stub = ok_stub().await;
    let client = stub.client();
    let c = &client;

    let interaction = nightly_interaction();
    let mut params = CreateTriggerRequest::new("0 5 * * *", "Europe/Paris", interaction.clone())
        .with_display_name("nightly")
        .with_environment_id("env-1")
        .with_max_consecutive_failures(3)
        .with_execution_timeout_seconds(600);
    params.extra.insert("labels".into(), json!({"team": "ops"}));
    let mut unknown_update = TriggerUpdate::new()
        .with_status(serde_json::from_value::<TriggerStatus>(json!("archived")).unwrap());
    unknown_update
        .extra
        .insert("max_consecutive_failures".into(), json!("5"));

    assert_wire(
        &stub,
        vec![
            wire(
                "POST",
                "/v1beta/triggers",
                Some(json!({
                    "schedule": "0 5 * * *",
                    "time_zone": "Europe/Paris",
                    "interaction": serde_json::to_value(&interaction).unwrap(),
                    "display_name": "nightly",
                    "environment_id": "env-1",
                    "max_consecutive_failures": 3,
                    "execution_timeout_seconds": 600,
                    "labels": {"team": "ops"}
                })),
                async move { c.triggers().create(&params).await },
            ),
            wire("GET", "/v1beta/triggers/t-1", None, c.triggers().get("t-1")),
            wire("GET", "/v1beta/triggers", None, c.triggers().list().send()),
            wire(
                "GET",
                "/v1beta/triggers?page_size=10&page_token=p2",
                None,
                c.triggers()
                    .list()
                    .with_page_size(10)
                    .with_page_token("p2")
                    .send(),
            ),
            // No update_mask: the spec defines none for triggers.
            wire(
                "PATCH",
                "/v1beta/triggers/t-1",
                Some(json!({"display_name": "renamed", "status": "paused"})),
                async move {
                    let update = TriggerUpdate::new()
                        .with_display_name("renamed")
                        .with_status(TriggerStatus::Paused);
                    c.triggers().update("t-1", &update).await
                },
            ),
            wire(
                "PATCH",
                "/v1beta/triggers/t-1",
                Some(json!({"status": "archived", "max_consecutive_failures": "5"})),
                async move { c.triggers().update("t-1", &unknown_update).await },
            ),
            wire(
                "DELETE",
                "/v1beta/triggers/t-1",
                None,
                c.triggers().delete("t-1"),
            ),
            wire(
                "POST",
                "/v1beta/triggers/t-1/executions",
                Some(json!({})),
                c.triggers().run("t-1"),
            ),
            wire(
                "GET",
                "/v1beta/triggers/t-1/executions",
                None,
                c.triggers().list_executions("t-1").send(),
            ),
            wire(
                "GET",
                "/v1beta/triggers/t-1/executions?page_size=2&page_token=next",
                None,
                c.triggers()
                    .list_executions("t-1")
                    .with_page_size(2)
                    .with_page_token("next")
                    .send(),
            ),
        ],
    )
    .await;
}

#[tokio::test]
async fn trigger_response_parses_string_counts_timestamp_aliases_and_sparse_interaction() {
    let stub = Stub::replying(vec![Reply::json(
        200,
        json!({
            "id": "t-1",
            "schedule": "0 5 * * *",
            "time_zone": "Europe/Paris",
            "display_name": "nightly",
            "status": "active",
            // A sparse projection: no `input`.
            "interaction": {"agent": "agents/nightly"},
            "max_consecutive_failures": "3",
            "consecutive_failure_count": "1",
            "execution_timeout_seconds": "600",
            "created": "2026-09-01T05:00:00Z",
            "updated": "2026-09-02T05:00:00Z",
            "next_run_time": "2026-09-25T03:00:00Z",
            "owner": "users/42"
        }),
    )])
    .await;

    let trigger = stub.client().triggers().get("t-1").await.unwrap();

    assert_eq!(trigger.status, Some(TriggerStatus::Active));
    assert_eq!(trigger.max_consecutive_failures, Some(3));
    assert_eq!(trigger.consecutive_failure_count, Some(1));
    assert_eq!(trigger.execution_timeout_seconds, Some(600));
    assert!(
        trigger.create_time.is_some() && trigger.update_time.is_some(),
        "the `created`/`updated` spellings alias the timestamps"
    );
    assert!(trigger.next_run_time.is_some());
    let interaction = trigger.interaction.as_ref().unwrap();
    assert_eq!(interaction.agent.as_deref(), Some("agents/nightly"));
    assert!(interaction.input.is_none(), "{:?}", interaction.input);
    assert_eq!(trigger.extra["owner"], "users/42");
    // Counts go back out as plain numbers, the trigger API's own wire form
    // (live 2026-11-01), whichever form came in.
    let back = serde_json::to_value(&trigger).unwrap();
    assert_eq!(back["max_consecutive_failures"], 3);
}

#[cfg(not(genai_strict_unknown))]
#[tokio::test]
async fn trigger_list_preserves_unknown_statuses() {
    let stub = Stub::replying(vec![
        Reply::json(
            200,
            json!({
                "triggers": [
                    {"id": "t-1", "status": "paused"},
                    {"id": "t-2", "status": "suspended_by_billing"}
                ],
                "next_page_token": "p2"
            }),
        ),
        // No triggers at all comes back as `{}`.
        Reply::json(200, json!({})),
    ])
    .await;
    let client = stub.client();

    let list = client.triggers().list().send().await.unwrap();
    assert_eq!(list.triggers[0].status, Some(TriggerStatus::Paused));
    let unknown = list.triggers[1].status.as_ref().unwrap();
    assert_eq!(unknown.unknown_status_type(), Some("suspended_by_billing"));
    assert_eq!(
        serde_json::to_value(unknown).unwrap(),
        "suspended_by_billing"
    );
    assert_eq!(list.next_page_token.as_deref(), Some("p2"));

    let empty = client.triggers().list().send().await.unwrap();
    assert!(empty.triggers.is_empty() && empty.next_page_token.is_none());
}

#[cfg(not(genai_strict_unknown))]
#[tokio::test]
async fn trigger_execution_responses_parse_under_both_list_keys() {
    let stub = Stub::replying(vec![
        Reply::json(
            200,
            json!({
                "id": "exec-1",
                "trigger_id": "t-1",
                "interaction_id": "int-9",
                "status": "queued",
                "scheduled_time": "2026-09-24T05:00:00Z",
                "attempt": 2
            }),
        ),
        Reply::json(
            200,
            json!({"trigger_executions": [{"id": "e1", "status": "completed"}], "next_page_token": "n"}),
        ),
        Reply::json(
            200,
            json!({"executions": [{"id": "e2", "status": "timed_out", "error": "deadline exceeded"}]}),
        ),
    ])
    .await;
    let client = stub.client();

    let run = client.triggers().run("t-1").await.unwrap();
    assert_eq!(run.interaction_id.as_deref(), Some("int-9"));
    assert_eq!(
        run.status.as_ref().unwrap().unknown_status_type(),
        Some("queued")
    );
    assert!(run.scheduled_time.is_some());
    assert_eq!(run.extra["attempt"], 2);

    let spec_key = client
        .triggers()
        .list_executions("t-1")
        .send()
        .await
        .unwrap();
    assert_eq!(
        spec_key.trigger_executions[0].status,
        Some(TriggerExecutionStatus::Completed)
    );
    assert_eq!(spec_key.next_page_token.as_deref(), Some("n"));

    let alias_key = client
        .triggers()
        .list_executions("t-1")
        .send()
        .await
        .unwrap();
    let execution = &alias_key.trigger_executions[0];
    assert_eq!(execution.status, Some(TriggerExecutionStatus::TimedOut));
    assert_eq!(execution.error.as_deref(), Some("deadline exceeded"));
}

/// `run` answers with the ended run's execution, as captured live
/// (2026-11-13): a completed run names its interaction and environment, and
/// one its execution timeout cut off says so in `error`, with neither.
#[tokio::test]
async fn trigger_run_returns_the_ended_runs_execution() {
    // Not the harness's import, which strict mode leaves out.
    use genai_rs::TriggerExecutionStatus;

    let stub = Stub::replying(vec![
        Reply::json(
            200,
            json!({
                "id": "1dbea6dd-f0f3-4ada-9a7b-b5fd92f3fac8",
                "start_time": "2026-11-13T00:02:48.193535Z",
                "end_time": "2026-11-13T00:03:01.047764Z",
                "status": "completed",
                "interaction_id": "v1_ChdybFAyYXZlTkRLdl92ZElQbEpQUmlRNBIXcmxQMmF2ZU5ES3ZfdmRJUGxKUFJpUTQ",
                "environment_id": "c71d9ecbbfd200336ff8a61f411cae6b",
                "trigger_id": "030588c7-31ce-4cf7-93a4-0657b567d786"
            }),
        ),
        Reply::json(
            200,
            json!({
                "id": "ac768074-100f-420f-9523-45437a0ad5e5",
                "start_time": "2026-11-13T00:06:28.171608Z",
                "end_time": "2026-11-13T00:07:28.810284Z",
                "status": "timed_out",
                "error": "Deadline expired before operation could complete.",
                "trigger_id": "62cf2482-62d0-40fd-9a32-ea7375ccd467"
            }),
        ),
    ])
    .await;
    let client = stub.client();

    let completed = client
        .triggers()
        .run("030588c7-31ce-4cf7-93a4-0657b567d786")
        .await
        .unwrap();
    assert_eq!(completed.status, Some(TriggerExecutionStatus::Completed));
    assert_eq!(
        completed.id.as_deref(),
        Some("1dbea6dd-f0f3-4ada-9a7b-b5fd92f3fac8")
    );
    assert_eq!(
        completed.trigger_id.as_deref(),
        Some("030588c7-31ce-4cf7-93a4-0657b567d786")
    );
    assert!(completed.interaction_id.is_some());
    assert_eq!(
        completed.environment_id.as_deref(),
        Some("c71d9ecbbfd200336ff8a61f411cae6b")
    );
    let (start, end) = (completed.start_time.unwrap(), completed.end_time.unwrap());
    assert_eq!((end - start).num_milliseconds(), 12_854);
    assert!(completed.scheduled_time.is_none() && completed.error.is_none());
    assert!(completed.extra.is_empty(), "{:?}", completed.extra);

    let timed_out = client
        .triggers()
        .run("62cf2482-62d0-40fd-9a32-ea7375ccd467")
        .await
        .unwrap();
    assert_eq!(timed_out.status, Some(TriggerExecutionStatus::TimedOut));
    assert_eq!(
        timed_out.error.as_deref(),
        Some("Deadline expired before operation could complete.")
    );
    assert!(timed_out.interaction_id.is_none() && timed_out.environment_id.is_none());
    assert!(timed_out.end_time.is_some());
    assert!(timed_out.extra.is_empty(), "{:?}", timed_out.extra);

    let targets: Vec<_> = stub.requests().into_iter().map(|r| r.target).collect();
    assert_eq!(
        targets,
        [
            "/v1beta/triggers/030588c7-31ce-4cf7-93a4-0657b567d786/executions",
            "/v1beta/triggers/62cf2482-62d0-40fd-9a32-ea7375ccd467/executions",
        ]
    );
}

#[tokio::test]
async fn trigger_list_items_follow_every_page_with_the_page_size() {
    let stub = Stub::replying(vec![
        Reply::json(
            200,
            json!({"triggers": [{"id": "t-1"}, {"id": "t-2"}], "next_page_token": "p2"}),
        ),
        Reply::json(200, json!({"triggers": [{"id": "t-3"}]})),
    ])
    .await;
    let client = stub.client();

    let triggers: Vec<Trigger> = client
        .triggers()
        .list()
        .with_page_size(2)
        .items()
        .try_collect()
        .await
        .unwrap();

    let ids: Vec<_> = triggers.iter().filter_map(|t| t.id.as_deref()).collect();
    assert_eq!(ids, ["t-1", "t-2", "t-3"]);
    assert_eq!(
        targets(&stub),
        [
            "/v1beta/triggers?page_size=2",
            "/v1beta/triggers?page_size=2&page_token=p2",
        ]
    );
}

#[tokio::test]
async fn trigger_execution_items_resend_the_trigger_and_page_size_on_every_page() {
    let stub = Stub::replying(vec![
        Reply::json(
            200,
            json!({
                "trigger_executions": [{"id": "e1"}, {"id": "e2"}],
                "next_page_token": "p2"
            }),
        ),
        // The path-segment spelling of the envelope key, on a later page.
        Reply::json(200, json!({"executions": [{"id": "e3"}]})),
    ])
    .await;
    let client = stub.client();

    let executions: Vec<TriggerExecution> = client
        .triggers()
        .list_executions("t/1")
        .with_page_size(2)
        .items()
        .try_collect()
        .await
        .unwrap();

    let ids: Vec<_> = executions.iter().filter_map(|e| e.id.as_deref()).collect();
    assert_eq!(ids, ["e1", "e2", "e3"]);
    // The owned trigger ID is encoded into every page's path.
    assert_eq!(
        targets(&stub),
        [
            "/v1beta/triggers/t%2F1/executions?page_size=2",
            "/v1beta/triggers/t%2F1/executions?page_size=2&page_token=p2",
        ]
    );
}
