use std::sync::mpsc;
use std::time::Duration;

use super::*;

/// A disposable pipe name per test, so parallel tests and a running app never share one.
fn test_name(test: &str) -> String {
    format!(
        "Parabolica-test-{test}-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
    )
    .replace([' ', ':', '{', '}', ','], "")
}

fn primary(name: &str) -> InstanceServer {
    match claim(name).unwrap() {
        Claim::Primary(server) => server,
        Claim::Running(_) => panic!("{name} was already claimed"),
    }
}

fn running(name: &str) -> RunningInstance {
    match claim(name).unwrap() {
        Claim::Running(running) => running,
        Claim::Primary(_) => panic!("{name} was not claimed"),
    }
}

#[test]
fn a_second_claim_forwards_requests_to_the_primary_in_order() {
    let name = test_name("forward");
    let (sender, received) = mpsc::channel();
    let server = primary(&name).serve(move |request| sender.send(request).is_ok());

    let second = running(&name);
    second.send("C:\\Música\\曲.flac".as_bytes()).unwrap();
    second.send(b"").unwrap();
    let large = vec![7; MAX_REQUEST_BYTES];
    running(&name).send(&large).unwrap();

    let timeout = Duration::from_secs(5);
    assert_eq!(
        received.recv_timeout(timeout).unwrap(),
        "C:\\Música\\曲.flac".as_bytes()
    );
    assert_eq!(received.recv_timeout(timeout).unwrap(), b"");
    assert_eq!(received.recv_timeout(timeout).unwrap(), large);
    drop(server);
}

#[test]
fn an_oversized_request_is_refused_before_it_is_sent() {
    let name = test_name("oversized");
    let (sender, received) = mpsc::channel();
    let _server = primary(&name).serve(move |request| sender.send(request).is_ok());
    let error = running(&name)
        .send(&vec![0; MAX_REQUEST_BYTES + 1])
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(received.recv_timeout(Duration::from_millis(200)).is_err());
}

#[test]
fn a_rejected_request_reports_an_error_to_the_sender() {
    let name = test_name("rejected");
    let _server = primary(&name).serve(|request| request != b"bad");
    running(&name).send(b"good").unwrap();
    let error = running(&name).send(b"bad").unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn a_stalled_client_does_not_block_stopping_the_server() {
    let name = test_name("stalled");
    let server = primary(&name).serve(|_| true);
    // Connects and sends half of the length prefix, then waits.
    let mut stalled = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(format!(r"\\.\pipe\{name}"))
        .unwrap();
    std::io::Write::write_all(&mut stalled, &[1, 0]).unwrap();
    std::thread::sleep(Duration::from_millis(100));

    let started = std::time::Instant::now();
    drop(server);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    drop(stalled);
    let _again = primary(&name);
}

#[test]
fn stopping_the_server_releases_the_name() {
    let name = test_name("release");
    let server = primary(&name).serve(|_| true);
    drop(server);
    let _again = primary(&name);
}

#[test]
fn the_instance_name_is_scoped_to_the_user_sid() {
    let name = instance_name().unwrap();
    assert!(name.starts_with("Parabolica-S-1-5-"), "{name}");
    assert!(!name.contains('\\'));
}
