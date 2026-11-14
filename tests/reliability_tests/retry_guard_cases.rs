//! The retry-loop guard against the defects it reads ([`retry_loops`],
//! D-069): synthetic loops, and the shipped recipes broken one way at a
//! time. A module of `retry_recipes`, whose private checks it calls.

use super::*;

fn block(text: &str) -> DocBlock {
    DocBlock {
        at: "x".into(),
        lines: text.lines().map(str::to_string).collect(),
    }
}

/// The problems of the one retry or poll loop in `text`.
fn problems(text: &str) -> Vec<&'static str> {
    let found = retry_loops(&block(text));
    assert_eq!(found.len(), 1, "{found:?}");
    found.into_iter().next().unwrap().problems
}

/// The check flags each defect the old recipes had, and passes a loop
/// that has none.
#[test]
fn the_retry_loop_check_finds_each_defect() {
    let unbounded = "let response = loop {\n    match client.execute(request.clone()).await {\n        Ok(r) => break r,\n        Err(e) if e.is_retryable() => continue,\n        Err(e) => return Err(e.into()),\n    }\n};";
    assert_eq!(problems(unbounded), [UNBOUNDED, NO_SLEEP, DEAF]);
    let deaf = "let mut attempts = 0;\nlet response = loop {\n    match client.execute(request.clone()).await {\n        Ok(r) => break r,\n        Err(e) if e.is_retryable() && attempts < 3 => {\n            attempts += 1;\n            tokio::time::sleep(Duration::from_millis(100 * 2u64.pow(attempts))).await;\n        }\n        Err(e) => return Err(e.into()),\n    }\n};";
    assert_eq!(problems(deaf), [DEAF, UNCAPPED, NO_JITTER]);
    // A sleep or a retry_after only in a comment, a string or a hidden
    // line is none.
    let hidden = "loop {\n    // tokio::time::sleep(e.retry_after()) here\n    /* sleep(e.retry_after()) */ let s = \"sleep(e.retry_after())\";\n    # tokio::time::sleep(d).await;\n    if attempt < MAX && e.is_retryable() {}\n    attempt += 1;\n}";
    assert_eq!(problems(hidden), [NO_SLEEP, DEAF, NO_JITTER]);
    let sound = "while attempt < MAX_ATTEMPTS {\n    if e.is_retryable() {\n        let jitter = step.mul_f64(random());\n        tokio::time::sleep(e.retry_after().unwrap_or(step + jitter).min(MAX_DELAY)).await;\n    } else {\n        return Err(e);\n    }\n    attempt += 1;\n}";
    assert_eq!(problems(sound), Vec::<&str>::new());
    let ranged = "for attempt in 1..=MAX_ATTEMPTS {\n    if e.is_retryable() {\n        let delay = e.retry_after().unwrap_or(jittered(step)).min(MAX_DELAY);\n        tokio::time::sleep(delay).await;\n    } else {\n        return Err(e);\n    }\n}";
    assert_eq!(problems(ranged), Vec::<&str>::new());
    // A count that is checked but never incremented bounds nothing.
    let stuck = "while attempt < MAX_ATTEMPTS {\n    if e.is_retryable() {\n        tokio::time::sleep(e.retry_after().unwrap_or(step).min(MAX_DELAY)).await;\n    } else {\n        return Err(e);\n    }\n}";
    assert_eq!(problems(stuck), [UNBOUNDED]);
    // A count without jitter, and a server's wait taken whole.
    let herd = "for attempt in 1..=MAX_ATTEMPTS {\n    if e.is_retryable() {\n        tokio::time::sleep(e.retry_after().unwrap_or(step)).await;\n    } else {\n        return Err(e);\n    }\n}";
    assert_eq!(problems(herd), [UNCAPPED, NO_JITTER]);
    // Jitter is a random value that reaches the sleep, not a name that
    // happens to contain "rand"; growth needs no spaces around its `*`.
    let named = "for attempt in 1..=MAX_ATTEMPTS {\n    if e.is_retryable() {\n        let operand = step.mul_f64(0.5);\n        let brand = rand_seed();\n        delay = (delay + operand).min(MAX_DELAY);\n        tokio::time::sleep(e.retry_after().unwrap_or(delay).min(MAX_DELAY)).await;\n    } else {\n        return Err(e);\n    }\n}";
    assert_eq!(problems(named), [NO_JITTER]);
    let tight = "for attempt in 1..=MAX_ATTEMPTS {\n    if e.is_retryable() {\n        delay = delay*2;\n        tokio::time::sleep(e.retry_after().unwrap_or(jittered(delay)).min(MAX_DELAY) + delay).await;\n    } else {\n        return Err(e);\n    }\n}";
    assert_eq!(problems(tight), [UNCAPPED]);
    // A sleep only in a branch leaves the other way to the end of the body
    // without one, and a `continue` needs one before it, in its arm or
    // around it, not in another arm.
    let branch = "while attempt < MAX_ATTEMPTS {\n    if e.is_retryable() {\n        tokio::time::sleep(e.retry_after().unwrap_or(jittered(step)).min(MAX_DELAY)).await;\n    }\n    attempt += 1;\n}";
    assert_eq!(problems(branch), [NO_SLEEP]);
    let arms = |retry: &str| {
        format!(
            "for attempt in 1..=MAX_ATTEMPTS {{\n    match client.execute(request.clone()).await {{\n        Ok(r) => return Ok(r),\n{retry}        Err(e) => return Err(e.into()),\n    }}\n}}"
        )
    };
    let wait = "tokio::time::sleep(e.retry_after().unwrap_or(jittered(step)).min(MAX_DELAY)).await";
    let retry = |then: &str| {
        format!("        Err(e) if e.is_retryable() => {{\n            {wait};\n{then}        }}\n")
    };
    assert_eq!(
        problems(&arms(&retry("            continue;\n"))),
        Vec::<&str>::new()
    );
    assert_eq!(
        problems(&arms(&format!(
            "        Err(e) if e.is_retryable() && attempt == 1 => continue,\n        Err(e) if e.is_retryable() => {wait},\n"
        ))),
        [NO_SLEEP]
    );
    // A `return` or `break` in the arm `is_retryable()` guards gives up on
    // an error it says to retry; one in a branch that negates it, or after
    // a `let ... else`, does not.
    assert_eq!(problems(&arms(&retry("            break;\n"))), [GIVES_UP]);
    let negated = format!(
        "for attempt in 1..=MAX_ATTEMPTS {{\n    let Err(e) = client.execute(request.clone()).await else {{\n        break;\n    }};\n    if !e.is_retryable() {{\n        return Err(e.into());\n    }}\n    {wait};\n}}"
    );
    assert_eq!(problems(&negated), Vec::<&str>::new());
    let sleepless = negated.replace(
        &format!("    {wait};\n"),
        &format!("    if attempt > 1 {{\n        {wait};\n    }}\n"),
    );
    assert_eq!(problems(&sleepless), [NO_SLEEP]);
    // A poll bounded by its deadline compares with it inside the loop; a
    // deadline only declared above the loop, or named in an error
    // message, bounds nothing. A deadline loop may wait as long as the
    // server asks, but not add that wait to a time.
    let poll = |check: &str| {
        format!(
            "async fn poll(deadline: Duration) {{\n    let deadline = Instant::now() + deadline;\n    loop {{\n        let left = Duration::from_secs(3600);\n        match timeout(left, get()).await {{\n            Err(_) => return Err(\"polling deadline exceeded\".into()),\n            Ok(Err(e)) if e.is_retryable() => wait = e.retry_after().unwrap_or(delay),\n            _ => {{}}\n        }}\n{check}        sleep(wait).await;\n    }}\n}}"
        )
    };
    assert_eq!(problems(&poll("")), [UNBOUNDED, UNCAPPED]);
    assert_eq!(
        problems(&poll(
            "        if wait >= deadline.saturating_duration_since(Instant::now()) {\n            return Err(e);\n        }\n"
        )),
        Vec::<&str>::new()
    );
    assert_eq!(
        problems(&poll(
            "        if Instant::now() + wait >= deadline {\n            return Err(e);\n        }\n"
        )),
        [OVERFLOWS]
    );
    assert_eq!(
        problems(&poll(
            "        if start.elapsed() + wait > max_wait {\n            return Err(e);\n        }\n"
        )),
        [OVERFLOWS]
    );
    // A poll that sends its status request with `?` ends on the first
    // transient error, and one bounded by a deadline needs each request
    // bounded too.
    let hasty = "loop {\n    let response = client\n        .interactions()\n        .get(&id)\n        .await?;\n    if start.elapsed() > max_wait {\n        return Ok(None);\n    }\n    sleep(Duration::from_secs(5)).await;\n}";
    assert_eq!(problems(hasty), [DEAF, GIVES_UP, STALLS]);
    // So does any loop the check reads that awaits with `?`, whatever the
    // request, and an `.await` in parentheses is awaited all the same.
    let created = "loop {\n    let r = client.interactions().create().send().await?;\n    if start.elapsed() > max_wait {\n        return Ok(None);\n    }\n    sleep(Duration::from_secs(5)).await;\n}";
    assert_eq!(problems(created), [GIVES_UP]);
    let counted = "for attempt in 1..=MAX_ATTEMPTS {\n    let r = client.execute(request.clone()).await?;\n    tokio::time::sleep(jittered(step).min(MAX_DELAY)).await;\n}";
    assert_eq!(problems(counted), [GIVES_UP]);
    let grouped = format!(
        "for attempt in 1..=MAX_ATTEMPTS {{\n    let r = (client.execute(request.clone()).await)?;\n    if e.is_retryable() {{\n        {wait};\n    }} else {{\n        return Err(e);\n    }}\n}}"
    );
    assert_eq!(problems(&grouped), [GIVES_UP]);
    // A closure's body is its own, braces or none: a sleep in one that is
    // never called is no sleep on the path.
    let napless = negated.replace(
        &format!("    {wait};\n"),
        &format!("    let _nap = || {wait};\n    if attempt > 1 {{\n        continue;\n    }}\n    {wait};\n"),
    );
    assert_eq!(problems(&napless), [NO_SLEEP]);
    // Not retry loops: is_retryable() outside a loop, a map lookup in a
    // loop, a loop nested in one (the inner loop is), and a `for` over a
    // collection that paces itself.
    for text in [
        "fn f(e: &GenaiError) -> bool { e.is_retryable() }",
        "loop {\n    if let Some(v) = cache.get(&key) {\n        break v;\n    }\n}",
        "for item in items {\n    client.send(item).await?;\n    sleep(PACE).await;\n}",
    ] {
        assert!(retry_loops(&block(text)).is_empty(), "{text}");
    }
    let nested = "for id in ids {\n    let mut attempt = 0;\n    while attempt < MAX {\n        attempt += 1;\n        sleep(e.retry_after().unwrap_or(jittered(step)).min(MAX_DELAY)).await;\n        if e.is_retryable() {}\n    }\n}";
    let found = retry_loops(&block(nested));
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].head, "while attempt < MAX {");
}

/// `block` with each `(from, to)` replaced, every `from` required, so a
/// recipe that changes cannot make the mutation a no-op.
fn mutated(block: &DocBlock, edits: &[(&str, &str)]) -> DocBlock {
    let mut text = block.lines.join("\n");
    for (from, to) in edits {
        assert!(text.contains(from), "{}: no {from:?} to mutate", block.at);
        text = text.replace(from, to);
    }
    DocBlock {
        at: block.at.clone(),
        lines: text.lines().map(str::to_string).collect(),
    }
}

/// The `(from, to)` replacements [`mutated`] makes.
type Edits<'a> = &'a [(&'a str, &'a str)];

/// Each defect found in a shipped recipe fails the check against the real
/// text: the request recipe without its cap, with the cap on the backoff
/// alone, or with its jitter dropped or left unused; the doc's poll and
/// the example's without the cap on their growth (or doubling by
/// addition), comparing the clock rather than the wait with the deadline,
/// applying `+` or `-` to a `Retry-After` (in a variable, inline or in a
/// closure) or unwrapping `checked_` arithmetic on one (E3), going back
/// to the loop's head without the sleep (E1, also with a sleep in a
/// closure never called), sending a status request with `?` after any
/// chain or around its parentheses (E4), or returning or breaking in the
/// retryable arm (E9); the example deciding other than the doc's poll
/// does (E7, a terminal status dropped, or a decision beside the `match`
/// on the status); the doc's deadline added with `+`; and the poll
/// `examples/deep_research.rs` had, which ended on the first transient
/// error and never bounded a request by the time left. (E1 to E9 name the
/// mutations a review under #504 found passing.)
#[test]
fn the_retry_loop_check_fails_each_broken_recipe() {
    let only = |block: &DocBlock| {
        let found = retry_loops(block);
        assert_eq!(found.len(), 1, "{}: {found:?}", block.at);
        found.into_iter().next().unwrap().problems
    };
    let request = recipe("src/request/mod.rs", "is_retryable");
    assert_eq!(only(&request), Vec::<&str>::new());
    let uncapped = mutated(&request, &[(".min(MAX_DELAY)", "")]);
    assert_eq!(only(&uncapped), [UNCAPPED]);
    let herd = mutated(
        &request,
        &[
            (
                "let backoff = step + step.mul_f64(random);",
                "let backoff = step;",
            ),
            (
                "let random = RandomState::new().hash_one(attempt) as f64 / u64::MAX as f64;",
                "",
            ),
        ],
    );
    assert_eq!(only(&herd), [NO_JITTER]);
    // Jitter computed but never added to the wait is none.
    let unused = mutated(
        &request,
        &[(
            "let backoff = step + step.mul_f64(random);",
            "let backoff = step;",
        )],
    );
    assert_eq!(only(&unused), [NO_JITTER]);
    // A `.min(` inside the fallback caps the backoff, not the server's
    // Retry-After.
    let inner = mutated(
        &request,
        &[(
            ".unwrap_or(backoff).min(MAX_DELAY);",
            ".unwrap_or(backoff.min(MAX_DELAY));",
        )],
    );
    assert_eq!(only(&inner), [UNCAPPED]);
    // A `.min(` after a method that panics on a Retry-After too large comes
    // too late: jitter scaled onto the server's wait, or the wait scaled.
    for scaled in [
        "e.retry_after().map(|asked| asked.mul_f64(1.0 + random)).unwrap_or(backoff).min(MAX_DELAY);",
        "e.retry_after().unwrap_or(backoff).mul_f64(1.5).min(MAX_DELAY);",
    ] {
        let broken = mutated(
            &request,
            &[("e.retry_after().unwrap_or(backoff).min(MAX_DELAY);", scaled)],
        );
        assert_eq!(only(&broken), [OVERFLOWS], "{scaled}");
    }

    let poll = recipe("docs/AGENTS_AND_BACKGROUND.md", "is_retryable");
    assert_eq!(only(&poll), Vec::<&str>::new());
    let overflowing = mutated(
        &poll,
        &[(
            "if wait >= deadline.saturating_duration_since(Instant::now()) {",
            "if Instant::now() + wait >= deadline {",
        )],
    );
    assert_eq!(only(&overflowing), [OVERFLOWS]);
    let growing = mutated(
        &poll,
        &[(
            "delay = (delay * 2).min(Duration::from_secs(10));",
            "delay = delay * 2;",
        )],
    );
    assert_eq!(only(&growing), [UNCAPPED]);

    // The doc's poll and the example's, each broken one way at a time.
    let example = recipe("examples/deep_research.rs", "is_retryable");
    assert_eq!(only(&example), Vec::<&str>::new());
    const GUARD: &str = "if wait >= deadline.saturating_duration_since(Instant::now()) {";
    const WAIT: &str = "wait = e.retry_after().map_or(delay, |asked| asked.max(delay));";
    for poll in [&poll, &example] {
        let grow = poll
            .lines
            .iter()
            .map(|line| line.trim())
            .find(|line| line.starts_with("delay = (delay * 2).min("))
            .unwrap();
        let broken: &[(Edits, &[&str])] = &[
            // Checking the clock, not the wait, against the deadline
            // leaves a Retry-After of u64::MAX seconds to sleep whole.
            (&[(GUARD, "if Instant::now() >= deadline {")], &[UNCAPPED]),
            // Arithmetic on a Retry-After overflows however it is
            // spelled: inline (here in the retryable arm, which gives up
            // there too), in a closure, or subtracted.
            (
                &[(
                    WAIT,
                    "if Instant::now() + e.retry_after().unwrap_or(delay) >= deadline {\n    return Err(e.into());\n}",
                )],
                &[GIVES_UP, OVERFLOWS],
            ),
            (
                &[(
                    WAIT,
                    "wait = e.retry_after().map_or(delay, |asked| asked + delay);",
                )],
                &[OVERFLOWS],
            ),
            (
                &[(GUARD, "if Instant::now() >= deadline - wait {")],
                &[OVERFLOWS],
            ),
            // A `.min(` beside the Retry-After, not on it, caps nothing.
            (
                &[
                    (
                        WAIT,
                        "wait = e.retry_after().unwrap_or(delay.min(MAX_POLL_DELAY));",
                    ),
                    (GUARD, "if Instant::now() + wait >= deadline {"),
                ],
                &[OVERFLOWS],
            ),
            // E3: `checked_` arithmetic whose `None` is unwrapped panics as
            // `+` does, on the wait or with it as the argument; one whose
            // `None` is handled does not.
            (
                &[(
                    GUARD,
                    "if Instant::now().checked_add(wait).unwrap() >= deadline {",
                )],
                &[OVERFLOWS],
            ),
            (
                &[(
                    GUARD,
                    "if Instant::now().checked_add(wait).expect(\"in range\") >= deadline {",
                )],
                &[OVERFLOWS],
            ),
            (
                &[(
                    GUARD,
                    "if Instant::now() >= deadline.checked_sub(wait).unwrap() {",
                )],
                &[OVERFLOWS],
            ),
            (
                &[(
                    WAIT,
                    "wait = e.retry_after().map_or(delay, |asked| asked.checked_add(delay).unwrap());",
                )],
                &[OVERFLOWS],
            ),
            (
                &[(
                    GUARD,
                    "if Instant::now().checked_add(wait).is_none_or(|end| end >= deadline) {",
                )],
                &[],
            ),
            // So does a `Duration` method that scales or adds, on the
            // Retry-After or with it in its argument: `mul_f64(1.5)` and
            // `div_f64(0.5)` panic on u64::MAX seconds, as do the float
            // constructor and `add`. Saturating arithmetic does not.
            (
                &[(
                    WAIT,
                    "wait = e.retry_after().map_or(delay, |asked| asked.mul_f64(1.5).max(delay));",
                )],
                &[OVERFLOWS],
            ),
            (
                &[(
                    WAIT,
                    "wait = e.retry_after().map_or(delay, |asked| asked.div_f64(0.5).max(delay));",
                )],
                &[OVERFLOWS],
            ),
            (
                &[(
                    WAIT,
                    "wait = e.retry_after().map_or(delay, |asked| asked.add(delay));",
                )],
                &[OVERFLOWS],
            ),
            (
                &[(
                    WAIT,
                    "wait = e.retry_after().map_or(delay, |asked| Duration::from_secs_f64(asked.as_secs_f64()).max(delay));",
                )],
                &[OVERFLOWS],
            ),
            (
                &[(GUARD, "if Instant::now().add(wait) >= deadline {")],
                &[OVERFLOWS],
            ),
            (
                &[(
                    WAIT,
                    "wait = e.retry_after().map_or(delay, |asked| asked.saturating_add(delay));",
                )],
                &[],
            ),
            // A Retry-After passed to `max` or `saturating_add`, as the
            // receiver or as an argument, is in what the call returns, so
            // what applies to that is checked as on the Retry-After. Past
            // a `.min(` it is capped; a `min` with it as the argument
            // returns no more than its receiver.
            (
                &[(
                    WAIT,
                    "wait = e.retry_after().map_or(delay, |asked| delay.max(asked).mul_f64(1.5));",
                )],
                &[OVERFLOWS],
            ),
            (
                &[(
                    WAIT,
                    "wait = e.retry_after().map_or(delay, |asked| std::cmp::max(asked, delay).mul_f64(1.5));",
                )],
                &[OVERFLOWS],
            ),
            (
                &[(
                    WAIT,
                    "wait = e.retry_after().map_or(delay, |asked| delay.max(asked) + delay);",
                )],
                &[OVERFLOWS],
            ),
            (
                &[(
                    WAIT,
                    "wait = e.retry_after().map_or(delay, |asked| delay.saturating_add(asked).max(delay).mul_f64(1.5));",
                )],
                &[OVERFLOWS],
            ),
            (
                &[(
                    WAIT,
                    "wait = e.retry_after().map_or(delay, |asked| std::cmp::max(delay, asked));",
                )],
                &[],
            ),
            (
                &[(
                    WAIT,
                    "wait = e.retry_after().map_or(delay, |asked| delay.max(asked).min(MAX_POLL_DELAY).mul_f64(1.5));",
                )],
                &[],
            ),
            (
                &[(
                    WAIT,
                    "wait = e.retry_after().map_or(delay, |asked| delay.min(asked).mul_f64(1.5));",
                )],
                &[],
            ),
            // Doubling by addition is growth too.
            (&[(grow, "delay += delay;")], &[UNCAPPED]),
            (&[(grow, "delay = delay + delay;")], &[UNCAPPED]),
            // E1: an in-progress status that goes back to the loop's head
            // skips the sleep, and polls as fast as the server answers.
            (&[("_ => {}", "_ => continue,")], &[NO_SLEEP]),
            // A sleep in a closure never called is none on the path.
            (
                &[
                    ("_ => {}", "_ => continue,"),
                    (
                        "let mut wait = delay;",
                        "let mut wait = delay;\nlet _nap = || sleep(wait);",
                    ),
                ],
                &[NO_SLEEP],
            ),
            // A status request sent with `?`, after any chain (E4), ends
            // the poll on its first transient error, whatever else the
            // loop retries; so does a `return` or `break` in the arm
            // `is_retryable()` guards (E9).
            (
                &[(
                    "let mut wait = delay;",
                    "let mut wait = delay;\nlet _s = client.interactions().get(id).await?;",
                )],
                &[GIVES_UP],
            ),
            (
                &[(
                    "let mut wait = delay;",
                    "let mut wait = delay;\nlet _s = client.interactions().get(id).await.map_err(Box::new)?;",
                )],
                &[GIVES_UP],
            ),
            (
                &[(
                    "let mut wait = delay;",
                    "let mut wait = delay;\nlet _s = (client.interactions().get(id).await)?;",
                )],
                &[GIVES_UP],
            ),
            (
                &[(
                    WAIT,
                    "wait = e.retry_after().map_or(delay, |asked| asked.max(delay));\nreturn Err(e.into());",
                )],
                &[GIVES_UP],
            ),
            (
                &[(
                    WAIT,
                    "wait = e.retry_after().map_or(delay, |asked| asked.max(delay));\nbreak;",
                )],
                &[GIVES_UP],
            ),
        ];
        for &(edits, want) in broken {
            assert_eq!(only(&mutated(poll, edits)), want, "{}: {edits:?}", poll.at);
        }
    }

    // The example's poll decides as the doc's does. E7: without
    // Incomplete among its terminal statuses it polls a finished run to its
    // budget and cancels it. The stated difference, `None` for an error,
    // holds only at the deadline.
    let decided = poll_decisions(&poll);
    for edits in [
        &[("| InteractionStatus::Incomplete", "")][..],
        &[("|asked| asked.max(delay)", "|asked| asked")],
        &[(
            GUARD,
            "if wait > deadline.saturating_duration_since(Instant::now()) {",
        )],
        &[(
            "Ok(Err(e)) => return Err(e.into()),",
            "Ok(Err(e)) => return Ok(None),",
        )],
        // A decision taken beside the `match` on the status counts too.
        &[(
            "println!(\"  {:>4}s  {:?}\", start.elapsed().as_secs(), response.status);",
            "if response.status == InteractionStatus::Incomplete {\n    return Ok(None);\n}",
        )],
    ] {
        let broken = example_as_doc(poll_decisions(&mutated(&example, edits)));
        assert_ne!(broken, decided, "{edits:?}");
    }
    // The doc's poll builds its deadline with `checked_add`; `+` panics
    // on a caller's deadline too far off.
    let unchecked = mutated(
        &poll,
        &[(
            ".checked_add(deadline).ok_or(\"deadline too far off\")?",
            " + deadline",
        )],
    );
    assert!(!checked_deadline(&unchecked));
    // So does a `checked_add` whose `None` is unwrapped, in either.
    for (poll, handled) in [
        (&poll, ".ok_or(\"deadline too far off\")?"),
        (
            &example,
            ".ok_or(\"DEEP_RESEARCH_MAX_WAIT_SECS is too large\")?",
        ),
    ] {
        assert!(checked_deadline(poll), "{}", poll.at);
        for unwrapped in [".unwrap()", ".expect(\"in range\")"] {
            let broken = mutated(poll, &[(handled, unwrapped)]);
            assert!(!checked_deadline(&broken), "{}: {unwrapped}", poll.at);
        }
    }
    // The example backs off as the doc's poll does, ceiling included.
    let slower = mutated(
        &example,
        &[(
            "delay = (delay * 2).min(Duration::from_secs(10));",
            "delay = (delay * 2).min(Duration::from_secs(15));",
        )],
    );
    assert_ne!(backoff(&slower), backoff(&poll));

    assert_eq!(
        only(&block(OLD_DEEP_RESEARCH_POLL)),
        [DEAF, GIVES_UP, STALLS]
    );
}

/// `examples/deep_research.rs`'s poll before it took the doc's.
const OLD_DEEP_RESEARCH_POLL: &str = r#"
async fn poll(
    client: &Client,
    mut response: InteractionResponse,
    max_wait: Duration,
) -> Result<Option<InteractionResponse>, Box<dyn Error>> {
    let id = response.id.clone().ok_or("interaction has no ID")?;
    let start = Instant::now();
    let mut delay = Duration::from_secs(2);
    loop {
        match response.status {
            InteractionStatus::Completed => return Ok(Some(response)),
            InteractionStatus::Failed | InteractionStatus::Cancelled => {
                return Err(format!("research ended with status {:?}", response.status).into());
            }
            _ => {}
        }
        if start.elapsed() + delay > max_wait {
            return Ok(None);
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(MAX_POLL_DELAY);
        response = client.interactions().get(&id).await?;
        println!("  {:>4}s  {:?}", start.elapsed().as_secs(), response.status);
    }
}
"#;
