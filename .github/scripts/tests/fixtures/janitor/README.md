# Janitor fixtures

Listings `fake_api_server.py` replays for `test_cleanup_test_resources.sh`,
routed by `routes.json`. The harness runs the janitor with "now" fixed at
2026-11-05T12:00:00Z, so its six-hour floor falls at 06:00:00Z.

Recorded from the shared test key on 2026-11-05 and trimmed (one script's
content cut short, the rest verbatim):

- `environments.page1.json`: the first five entries (an empty one from
  July, the `hello from genai-rs` and `probe` leaks from August, a fresh
  empty one, and the credentials test's `genai_rs_test/` sources, with
  its `created` moved back four days).
- `credentials.json`: the two `genai-rs-test-*` leaks and a UUID-named one
  (moved back four days).
- `fileSearchStores.json`: the first entry, another agent's probe store,
  which carries no `genai-rs-test` marker.
- `voices.prompted.json`: the first entry, a prompted voice made for this
  harness's probe, with its display name's timestamp moved back a day.
- `agents.json`: the shape of a probe agent; agents carry no timestamp.

Not a test's, written for the harness so that "kept" is exercised: the
environments named `...user`, `...mixed` and `...gitrepo`, the
`github-deploy-token` credential, the `wh-production` webhook, the
`support-bot` agent, the `t-nightly` trigger and the `Brand narrator`
voice. The `...prefix` and `...young` environments, the webhooks, the
triggers, the second store and the other voices are made-up test
resources in the recorded shapes, placed either side of the floor.
