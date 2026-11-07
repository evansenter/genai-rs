use super::*;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

/// The lines `drain_stderr` keeps from `data`.
async fn drained(data: &[u8]) -> Vec<String> {
    let ring = Arc::new(Mutex::new(StderrRing::default()));
    drain_stderr(data, Arc::clone(&ring), WireContext::new(Vec::new())).await;
    ring.lock().unwrap().lines.iter().cloned().collect()
}

#[tokio::test]
async fn test_stderr_drain_survives_invalid_utf8() {
    // The drain sits at the wrong-binary trust boundary: a non-UTF-8
    // line must be replaced lossily, not end the drain (a stopped
    // drain lets the pipe fill and deadlocks the child).
    let data: &[u8] = b"first line\n\xff\xfe broken\nafter\r\nlast without newline";
    let lines = drained(data).await;
    assert_eq!(lines.len(), 4, "got: {lines:?}");
    assert_eq!(lines[0], "first line");
    assert!(
        lines[1].contains('\u{FFFD}') && lines[1].ends_with(" broken"),
        "invalid bytes must be replaced lossily, got: {:?}",
        lines[1]
    );
    assert_eq!(lines[2], "after", "CRLF must be stripped");
    assert_eq!(lines[3], "last without newline");
}

#[tokio::test]
async fn test_stderr_drain_ring_is_bounded() {
    let mut data = Vec::new();
    for i in 0..(STDERR_RING_CAPACITY + 10) {
        data.extend_from_slice(format!("line {i}\n").as_bytes());
    }
    let lines = drained(&data[..]).await;
    assert_eq!(lines.len(), STDERR_RING_CAPACITY);
    assert_eq!(
        lines.last().unwrap(),
        &format!("line {}", STDERR_RING_CAPACITY + 9)
    );
}

#[tokio::test]
async fn test_stderr_drain_caps_a_line_with_no_newline() {
    // A megabyte with no newline (the wrong binary, a crash dump), then an
    // ordinary line: the long one keeps its head only, marked as cut, and
    // the next line is read normally.
    let mut data = vec![b'x'; 1024 * 1024];
    data.extend_from_slice(b"\r\nafter\n");
    let lines = drained(&data).await;
    assert_eq!(
        lines.len(),
        2,
        "{:?}",
        lines.iter().map(String::len).collect::<Vec<_>>()
    );
    let head = &lines[0];
    assert!(
        head.starts_with(&"x".repeat(MAX_STDERR_LINE_BYTES)),
        "the head is kept"
    );
    assert!(
        head.len() < MAX_STDERR_LINE_BYTES + 100,
        "a line is capped at {MAX_STDERR_LINE_BYTES} bytes, got {}",
        head.len()
    );
    assert!(
        head.ends_with("the rest was dropped]"),
        "{}",
        &head[MAX_STDERR_LINE_BYTES..]
    );
    assert_eq!(lines[1], "after");

    // A line of exactly the cap is whole, not cut.
    let mut exact = vec![b'y'; MAX_STDERR_LINE_BYTES];
    exact.push(b'\n');
    assert_eq!(drained(&exact).await, ["y".repeat(MAX_STDERR_LINE_BYTES)]);
}

#[tokio::test]
async fn test_stderr_drain_records_a_capped_line_before_its_newline_arrives() {
    // An endless line must not wait for a newline to show up: its head is
    // recorded once the cap is reached, and the rest is dropped as it
    // arrives.
    let (mut writer, reader) = tokio::io::duplex(4096);
    let ring = Arc::new(Mutex::new(StderrRing::default()));
    let drain = tokio::spawn(drain_stderr(
        reader,
        Arc::clone(&ring),
        WireContext::new(Vec::new()),
    ));
    writer.write_all(&vec![b'z'; 256 * 1024]).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while ring.lock().unwrap().lines.is_empty() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the capped head is recorded with no newline yet");
    writer.write_all(b"zzz\nnext\n").await.unwrap();
    drop(writer);
    drain.await.unwrap();
    let ring = ring.lock().unwrap();
    let lines: Vec<&str> = ring.lines.iter().map(String::as_str).collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with('z') && lines[0].ends_with("dropped]"));
    assert_eq!(lines[1], "next");
}

#[test]
fn test_stderr_ring_is_bounded_by_bytes() {
    let mut ring = StderrRing::default();
    let line = "e".repeat(4096);
    for _ in 0..STDERR_RING_CAPACITY {
        ring.push(line.clone());
    }
    ring.push("last".to_string());
    let held: usize = ring.lines.iter().map(String::len).sum();
    assert_eq!(held, ring.bytes, "the byte count tracks the lines held");
    assert!(
        held <= STDERR_RING_MAX_BYTES,
        "{held} bytes held, over the {STDERR_RING_MAX_BYTES}-byte cap"
    );
    assert_eq!(ring.lines.back().map(String::as_str), Some("last"));
    assert!(ring.tail().ends_with("\nlast"));

    // One line over the byte cap is still kept: the latest line always is.
    let mut ring = StderrRing::default();
    ring.push("o".repeat(STDERR_RING_MAX_BYTES + 1));
    assert_eq!(ring.lines.len(), 1);
}
