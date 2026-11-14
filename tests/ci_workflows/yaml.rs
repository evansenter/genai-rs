//! The workflows read as YAML, by a YAML parser (`yaml-rust2`): a key is
//! the key YAML reads, quoted or with a space before its colon, and a
//! `run:` value is the string YAML reads, a plain value's deeper
//! continuation lines and a folded block's lines joined as YAML joins
//! them (D-058). The runner splices each `${{ }}` expression's value into
//! that string before the shell reads it, which `super::shell` reads
//! ([`matrix_values`] gives it a matrix's values). The guards still find a
//! job or a step by its text (`super::jobs`, `super::steps`);
//! [`split_problems`] checks that each one they found is one YAML reads,
//! with every key YAML gives it.

use yaml_rust2::yaml::Hash;
use yaml_rust2::{Yaml, YamlLoader};

/// `text` read as one YAML document (an empty one is null).
pub(super) fn document(text: &str) -> Result<Yaml, String> {
    let mut docs = YamlLoader::load_from_str(text).map_err(|e| e.to_string())?;
    match docs.len() {
        0 => Ok(Yaml::Null),
        1 => Ok(docs.remove(0)),
        n => Err(format!("{n} YAML documents, not one")),
    }
}

/// The mapping of the one step the text of a step holds: a list of one
/// item, a mapping.
pub(super) fn step_mapping(step: &str) -> Result<Hash, String> {
    match document(step)? {
        Yaml::Array(items) => match <[Yaml; 1]>::try_from(items) {
            Ok([Yaml::Hash(step)]) => Ok(step),
            Ok([other]) => Err(format!("a step that is not a mapping: {}", text(&other))),
            Err(items) => Err(format!(
                "{} list items where the guard split one step",
                items.len()
            )),
        },
        other => Err(format!("a step that is not a list item: {}", text(&other))),
    }
}

/// The mapping the text of a job (its keys at four spaces) reads as.
pub(super) fn job_mapping(job: &str) -> Result<Hash, String> {
    match document(job)? {
        Yaml::Hash(job) => Ok(job),
        other => Err(format!("a job that is not a mapping: {}", text(&other))),
    }
}

/// The value of `key` in `map`.
pub(super) fn get<'a>(map: &'a Hash, key: &str) -> Option<&'a Yaml> {
    map.get(&Yaml::String(key.to_owned()))
}

/// A value as text: a scalar as YAML read it (a string without its quotes),
/// a list as `[a, b]`, a mapping as `{k: v}`.
pub(super) fn text(value: &Yaml) -> String {
    match value {
        Yaml::String(s) | Yaml::Real(s) => s.clone(),
        Yaml::Integer(n) => n.to_string(),
        Yaml::Boolean(b) => b.to_string(),
        Yaml::Null => String::new(),
        Yaml::Array(items) => format!(
            "[{}]",
            items.iter().map(text).collect::<Vec<_>>().join(", ")
        ),
        Yaml::Hash(map) => format!(
            "{{{}}}",
            map.iter()
                .map(|(key, value)| format!("{}: {}", text(key), text(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Yaml::Alias(_) | Yaml::BadValue => "<unreadable>".to_owned(),
    }
}

/// The `run:` value of a step, as YAML reads it, before the runner
/// splices its `${{ }}` expressions in: `None` for a step with no `run:`.
pub(super) fn run_value(step: &str) -> Result<Option<String>, String> {
    let step = step_mapping(step)?;
    Ok(get(&step, "run").map(text))
}

/// The value of the step's own `key` (its `if:`, say), as YAML reads it:
/// `None` when the step has no such key, an error when YAML cannot read
/// the text as one step.
pub(super) fn step_value(step: &str, key: &str) -> Result<Option<String>, String> {
    Ok(get(&step_mapping(step)?, key).map(text))
}

/// The value of the job's own `key` (its `if:`, say, not a step's), as
/// YAML reads it: `None` when the job has no such key, an error when YAML
/// cannot read the text as a job.
pub(super) fn job_value(job: &str, key: &str) -> Result<Option<String>, String> {
    Ok(get(&job_mapping(job)?, key).map(text))
}

/// The value the step's `env:` gives `name`, as YAML reads it.
pub(super) fn step_env(step: &str, name: &str) -> Option<String> {
    let step = step_mapping(step).ok()?;
    let env = get(&step, "env")?.as_hash()?;
    get(env, name).map(text)
}

/// The values `job`'s `strategy.matrix` gives `key`, which the runner
/// splices in for `${{ matrix.<key> }}`: the key's own list and each
/// `include` entry's value. An error when the guard cannot read every one:
/// a matrix that is not a mapping (one an expression builds), a value that
/// is not a scalar, a merge key (`<<`) whose keys the parser does not merge
/// in, or no value at all.
pub(super) fn matrix_values(job: &Hash, key: &str) -> Result<Vec<String>, String> {
    let matrix = get(job, "strategy")
        .and_then(Yaml::as_hash)
        .and_then(|strategy| get(strategy, "matrix"))
        .ok_or("the job has no `strategy.matrix`")?;
    let matrix = matrix
        .as_hash()
        .ok_or_else(|| format!("a matrix that is not a mapping: {}", text(matrix)))?;
    let scalar = |value: &Yaml| match value {
        Yaml::String(_) | Yaml::Integer(_) | Yaml::Real(_) | Yaml::Boolean(_) => Ok(text(value)),
        other => Err(format!("a `{key}` that is not a scalar: {}", text(other))),
    };
    let merged = |map: &Hash| {
        get(map, "<<").map_or(Ok(()), |_| {
            Err("a merge key (`<<`) in the matrix".to_owned())
        })
    };
    merged(matrix)?;
    let mut values = Vec::new();
    if let Some(own) = get(matrix, key) {
        for value in own
            .as_vec()
            .ok_or_else(|| format!("a `{key}` that is not a list: {}", text(own)))?
        {
            values.push(scalar(value)?);
        }
    }
    if let Some(include) = get(matrix, "include") {
        for entry in include
            .as_vec()
            .ok_or_else(|| format!("an `include` that is not a list: {}", text(include)))?
        {
            let entry = entry.as_hash().ok_or_else(|| {
                format!("an `include` entry that is not a mapping: {}", text(entry))
            })?;
            merged(entry)?;
            if let Some(value) = get(entry, key) {
                values.push(scalar(value)?);
            }
        }
    }
    if values.is_empty() {
        return Err(format!("no value for `{key}` in the job's matrix"));
    }
    Ok(values)
}

/// The jobs of a workflow document, by id.
pub(super) fn document_jobs(doc: &Yaml) -> Vec<(String, &Hash)> {
    doc["jobs"]
        .as_hash()
        .map(|jobs| {
            jobs.iter()
                .filter_map(|(id, job)| Some((text(id), job.as_hash()?)))
                .collect()
        })
        .unwrap_or_default()
}

/// The steps of a job, as YAML reads them.
pub(super) fn job_steps(job: &Hash) -> Vec<&Yaml> {
    get(job, "steps")
        .and_then(Yaml::as_vec)
        .map(|steps| steps.iter().collect())
        .unwrap_or_default()
}

/// Whether the job `job` and its step `step`, each read from the text the
/// guard split, are a job of `workflow` and one of that job's steps, with
/// every key YAML gives them: `None` when they are, or what differs. A
/// line the split left out of the step (a key past a comment it stopped
/// at) or put in it (a key of the next step) shows here.
pub(super) fn split_problems(workflow: &str, job: &str, step: &str) -> Option<String> {
    let read = (document(workflow), job_mapping(job), step_mapping(step));
    let (doc, job, step) = match read {
        (Ok(doc), Ok(job), Ok(step)) => (doc, job, step),
        (doc, job, step) => {
            let errors: Vec<String> = [doc.err(), job.err(), step.err()]
                .into_iter()
                .flatten()
                .collect();
            return Some(format!(
                "YAML the guard cannot read, so neither can it read the step's keys: {}",
                errors.join("; ")
            ));
        }
    };
    let found = document_jobs(&doc).into_iter().any(|(_, read)| {
        *read == job
            && job_steps(read)
                .into_iter()
                .any(|read| *read == Yaml::Hash(step.clone()))
    });
    (!found).then(|| {
        "a step or job the guard split from the text differently from how YAML reads \
         the workflow, so a key YAML gives it may be one the guard never reads"
            .to_owned()
    })
}

/// Problems with how the guards split `workflow` into jobs and steps
/// (`super::jobs`, `super::steps`), against how YAML reads it: the same
/// job ids, in order, and each job's text and each of its steps' reading
/// as the job and the step YAML reads there. Empty when the split is the
/// workflow's own.
pub(super) fn structure_problems(workflow: &str) -> Vec<String> {
    let doc = match document(workflow) {
        Ok(doc) => doc,
        Err(e) => return vec![format!("not YAML: {e}")],
    };
    let read = document_jobs(&doc);
    let split = super::jobs(workflow);
    let read_ids: Vec<&str> = read.iter().map(|(id, _)| id.as_str()).collect();
    let split_ids: Vec<&str> = split.iter().map(|(id, _)| id.as_str()).collect();
    if read_ids != split_ids {
        return vec![format!(
            "the guards split jobs {split_ids:?}, but YAML reads {read_ids:?}"
        )];
    }
    let mut problems = Vec::new();
    for ((id, read), (_, body)) in read.iter().zip(&split) {
        match job_mapping(body) {
            Ok(job) if job == **read => {}
            Ok(_) => problems.push(format!("job {id}: its text reads as another job")),
            Err(e) => problems.push(format!("job {id}: {e}")),
        }
        let read_steps = job_steps(read);
        let split_steps = super::steps(body);
        if read_steps.len() != split_steps.len() {
            problems.push(format!(
                "job {id}: the guards split {} steps, but YAML reads {}",
                split_steps.len(),
                read_steps.len()
            ));
            continue;
        }
        for (i, (read, text)) in read_steps.iter().zip(&split_steps).enumerate() {
            match step_mapping(text) {
                Ok(step) if Yaml::Hash(step.clone()) == **read => {}
                Ok(_) => problems.push(format!(
                    "job {id}, step {i}: its text reads as another step"
                )),
                Err(e) => problems.push(format!("job {id}, step {i}: {e}")),
            }
        }
    }
    problems
}

/// The guards' split of every workflow is the one YAML reads: each job and
/// step they find holds every key YAML gives it, and none they do not.
#[test]
fn the_guards_split_each_workflow_as_yaml_reads_it() {
    let mut problems = Vec::new();
    let mut steps = 0;
    for (name, text) in super::workflows() {
        problems.extend(
            structure_problems(&text)
                .into_iter()
                .map(|problem| format!("{name}: {problem}")),
        );
        steps += super::jobs(&text)
            .iter()
            .map(|(_, body)| super::steps(body).len())
            .sum::<usize>();
    }
    assert!(steps > 100, "found only {steps} steps");
    assert!(
        problems.is_empty(),
        "the guards' split of the workflows differs from YAML's:\n  {}",
        problems.join("\n  ")
    );

    // A key past a comment the split once stopped at is read as the
    // step's, as YAML reads it.
    let workflow = "on: push\njobs:\n  a:\n    runs-on: x\n    steps:\n      - run: x\n    # a comment\n        continue-on-error: true\n      - run: y\n";
    assert!(structure_problems(workflow).is_empty());
    let (_, job) = &super::jobs(workflow)[0];
    let read = step_mapping(&super::steps(job)[0]).unwrap();
    assert_eq!(get(&read, "continue-on-error"), Some(&Yaml::Boolean(true)));
    // A split the text cannot make as YAML does is named: a step whose `-`
    // stands alone on its line, a job header with a comment or quotes, and
    // a job whose `steps:` is its first key.
    for broken in [
        "on: push\njobs:\n  a:\n    runs-on: x\n    steps:\n      - run: x\n      -\n        run: y\n",
        "on: push\njobs:\n  a: # a comment\n    runs-on: x\n    steps:\n      - run: x\n",
        "on: push\njobs:\n  \"a\":\n    runs-on: x\n    steps:\n      - run: x\n",
        "on: push\njobs:\n  a:\n    steps:\n      - run: x\n",
    ] {
        assert!(!structure_problems(broken).is_empty(), "{broken}");
    }
}

/// A value reads as YAML reads it: quoted or spaced keys are the key, and a
/// `run:` value is joined as YAML joins it.
#[test]
fn a_step_reads_as_yaml_reads_it() {
    for (step, run) in [
        ("      - run: a\n          || true\n", "a || true"),
        (
            "      - run: >\n          set -e\n          a || true\n",
            "set -e a || true\n",
        ),
        ("      - run: \"a\n          || true\"\n", "a || true"),
        ("      - run: 'it''s'\n", "it's"),
        ("      - run: |\n          a\n            b\n", "a\n  b\n"),
    ] {
        assert_eq!(run_value(step).unwrap().as_deref(), Some(run), "{step}");
    }
    for step in [
        "      - \"continue-on-error\": true\n        run: a\n",
        "      - continue-on-error : true\n        run: a\n",
        "      - 'continue-on-error': true\n        run: a\n",
    ] {
        let read = step_mapping(step).unwrap();
        assert_eq!(get(&read, "continue-on-error"), Some(&Yaml::Boolean(true)));
    }
    assert_eq!(
        step_env(
            "      - run: a\n        env:\n          \"KEY\": \"${{ secrets.KEY }}\"\n",
            "KEY"
        )
        .as_deref(),
        Some("${{ secrets.KEY }}")
    );
    // Two items, or a key YAML reads twice, are not one step.
    assert!(step_mapping("      - run: a\n      - run: b\n").is_err());
    assert!(step_mapping("      - run: a\n        run: b\n").is_err());
}
