use super::capture;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn real_media_probe_capture_preserves_status_streams_and_spawn_cause() {
    let executable = std::env::var("PROBIERZ_TEST_FFPROBE")
        .expect("PROBIERZ_TEST_FFPROBE must name the real ffprobe executable");
    let fixture = PathBuf::from(
        std::env::var_os("PROBIERZ_TEST_VIDEO")
            .expect("PROBIERZ_TEST_VIDEO must name a recording from an actual product run"),
    )
    .canonicalize()
    .expect("resolve the genuine recording");
    let revision = std::env::var("PROBIERZ_TEST_SOURCE_REVISION")
        .expect("the qualification runner must bind the exact source revision");
    let original = std::fs::read(&fixture).expect("read original recording");
    let parent = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/real-tests/capture");
    std::fs::create_dir_all(&parent).expect("create ignored qualification directory");
    let isolated = tempfile::tempdir_in(parent).expect("create isolated capture directory");
    let args = |input: &std::path::Path| {
        ["-v", "error", "-show_format", "-of", "json"]
            .into_iter()
            .map(str::to_owned)
            .chain(std::iter::once(input.to_string_lossy().into_owned()))
            .collect::<Vec<_>>()
    };
    let success = capture(&executable, &args(&fixture), Some(isolated.path()), None);
    assert!(success.error.is_none(), "{:?}", success.error);
    assert!(
        success.status.expect("the real probe exited").success(),
        "{}",
        String::from_utf8_lossy(&success.stderr)
    );
    let document: serde_json::Value =
        serde_json::from_slice(&success.stdout).expect("real ffprobe metadata");
    assert_eq!(
        document.pointer("/format/filename").and_then(serde_json::Value::as_str),
        fixture.to_str()
    );
    let absent = isolated.path().join("absent-recording");
    let refused = capture(&executable, &args(&absent), Some(isolated.path()), None);
    assert!(refused.error.is_none(), "the child itself must report the missing input");
    assert!(!refused.status.expect("failed probe exited").success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains(&*absent.to_string_lossy()));
    let missing_program = isolated.path().join("absent-executable");
    let cause = Command::new(&missing_program)
        .spawn()
        .expect_err("isolated executable must not exist")
        .to_string();
    let launch = capture(
        missing_program.to_str().expect("test path is UTF-8"),
        &[],
        Some(isolated.path()),
        None,
    );
    assert!(launch.status.is_none());
    let launch_error = launch.error.expect("launch failure must remain a capture error");
    assert!(launch_error.contains(&cause));
    assert!(launch_error.contains(&*missing_program.to_string_lossy()));
    assert!(launch_error.contains(&*isolated.path().to_string_lossy()));
    assert_eq!(std::fs::read(&fixture).expect("read retained recording"), original);
    eprintln!(
        "capture source={revision} fixture={} executable={executable} refused_stderr={} launch_error={launch_error}",
        fixture.display(),
        String::from_utf8_lossy(&refused.stderr)
    );
}
