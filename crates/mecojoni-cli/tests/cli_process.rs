use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    process::{Command, Output},
};

fn fixture(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(relative)
}

fn meco(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_meco"))
        .args(arguments)
        .output()
        .expect("run meco subprocess")
}

fn text(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).expect("CLI output is UTF-8")
}

fn root() -> PathBuf {
    fixture("v2/root.meco.md")
}

fn root_string() -> String {
    root().display().to_string()
}

#[test]
fn every_authoring_command_runs_against_real_filesystem_sources() {
    let root = root_string();
    let v1 = fixture("v1/dialogue.meco");
    let commands: &[&[&str]] = &[
        &["check", &root],
        &["generate", &root, "--data", "playerName=Rin"],
        &["trace", &root, "--data=playerName=Rin"],
        &["lint", &root],
        &["audit", &root, "--samples=8", "--data", "playerName=Rin"],
        &["manifest", &root],
        &["bench", &root, "--count", "4", "--data", "playerName=Rin"],
        &["fmt", &root],
        &["migrate", v1.to_str().unwrap()],
    ];
    for arguments in commands {
        let output = meco(arguments);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}\nstdout={}\nstderr={}",
            arguments.join(" "),
            text(&output.stdout),
            text(&output.stderr)
        );
        assert!(
            !output.stdout.is_empty(),
            "{} produced no report",
            arguments[0]
        );
    }
}

#[test]
fn text_jsonl_trace_and_stream_separation_follow_cli_1() {
    let root = root_string();
    let text_output = meco(&[
        "generate",
        &root,
        "--count=2",
        "--seed",
        "7",
        "--data=playerName=Rin",
    ]);
    assert_eq!(text_output.status.code(), Some(0));
    assert_eq!(text(&text_output.stdout).lines().count(), 2);
    assert!(text_output.stdout.ends_with(b"\n"));
    assert!(text_output.stderr.is_empty());

    let traced = meco(&["trace", &root, "--seed=7", "--data", "playerName=Rin"]);
    assert_eq!(traced.status.code(), Some(0));
    assert!(text(&traced.stdout).contains("Rin"));
    assert!(text(&traced.stderr).contains("trace 1:"));

    let jsonl = meco(&[
        "trace",
        &root,
        "--output=jsonl",
        "--count",
        "2",
        "--data",
        "playerName=Rin",
    ]);
    assert_eq!(jsonl.status.code(), Some(0));
    assert_eq!(text(&jsonl.stdout).lines().count(), 2);
    assert!(text(&jsonl.stdout).lines().all(|line| {
        line.starts_with("{\"cli\":\"cli/1\"") && line.contains("\"selections\":[")
    }));
    assert!(jsonl.stderr.is_empty(), "JSONL trace leaked to stderr");
}

#[test]
fn every_defined_exit_status_and_no_partial_success_are_exercised() {
    let root = root_string();
    let success = meco(&["check", &root]);
    assert_eq!(success.status.code(), Some(0));

    let domain = meco(&["check", fixture("v2/invalid.meco.md").to_str().unwrap()]);
    assert_eq!(domain.status.code(), Some(1));
    assert!(domain.stdout.is_empty());
    assert!(text(&domain.stderr).contains("E_UNDEFINED_RULE"));

    let warning = meco(&["lint", &root, "--deny-warnings"]);
    assert_eq!(warning.status.code(), Some(1));
    assert!(text(&warning.stdout).contains("lint:"));
    assert!(text(&warning.stderr).contains("W_COMPOSITION_SHELL"));

    let missing_data = meco(&["generate", &root, "--count", "3"]);
    assert_eq!(missing_data.status.code(), Some(1));
    assert!(
        missing_data.stdout.is_empty(),
        "generation wrote a partial record"
    );

    let usage = meco(&["generate", &root, "--seed", "--trace"]);
    assert_eq!(usage.status.code(), Some(2));
    assert!(usage.stdout.is_empty());

    let io = meco(&["check", "definitely-not-present.meco.md"]);
    assert_eq!(io.status.code(), Some(2));
    assert!(io.stdout.is_empty());

    let internal = Command::new(env!("CARGO_BIN_EXE_meco"))
        .args(["check", &root])
        .env("MECO_TEST_INTERNAL_ERROR", "1")
        .output()
        .expect("run internal-failure fault injection");
    assert_eq!(internal.status.code(), Some(3));
    assert!(internal.stdout.is_empty());
    assert!(text(&internal.stderr).contains("internal failure"));
}

#[test]
fn formatter_is_byte_stable_and_generation_semantics_are_unchanged() {
    let root = root();
    let output = meco(&["fmt", root.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, fs::read(&root).unwrap());

    let copied = temp_path("formatted.meco.md");
    let formatted = meco(&[
        "fmt",
        root.to_str().unwrap(),
        "--write",
        copied.to_str().unwrap(),
    ]);
    assert_eq!(formatted.status.code(), Some(0));
    assert_eq!(fs::read(&copied).unwrap(), fs::read(&root).unwrap());
    let _ = fs::remove_file(copied);
}

#[test]
fn real_v1_corpus_migrates_compiles_and_reports_honest_differences() {
    let source = fixture("v1/dialogue.meco");
    let migrated = temp_path("dialogue.meco.md");
    let migration = meco(&[
        "migrate",
        source.to_str().unwrap(),
        "--write",
        migrated.to_str().unwrap(),
    ]);
    assert_eq!(migration.status.code(), Some(0));
    let notices = text(&migration.stderr);
    assert!(notices.contains("M_COMMENT_REWRITE"));
    assert!(notices.contains("M_AMBIGUOUS_WHITESPACE"));
    assert!(notices.contains("M_EMPTY_REWRITE"));
    assert!(notices.contains("M_SIGIL_REWRITE"));
    assert!(notices.contains("M_WEIGHT_LOOKING_PROSE"));
    assert!(notices.contains("M_BEHAVIOR_CHANGE"));

    let checked = meco(&["check", migrated.to_str().unwrap()]);
    assert_eq!(checked.status.code(), Some(0), "{}", text(&checked.stderr));

    let mut observed = BTreeSet::new();
    for seed in 0..64_u64 {
        let output = meco(&[
            "generate",
            migrated.to_str().unwrap(),
            "--seed",
            &seed.to_string(),
        ]);
        assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
        observed.insert(text(&output.stdout).trim_end_matches('\n').to_string());
    }
    let expected = BTreeSet::from([
        String::new(),
        "Hello, Ada!".to_string(),
        "Hello, Tomas!".to_string(),
        "Contact Ada@example.invalid for $5 & tea.".to_string(),
        "Contact Tomas@example.invalid for $5 & tea.".to_string(),
        "spaced".to_string(),
        "[status] ready".to_string(),
    ]);
    assert!(observed.is_subset(&expected));
    assert!(
        observed.len() >= 6,
        "seed corpus did not cover migrated alternatives"
    );
    let _ = fs::remove_file(migrated);
}

#[test]
fn jsonl_reports_and_duplicate_scalar_flags_are_stable() {
    let root = root_string();
    for command in ["check", "lint", "audit", "manifest", "bench"] {
        let mut arguments = vec![command, &root, "--output", "jsonl"];
        if matches!(command, "audit" | "bench") {
            arguments.extend(["--data", "playerName=Rin"]);
        }
        let output = meco(&arguments);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{command}: {}",
            text(&output.stderr)
        );
        assert!(text(&output.stdout).starts_with("{\"cli\":\"cli/1\""));
        assert_eq!(text(&output.stdout).lines().count(), 1);
        assert!(
            output.stderr.is_empty(),
            "{command} JSONL leaked diagnostics"
        );
    }
    for (command, source) in [
        ("fmt", fixture("v2/root.meco.md")),
        ("migrate", fixture("v1/dialogue.meco")),
    ] {
        let output = meco(&[command, source.to_str().unwrap(), "--output=jsonl"]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{command}: {}",
            text(&output.stderr)
        );
        assert_eq!(text(&output.stdout).lines().count(), 1);
        assert!(
            output.stderr.is_empty(),
            "{command} JSONL leaked diagnostics"
        );
    }
    let duplicate = meco(&["check", &root, "--output=text", "--output", "jsonl"]);
    assert_eq!(duplicate.status.code(), Some(2));
    assert!(duplicate.stdout.is_empty());
}

#[test]
fn explicit_global_or_command_help_uses_stdout_and_success() {
    for arguments in [&["--help"][..], &["check", "--help"][..]] {
        let output = meco(arguments);
        assert_eq!(output.status.code(), Some(0));
        assert!(text(&output.stdout).starts_with("Usage: meco"));
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn external_message_schema_is_loaded_for_check_and_manifest_export() {
    let root = fixture("messages/root.meco.md");
    let messages = fixture("messages/messages.manifest");
    let checked = meco(&[
        "check",
        root.to_str().unwrap(),
        "--messages",
        messages.to_str().unwrap(),
    ]);
    assert_eq!(checked.status.code(), Some(0), "{}", text(&checked.stderr));

    let manifest = meco(&[
        "manifest",
        root.to_str().unwrap(),
        "--messages",
        messages.to_str().unwrap(),
        "--output=jsonl",
    ]);
    assert_eq!(
        manifest.status.code(),
        Some(0),
        "{}",
        text(&manifest.stderr)
    );
    let report = text(&manifest.stdout);
    assert!(report.contains("\"id\":\"welcome\""));
    assert!(report.contains("\"name\":\"name\",\"type\":\"text\""));
}

#[test]
fn published_single_and_multimodule_examples_run_from_the_filesystem() {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let hello = workspace.join("examples/hello.meco.md");
    let npc = workspace.join("examples/npc/root.meco.md");
    for arguments in [
        vec!["generate", hello.to_str().unwrap(), "--seed=7"],
        vec![
            "generate",
            npc.to_str().unwrap(),
            "--seed=7",
            "--data=playerName=Rin",
        ],
    ] {
        let output = meco(&arguments);
        assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
        assert!(!output.stdout.is_empty());
    }
}

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "mecojoni-cli-{}-{}-{name}",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ))
}
