//! End-to-end: `lab loop` measures, evaluates, signals once, retries failed
//! sends, runs in the background, and feeds `lab brief`; `lab evaluate`
//! gates `verified`.

use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

fn lab(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lab"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run lab")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn read(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(dir.join(rel)).unwrap_or_default()
}

const LOCK: &str = ".research/lab/loop/lock";

/// Takes the loop's lock as another loop would, or `None` while a loop holds it.
fn take_lock(dir: &Path) -> Option<std::fs::File> {
    let path = dir.join(LOCK);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .unwrap();
    match f.try_lock() {
        Ok(()) => Some(f),
        Err(std::fs::TryLockError::WouldBlock) => None,
        Err(std::fs::TryLockError::Error(e)) => panic!("lock: {e}"),
    }
}

fn locked(dir: &Path) -> bool {
    take_lock(dir).is_none()
}

const CONFIG: &str = r#"[project]
name = "demo"
timezone = "UTC"

[lab]
agent_findings = "agent/findings.jsonl"

[lab.evaluate]
min_sessions = 5
baseline_days = 0

[lab.loop]
measure = [["sh", "measure.sh"]]
notify = ["sh", "notify.sh"]
"#;

/// The measure command fails while `measure-fails` exists and sleeps while `slow` exists.
const MEASURE: &str = "echo measured\n\
    if [ -f measure-fails ]; then echo 'scan broke' >&2; exit 4; fi\n\
    if [ -f slow ]; then sleep 3; fi\n";

/// The notify command appends each signal to sent.jsonl and answers with a
/// ref, or fails while `notify-fails` exists.
const NOTIFY: &str = "if [ -f notify-fails ]; then cat > /dev/null; echo 'board is down' >&2; exit 3; fi\n\
    cat >> sent.jsonl\n\
    echo\n\
    echo board:7\n";

fn lesson(id: &str, rule: &str) -> String {
    format!(
        "---\nid: {id}\ntitle: Lesson {id}\nkind: bad\nstatus: adopted\nscope: workflow\n\
         rule: Do {id}.\nmetric:\n  tool: agentprof\n  rule: {rule}\ncreated: 2026-01-01\n\
         updated: 2026-01-10\nadopted: 2026-01-10\n---\n\nBody.\n"
    )
}

/// A project with two adopted lessons: `fewer-rereads`, whose rule appears in
/// half of the 100 sessions before adoption and none of the 100 after, and
/// `never-seen`, whose rule never appears. A third rule recurs without a lesson.
fn project(d: &Path) {
    write(d, "research.toml", CONFIG);
    write(d, "measure.sh", MEASURE);
    write(d, "notify.sh", NOTIFY);
    write(
        d,
        "lessons/fewer-rereads.md",
        &lesson("fewer-rereads", "file-reread"),
    );
    write(
        d,
        "lessons/never-seen.md",
        &lesson("never-seen", "never-fires"),
    );
    let (mut sessions, mut findings) = (String::new(), String::new());
    for (side, date) in [("b", "2026-01-05"), ("a", "2026-01-15")] {
        for i in 0..100 {
            let id = format!("{side}{i}");
            sessions += &format!(
                "{{\"schema\":\"rf.session/1\",\"source\":\"claude\",\"id\":\"{id}\",\"role\":\"main\",\"start\":\"{date}T10:00:00Z\",\"max_prompt_tokens\":{}}}\n",
                1000 + i
            );
            let mut finding = |rule: &str| {
                findings += &format!(
                    "{{\"schema\":\"rf.finding/1\",\"tool\":\"agentprof\",\"rule\":\"{rule}\",\"severity\":\"warn\",\"subject\":\"claude:{id}\",\"message\":\"m\"}}\n"
                );
            };
            if side == "b" && i % 2 == 0 {
                finding("file-reread");
            }
            if i < 3 {
                finding("sleep-poll-loop");
            }
        }
    }
    write(d, "agent/sessions.jsonl", &sessions);
    write(d, "agent/findings.jsonl", &findings);
}

fn sent(d: &Path) -> Vec<serde_json::Value> {
    read(d, "sent.jsonl")
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn last(d: &Path) -> serde_json::Value {
    serde_json::from_str(&read(d, ".research/lab/loop/last.json")).unwrap()
}

#[test]
fn the_loop_signals_once_and_feeds_the_brief() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    project(d);

    let brief = stdout(&lab(d, &["brief"]));
    assert!(
        brief.contains("## Lesson measurements\n\nThe loop has never run: run `lab loop`."),
        "{brief}"
    );
    assert!(
        !brief.contains("not yet verified"),
        "the loop replaces the reminder:\n{brief}"
    );

    let out = lab(d, &["loop"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    let signals = sent(d);
    let ids: Vec<&str> = signals.iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!(
        ids,
        [
            "lesson-verifiable:fewer-rereads",
            "lesson-unmeasurable:never-seen",
            "uncovered-rules"
        ]
    );
    let v = &signals[0];
    assert_eq!(v["schema"], "rf.signal/1");
    assert_eq!(v["project"], "demo");
    assert_eq!(v["lesson"], "fewer-rereads");
    assert_eq!(
        (v["repeat"].as_bool(), v["reopened"].as_bool()),
        (Some(false), Some(false))
    );
    let body = v["body"].as_str().unwrap();
    assert!(
        body.contains("lab lesson set fewer-rereads verified --note"),
        "{body}"
    );
    assert!(
        body.contains("50 of 100 sessions (50.0%)") && body.contains("0 of 100 sessions (0.0%)"),
        "{body}"
    );
    assert!(
        signals[2]["body"]
            .as_str()
            .unwrap()
            .contains("agentprof:sleep-poll-loop")
    );
    assert!(v["title"].as_str().unwrap().chars().count() <= 72);

    let rec = last(d);
    assert_eq!(rec["schema"], "rf.loop/1");
    assert_eq!(rec["measure"][0]["ok"], true);
    assert_eq!(rec["evaluations"].as_array().unwrap().len(), 2);
    assert!(
        rec["signals"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["status"] == "notified")
    );
    let state = read(d, ".research/lab/loop/signals.json");
    assert!(
        state.contains("\"ref\": \"board:7\""),
        "the first non-empty stdout line: {state}"
    );
    assert!(read(d, ".research/lab/loop/loop.log").contains("measured"));
    assert!(!locked(d));

    // Nothing changed: nothing is sent again.
    let again = lab(d, &["loop", "--format", "jsonl"]);
    assert_eq!(again.status.code(), Some(0), "{}", stderr(&again));
    assert_eq!(sent(d).len(), 3);
    assert_eq!(
        stdout(&again).lines().count(),
        3,
        "jsonl prints the current signals"
    );
    assert!(
        last(d)["signals"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["status"] == "unchanged")
    );

    let brief = stdout(&lab(d, &["brief"]));
    assert!(brief.contains("Last measured "), "{brief}");
    assert!(
        brief.contains("- `fewer-rereads`: improved p<0.001 → verify"),
        "{brief}"
    );
    assert!(
        brief.contains("- `never-seen`: unmeasurable: metric never observed"),
        "{brief}"
    );
    assert!(!brief.contains("hooks"), "fresh: {brief}");

    let eval = stdout(&lab(d, &["evaluate", "--format", "jsonl"]));
    let rows: Vec<serde_json::Value> = eval
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["schema"], "rf.evaluation/2");
    assert_eq!(rows[0]["verdict"], "improved");
    assert_eq!(rows[0]["method"], "stratified-hypergeometric");
    assert_eq!(rows[1]["verdict"], "unmeasurable");

    // The verified gate follows the verdict.
    let refused = lab(
        d,
        &["lesson", "set", "never-seen", "verified", "--note", "x"],
    );
    assert_eq!(refused.status.code(), Some(2));
    assert!(
        stderr(&refused).contains("says unmeasurable"),
        "{}",
        stderr(&refused)
    );
    let ok = lab(
        d,
        &[
            "lesson",
            "set",
            "fewer-rereads",
            "verified",
            "--note",
            "p<0.001",
        ],
    );
    assert!(ok.status.success(), "{}", stderr(&ok));

    // The brief stops asking to verify at once, before the next loop.
    let brief = stdout(&lab(d, &["brief"]));
    assert!(
        brief.contains("- `fewer-rereads`: improved") && !brief.contains("→ verify"),
        "{brief}"
    );

    // A lesson verified on other evidence no longer asks for a metric.
    let forced = lab(
        d,
        &[
            "lesson",
            "set",
            "never-seen",
            "verified",
            "--force",
            "--note",
            "other evidence",
        ],
    );
    assert!(forced.status.success(), "{}", stderr(&forced));
    assert!(lab(d, &["loop"]).status.success());
    let state: serde_json::Value =
        serde_json::from_str(&read(d, ".research/lab/loop/signals.json")).unwrap();
    for id in [
        "lesson-unmeasurable:never-seen",
        "lesson-verifiable:fewer-rereads",
    ] {
        assert!(
            state["signals"][id]["resolved"].is_string(),
            "{id}: {state}"
        );
    }
}

#[test]
fn failed_measures_and_sends_are_reported_and_retried() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    project(d);
    write(d, "notify-fails", "");
    write(d, "measure-fails", "");
    let out = lab(d, &["loop"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    let rec = last(d);
    assert_eq!(rec["measure"][0]["ok"], false);
    assert_eq!(rec["measure"][0]["status"], "exit 4");
    let statuses: Vec<&str> = rec["signals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["status"].as_str().unwrap())
        .collect();
    assert_eq!(statuses, ["failed"; 4], "{rec}");
    assert!(
        rec["errors"][0].as_str().unwrap().contains("board is down"),
        "{rec}"
    );
    let brief = stdout(&lab(d, &["brief"]));
    assert!(
        brief.contains("Measure commands failed: `sh measure.sh` (exit 4)."),
        "{brief}"
    );
    assert!(brief.contains("4 signal(s) could not be sent"), "{brief}");

    // The next loop retries every unsent signal; the measure failure is resolved.
    std::fs::remove_file(d.join("notify-fails")).unwrap();
    std::fs::remove_file(d.join("measure-fails")).unwrap();
    let out = lab(d, &["loop"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    let ids: Vec<String> = sent(d)
        .iter()
        .map(|s| s["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids.len(), 3, "{ids:?}");
    assert!(!ids.iter().any(|i| i.starts_with("measure-failed")));
    let state = read(d, ".research/lab/loop/signals.json");
    assert!(
        state.contains("\"measure-failed:0\"") && state.contains("\"resolved\": \"20"),
        "{state}"
    );
    // Its notice never went out; the record and the brief say it is dropped.
    assert_eq!(
        last(d)["resolved"],
        serde_json::json!([{"id": "measure-failed:0", "unsent": true}])
    );
    let brief = stdout(&lab(d, &["brief"]));
    assert!(
        brief.contains("`measure-failed:0` (its notice was never sent and is dropped)"),
        "{brief}"
    );

    // It comes back: never sent, so new rather than reopened.
    write(d, "measure-fails", "");
    assert_eq!(lab(d, &["loop"]).status.code(), Some(1));
    let back = sent(d).pop().unwrap();
    assert_eq!(back["id"], "measure-failed:0");
    assert_eq!(back["reopened"], false);
    assert!(back["body"].as_str().unwrap().contains("scan broke"));

    // Sent this time; when it resolves, the brief names its task.
    std::fs::remove_file(d.join("measure-fails")).unwrap();
    lab(d, &["loop"]);
    let r = &last(d)["resolved"][0];
    assert_eq!(r["id"], "measure-failed:0");
    assert!(r["ref"].is_string() && r.get("unsent").is_none(), "{r}");
}

#[test]
fn if_older_skips_and_a_held_lock_blocks_but_a_leftover_file_does_not() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    project(d);
    let held = take_lock(d).unwrap();
    let busy = lab(d, &["loop"]);
    assert_eq!(busy.status.code(), Some(0));
    assert!(stdout(&busy).contains("another loop is running"));
    assert!(!d.join(".research/lab/loop/last.json").exists());
    drop(held);

    // The file a finished or killed loop leaves behind is not a lock.
    assert!(d.join(LOCK).exists());
    let ran = lab(d, &["loop"]);
    assert_eq!(ran.status.code(), Some(0), "{}", stderr(&ran));
    assert!(!locked(d));
    let finished = last(d)["finished"].clone();

    let skipped = lab(d, &["loop", "--if-older", "60"]);
    assert_eq!(skipped.status.code(), Some(0));
    assert_eq!(stdout(&skipped), "", "quiet");
    assert_eq!(last(d)["finished"], finished, "not run again");
    assert_eq!(sent(d).len(), 3);
}

#[test]
fn background_returns_at_once_and_the_child_finishes() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    project(d);
    write(d, "slow", "");
    let started = Instant::now();
    let out = lab(d, &["loop", "--background", "--if-older", "15"]);
    let took = started.elapsed();
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(took < Duration::from_millis(900), "returned after {took:?}");
    assert!(
        !d.join(".research/lab/loop/last.json").exists(),
        "the child is still measuring"
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    while !d.join(".research/lab/loop/last.json").exists() {
        assert!(Instant::now() < deadline, "the child never wrote last.json");
        std::thread::sleep(Duration::from_millis(100));
    }
    // Wait for the child to release its lock after writing last.json.
    while locked(d) {
        assert!(
            Instant::now() < deadline,
            "the child never released its lock"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(last(d)["measure"][0]["ok"], true);
    let log = read(d, ".research/lab/loop/loop.log");
    assert!(
        log.contains("measured") && log.contains("Signals: 3 notified."),
        "{log}"
    );
}

#[test]
fn a_lab_error_is_recorded_and_releases_the_lock() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    project(d);
    write(d, "lessons/broken.md", "no front matter\n");
    let out = lab(d, &["loop"]);
    assert_eq!(out.status.code(), Some(2), "{}", stdout(&out));
    assert!(stderr(&out).contains("broken.md"), "{}", stderr(&out));
    assert!(!locked(d));
    let rec = last(d);
    assert!(
        rec["errors"][0].as_str().unwrap().contains("broken.md"),
        "{rec}"
    );
    let brief = stdout(&lab(d, &["brief"]));
    assert_eq!(brief, "", "the brief itself needs valid lessons");
}

#[test]
fn an_unreadable_signal_state_is_recorded_and_backs_off() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    project(d);
    write(d, ".research/lab/loop/signals.json", "{ not json");
    let out = lab(d, &["loop"]);
    assert_eq!(out.status.code(), Some(2), "{}", stdout(&out));
    let rec = last(d);
    assert!(
        rec["errors"][0].as_str().unwrap().contains("signals.json"),
        "{rec}"
    );
    assert_eq!(sent(d).len(), 0, "nothing sent without the dedupe state");
    assert!(!locked(d));
    let skipped = lab(d, &["loop", "--if-older", "60"]);
    assert_eq!(skipped.status.code(), Some(0));
    assert_eq!(last(d)["finished"], rec["finished"], "backed off");
}

#[test]
fn an_unreadable_loop_record_is_reported_and_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    project(d);
    write(d, ".research/lab/loop/last.json", "{ not json");
    // The record cannot show a recent loop, so `--if-older` runs it.
    let out = lab(d, &["loop", "--if-older", "60"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("Error: the previous loop record was unreadable"),
        "{}",
        stdout(&out)
    );
    let rec = last(d);
    let note = rec["errors"][0].as_str().unwrap();
    assert!(
        note.starts_with("the previous loop record was unreadable (")
            && note.ends_with("); this loop replaces it"),
        "{rec}"
    );
    assert!(read(d, ".research/lab/loop/loop.log").contains(note));
    let brief = stdout(&lab(d, &["brief"]));
    assert!(
        brief.contains("Before the last loop, the previous loop record"),
        "{brief}"
    );
    assert!(!brief.contains("The loop failed"), "{brief}");
    // Now readable: the next `--if-older` skips.
    let skipped = lab(d, &["loop", "--if-older", "60"]);
    assert_eq!(stdout(&skipped), "", "quiet");
    assert_eq!(last(d)["finished"], rec["finished"]);
}

/// Runs `program` in `dir` with git confined to repositories below `ceiling`
/// and free of the user's configuration.
fn isolated(program: &str, dir: &Path, ceiling: &Path, args: &[&str]) -> Output {
    Command::new(program)
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CEILING_DIRECTORIES", ceiling)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("run")
}

fn git(dir: &Path, ceiling: &Path, args: &[&str]) {
    let mut all = vec!["-c", "user.name=t", "-c", "user.email=t@example.com"];
    all.extend_from_slice(args);
    let out = isolated("git", dir, ceiling, &all);
    assert!(out.status.success(), "git {args:?}: {}", stderr(&out));
}

#[test]
fn a_loop_in_a_linked_worktree_runs_for_the_main_checkout() {
    let dir = tempfile::tempdir().unwrap();
    let ceiling = dir.path();
    let (main, linked) = (ceiling.join("main"), ceiling.join("linked"));
    // The project is in a subdirectory, and its measured data is not committed.
    let (m, w) = (main.join("proj"), linked.join("proj"));
    project(&m);
    write(&main, ".gitignore", ".research/\nagent/\nsent.jsonl\n");
    git(&main, ceiling, &["init", "-q"]);
    git(&main, ceiling, &["add", "-A"]);
    git(&main, ceiling, &["commit", "-qm", "project"]);
    git(&main, ceiling, &["worktree", "add", "-q", "../linked"]);
    let lab = |d: &Path, args: &[&str]| isolated(env!("CARGO_BIN_EXE_lab"), d, ceiling, args);

    let out = lab(&w, &["loop"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    assert!(
        stdout(&out).contains("ran the loop for the main checkout"),
        "{}",
        stdout(&out)
    );
    // The main checkout's measures, data, notify command and state served.
    let ids: Vec<String> = sent(&m)
        .iter()
        .map(|s| s["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        ids,
        [
            "lesson-verifiable:fewer-rereads",
            "lesson-unmeasurable:never-seen",
            "uncovered-rules"
        ]
    );
    assert!(read(&m, "measure.out").is_empty());
    assert!(read(&m, ".research/lab/loop/loop.log").contains("linked git worktree"));
    assert!(!w.join("sent.jsonl").exists());
    assert!(!w.join(".research").exists(), "no state in the worktree");

    // One dedupe state and one `--if-older` clock for both checkouts.
    let finished = last(&m)["finished"].clone();
    let skipped = lab(&w, &["loop", "--if-older", "60"]);
    assert_eq!(skipped.status.code(), Some(0));
    assert_eq!(stdout(&skipped), "", "quiet");
    assert_eq!(last(&m)["finished"], finished, "not run again");
    let again = lab(&m, &["loop"]);
    assert_eq!(again.status.code(), Some(0), "{}", stderr(&again));
    assert!(!stdout(&again).contains("worktree"), "{}", stdout(&again));
    assert_eq!(sent(&m).len(), 3, "nothing sent twice");

    // The worktree's brief shows the main checkout's record, not a stale one
    // left in the worktree by an older lab.
    let mut stale = last(&m);
    stale["finished"] = "2026-01-01T00:00:00Z".into();
    stale["evaluations"] = serde_json::json!([]);
    write(&w, ".research/lab/loop/last.json", &stale.to_string());
    let brief = lab(&w, &["brief"]);
    assert_eq!(brief.status.code(), Some(0), "{}", stderr(&brief));
    let text = stdout(&brief);
    let main_root = m.canonicalize().unwrap();
    assert!(
        text.contains(&format!(
            "## Lesson measurements\n\nThis is a linked git worktree: the loop runs for the \
             main checkout {}, and these are its results.\n\nLast measured ",
            main_root.display()
        )),
        "{text}"
    );
    assert!(text.contains("- `never-seen`: unmeasurable"), "{text}");
    assert!(!text.contains("not measured yet"), "{text}");
    assert!(!text.contains("over a day"), "{text}");
    let text = stdout(&lab(&m, &["brief"]));
    assert!(text.contains("- `never-seen`: unmeasurable"), "{text}");
    assert!(!text.contains("worktree"), "{text}");
    std::fs::remove_dir_all(w.join(".research")).unwrap();

    // Without the project in the main checkout, the worktree's loop does nothing,
    // and its brief says there is nothing to show.
    std::fs::remove_file(m.join("research.toml")).unwrap();
    let out = lab(&w, &["loop"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(stdout(&out).contains("did nothing"), "{}", stdout(&out));
    assert!(!w.join(".research").exists());
    assert_eq!(sent(&m).len(), 3);
    let text = stdout(&lab(&w, &["brief"]));
    assert!(
        text.contains("is a linked git worktree, and the main checkout has no ")
            && text.contains(
                "; the loop runs only in a main checkout, so there are no measurements to show.\n"
            ),
        "{text}"
    );
}

#[test]
fn evaluate_in_a_linked_worktree_reads_the_main_checkouts_ignored_agent_scan() {
    let dir = tempfile::tempdir().unwrap();
    let ceiling = dir.path();
    let (main, linked) = (ceiling.join("main"), ceiling.join("linked"));
    let (m, w) = (main.join("proj"), linked.join("proj"));
    project(&m);
    // The agent scan is not committed, so a new worktree has none.
    write(&main, ".gitignore", ".research/\nagent/\n");
    git(&main, ceiling, &["init", "-q", "-b", "main"]);
    git(&main, ceiling, &["add", "-A"]);
    git(&main, ceiling, &["commit", "-qm", "project"]);
    git(&main, ceiling, &["worktree", "add", "-q", "../linked"]);
    assert!(!w.join("agent").exists());
    let lab = |d: &Path, args: &[&str]| isolated(env!("CARGO_BIN_EXE_lab"), d, ceiling, args);
    let verdicts = |o: &Output| -> Vec<String> {
        stdout(o)
            .lines()
            .map(|l| {
                let row: serde_json::Value = serde_json::from_str(l).unwrap();
                row["verdict"].as_str().unwrap().to_string()
            })
            .collect()
    };

    let out = lab(&w, &["evaluate", "--format", "jsonl"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(verdicts(&out), ["improved", "unmeasurable"]);
    let main_root = m.canonicalize().unwrap();
    assert!(
        stderr(&out).contains(&format!(
            "this linked git worktree has no agent/findings.jsonl, which git ignores; reading \
             the main checkout's {}",
            main_root.join("agent/findings.jsonl").display()
        )),
        "{}",
        stderr(&out)
    );
    assert!(!w.join("agent").exists(), "nothing copied");

    // The worktree's own scan wins over the main checkout's.
    write(&w, "agent/findings.jsonl", "");
    write(&w, "agent/sessions.jsonl", "");
    let own = lab(&w, &["evaluate", "--format", "jsonl"]);
    assert_eq!(own.status.code(), Some(0), "{}", stderr(&own));
    assert!(!stderr(&own).contains("main checkout"), "{}", stderr(&own));
    assert_eq!(verdicts(&own), ["unmeasurable", "unmeasurable"]);
    std::fs::remove_dir_all(w.join("agent")).unwrap();

    // A tracked file deleted in the worktree is not looked up in the main checkout.
    write(&main, ".gitignore", ".research/\n");
    git(&main, ceiling, &["add", "-A"]);
    git(&main, ceiling, &["commit", "-qm", "track the scan"]);
    git(&w, ceiling, &["merge", "-q", "--ff-only", "main"]);
    std::fs::remove_dir_all(w.join("agent")).unwrap();
    let tracked = lab(&w, &["evaluate", "--format", "jsonl"]);
    assert_eq!(tracked.status.code(), Some(0), "{}", stderr(&tracked));
    assert!(
        !stderr(&tracked).contains("main checkout"),
        "{}",
        stderr(&tracked)
    );
    assert_eq!(verdicts(&tracked), ["unmeasurable", "unmeasurable"]);
}

#[test]
fn only_the_home_checkout_runs_the_loop_not_a_clone_or_copy() {
    let dir = tempfile::tempdir().unwrap();
    let ceiling = dir.path();
    let (home, clone, copy) = (
        ceiling.join("home"),
        ceiling.join("clone"),
        ceiling.join("copy"),
    );
    let config = |root: &Path| format!("{CONFIG}home = \"{}\"\n", root.display());
    project(&home);
    write(&home, "research.toml", &config(&home));
    // The measured data is not committed, so a clone has none of it.
    write(&home, ".gitignore", ".research/\nagent/\nsent.jsonl\n");
    git(&home, ceiling, &["init", "-q"]);
    git(&home, ceiling, &["add", "-A"]);
    git(&home, ceiling, &["commit", "-qm", "project"]);
    let lab = |d: &Path, args: &[&str]| isolated(env!("CARGO_BIN_EXE_lab"), d, ceiling, args);

    // A clone, such as an experiment's scratch copy, files nothing and keeps no state.
    git(ceiling, ceiling, &["clone", "-q", "home", "clone"]);
    let out = lab(&clone, &["loop"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stdout(&out).contains(&format!(
            "is not the loop's home checkout {} ([lab.loop] home); the loop runs only there, \
             so `lab loop` did nothing.",
            home.display()
        )),
        "{}",
        stdout(&out)
    );
    assert!(sent(&clone).is_empty() && sent(&home).is_empty());
    assert!(!clone.join(".research").exists(), "no state in the clone");
    let out = lab(&clone, &["loop", "--background", "--if-older", "15"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(!clone.join(".research").exists(), "no background loop");
    let text = stdout(&lab(&clone, &["brief"]));
    assert!(
        text.contains("the loop runs only there, so there are no measurements to show."),
        "{text}"
    );

    // A copy outside git does not run it either.
    project(&copy);
    write(&copy, "research.toml", &config(&home));
    let out = lab(&copy, &["loop"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(stdout(&out).contains("did nothing"), "{}", stdout(&out));
    assert!(sent(&copy).is_empty() && !copy.join(".research").exists());

    // The home checkout runs it, and so does a linked worktree, for the home checkout.
    git(&home, ceiling, &["worktree", "add", "-q", "../linked"]);
    let out = lab(&ceiling.join("linked"), &["loop"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("ran the loop for the main checkout"),
        "{}",
        stdout(&out)
    );
    assert_eq!(sent(&home).len(), 3);
    let out = lab(&home, &["loop"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(sent(&home).len(), 3, "nothing sent twice");

    // A home that does not exist here runs the loop nowhere; a relative one is an error.
    write(&copy, "research.toml", &config(&ceiling.join("gone")));
    let out = lab(&copy, &["loop"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("does not exist here"),
        "{}",
        stdout(&out)
    );
    write(
        &copy,
        "research.toml",
        &format!("{CONFIG}home = \"home\"\n"),
    );
    let out = lab(&copy, &["loop"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        stderr(&out).contains("[lab.loop] home must be an absolute path or start with `~/`"),
        "{}",
        stderr(&out)
    );
}
