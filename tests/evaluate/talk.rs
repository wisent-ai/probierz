//! `probierz evaluate talk` through the real binary, over a talk under this
//! checkout's ignored `.build`: every input it cannot judge is refused by name
//! before any model is asked, and the refusal says what to supply.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PROBIERZ: &str = env!("CARGO_BIN_EXE_probierz");

fn workspace(name: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../.build/evaluate-talk")
        .join(format!("{name}-{}", std::process::id()));
    fs::create_dir_all(root.join("slides")).expect("create the talk workspace");
    fs::write(
        root.join("script.md"),
        "# Talk\n\n## 1. Cover (1 min, from minute 0)\n\nHello.\n",
    )
    .expect("write the script");
    fs::write(
        root.join("source.md"),
        "The application asks for funding.\n",
    )
    .expect("write the source");
    fs::write(root.join("slides/slide-1.png"), b"png").expect("write slide one");
    root
}

fn evaluate(root: &Path, extra: &[&str]) -> Output {
    let slides = root.join("slides");
    let script = root.join("script.md");
    let mut words = vec![
        "evaluate".to_string(),
        "talk".to_string(),
        "--slides".to_string(),
        slides.display().to_string(),
        "--script".to_string(),
        script.display().to_string(),
    ];
    words.extend(extra.iter().map(|word| word.to_string()));
    Command::new(PROBIERZ)
        .args(&words)
        .env_remove("PROBIERZ_TALK_MODEL")
        .env_remove("STADO_MODEL_ROUTER_TOKEN")
        .output()
        .expect("start probierz")
}

fn refusal(output: &Output) -> String {
    assert!(
        !output.status.success(),
        "the evaluation passed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn evaluate_lists_talk() {
    let help = Command::new(PROBIERZ)
        .args(["evaluate", "--help"])
        .output()
        .expect("start probierz");
    assert!(
        help.status.success(),
        "{}",
        String::from_utf8_lossy(&help.stderr)
    );
    assert!(String::from_utf8_lossy(&help.stdout).contains("talk"));
}

#[test]
fn a_talk_without_a_source_is_refused() {
    let root = workspace("no-source");
    let said = refusal(&evaluate(&root, &[]));
    assert!(said.contains("--source <document> is required"), "{said}");
    fs::remove_dir_all(root).expect("remove the talk workspace");
}

#[test]
fn a_gap_in_the_slides_is_refused_naming_the_missing_slide() {
    let root = workspace("gap");
    fs::write(root.join("slides/slide-3.png"), b"png").expect("write slide three");
    let source = root.join("source.md").display().to_string();
    let said = refusal(&evaluate(&root, &["--source", &source]));
    assert!(said.contains("has no slide-2.png"), "{said}");
    fs::remove_dir_all(root).expect("remove the talk workspace");
}

#[test]
fn two_pictures_of_one_slide_are_refused_as_a_repeat_not_a_gap() {
    let root = workspace("repeat");
    fs::write(root.join("slides/slide-01.png"), b"png").expect("write slide one again");
    let source = root.join("source.md").display().to_string();
    let said = refusal(&evaluate(&root, &["--source", &source]));
    assert!(said.contains("two pictures of slide 1"), "{said}");
    assert!(
        !said.contains("has no slide-2.png"),
        "a repeat was reported as a gap: {said}"
    );
    fs::remove_dir_all(root).expect("remove the talk workspace");
}

#[test]
fn a_picture_not_named_as_a_slide_is_refused() {
    let root = workspace("unnamed");
    fs::write(root.join("slides/cover.png"), b"png").expect("write a stray picture");
    let source = root.join("source.md").display().to_string();
    let said = refusal(&evaluate(&root, &["--source", &source]));
    assert!(
        said.contains("cover.png is not named slide-<number>.png"),
        "{said}"
    );
    fs::remove_dir_all(root).expect("remove the talk workspace");
}

#[test]
fn a_source_of_another_type_is_refused() {
    let root = workspace("source-type");
    let source = root.join("source.docx");
    fs::write(&source, b"docx").expect("write the source");
    let source = source.display().to_string();
    let said = refusal(&evaluate(&root, &["--source", &source]));
    assert!(
        said.contains("must be Markdown, text, TeX or PDF"),
        "{said}"
    );
    fs::remove_dir_all(root).expect("remove the talk workspace");
}

#[test]
fn a_talk_without_a_model_is_refused_before_anything_is_sent() {
    let root = workspace("no-model");
    let source = root.join("source.md").display().to_string();
    let said = refusal(&evaluate(&root, &["--source", &source]));
    assert!(said.contains("--model or PROBIERZ_TALK_MODEL"), "{said}");
    fs::remove_dir_all(root).expect("remove the talk workspace");
}
