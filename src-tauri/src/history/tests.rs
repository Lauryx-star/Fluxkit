use super::*;
use tempfile::tempdir;
const START: u64 = 1_800_000_000;
fn project() -> String {
    Uuid::new_v4().to_string()
}
fn candidate() -> Candidate {
    Candidate::new("GET", "https://private-user:private-password@api.example.test/private-path?private-query=private-value#private-fragment")
}
fn add(store: &Store, project: &str, time: u64, elapsed: Option<u64>) {
    let ticket = store.begin(project, candidate(), time).unwrap().unwrap();
    store.finish(ticket, Some(200), elapsed, time).unwrap();
}

#[test]
fn parallel_finishes_do_not_lose_entries_or_overwrite_other_completions() {
    let root = tempdir().unwrap();
    let store = Store::new(root.path().join("history"));
    let id = project();
    store.set_recording(&id, true, START).unwrap();
    let tickets: Vec<_> = (0..8)
        .map(|_| store.begin(&id, candidate(), START).unwrap().unwrap())
        .collect();
    std::thread::scope(|scope| {
        for (i, ticket) in tickets.into_iter().enumerate() {
            let store = store.clone();
            scope.spawn(move || {
                store
                    .finish(ticket, Some(200), Some(i as u64), START)
                    .unwrap()
            });
        }
    });
    assert_eq!(store.view(&id, START).unwrap().entries.len(), 8);
}

#[test]
fn starts_opted_out_and_persists_only_redacted_four_field_metadata() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("history");
    let store = Store::new(root.clone());
    let id = project();
    assert!(!store.view(&id, START).unwrap().recording);
    assert!(store.begin(&id, candidate(), START).unwrap().is_none());
    assert!(!root.exists());
    store.set_recording(&id, true, START).unwrap();
    add(&store, &id, START, Some(12));
    let store = Store::new(root.clone()); // Fresh process-like instance, no resident cache.
    let view = store.view(&id, START + 1).unwrap();
    assert!(view.recording);
    let summary = serde_json::to_value(&view.entries[0]).unwrap();
    assert_eq!(
        summary,
        serde_json::json!({"url":"https://api.example.test/[redacted]?[redacted]=[redacted]","method":"GET","status":200,"elapsedMs":12})
    );
    let raw = fs::read_to_string(root.join(format!("{id}.json"))).unwrap();
    assert!(raw.contains("recordedAt"));
    assert!(!serde_json::to_string(&view).unwrap().contains("recordedAt"));
    for private in [
        "private-user",
        "private-password",
        "private-path",
        "private-query",
        "private-value",
        "private-fragment",
        "headers",
        "body",
        "token",
        "outcome",
    ] {
        assert!(!raw.contains(private), "unexpected stored field");
    }
    assert!(store.view(&project(), START).unwrap().entries.is_empty());
    let files: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(files.len(), 2); // One project document and a private advisory lock, no backup.
}

#[test]
fn enforces_count_and_exact_seven_day_boundary_with_a_deterministic_clock() {
    let root = tempdir().unwrap();
    let store = Store::new(root.path().join("history"));
    let id = project();
    store.set_recording(&id, true, START).unwrap();
    for index in 0..105 {
        add(&store, &id, START + index, Some(index));
    }
    let entries = store.view(&id, START + 105).unwrap().entries;
    assert_eq!(entries.len(), 100);
    assert_eq!(entries[0].elapsed_ms, Some(104));
    assert_eq!(entries[99].elapsed_ms, Some(5));
    let entries = store
        .view(&id, START + MAX_AGE_SECONDS + 5)
        .unwrap()
        .entries;
    assert_eq!(entries.len(), 99); // Entry at second 5 expires at exactly seven days.
    assert_eq!(entries.last().unwrap().elapsed_ms, Some(6));
    let new = Store::new(store.root.clone());
    new.cleanup(START + MAX_AGE_SECONDS + 104).unwrap();
    assert!(new
        .view(&id, START + MAX_AGE_SECONDS + 104)
        .unwrap()
        .entries
        .is_empty());
    let raw = fs::read_to_string(new.path(&id).unwrap()).unwrap();
    assert!(!raw.contains("api.example.test"));
    // A pending completion also prunes, without a preceding view or timer tick.
    let pending_project = project();
    store.set_recording(&pending_project, true, START).unwrap();
    add(&store, &pending_project, START, Some(1));
    let pending = store
        .begin(&pending_project, candidate(), START + 1)
        .unwrap()
        .unwrap();
    store
        .finish(pending, Some(200), Some(2), START + MAX_AGE_SECONDS)
        .unwrap();
    let entries = store
        .view(&pending_project, START + MAX_AGE_SECONDS)
        .unwrap()
        .entries;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].elapsed_ms, Some(2));
    add(&store, &id, START + MAX_AGE_SECONDS + 105, None);
    // Backwards clock adjustment never extends the life of future-dated entries.
    assert!(store.view(&id, START).unwrap().entries.is_empty());
}

#[test]
fn disable_and_clear_preserve_scope_and_invalidate_inflight_completions_across_instances() {
    let root = tempdir().unwrap();
    let store = Store::new(root.path().join("history"));
    let other = Store::new(store.root.clone());
    let (a, b) = (project(), project());
    for id in [&a, &b] {
        store.set_recording(id, true, START).unwrap();
        add(&store, id, START, None);
    }
    let pending = store.begin(&a, candidate(), START).unwrap().unwrap();
    other.set_recording(&a, false, START + 1).unwrap();
    assert_eq!(store.view(&a, START + 1).unwrap().entries.len(), 1);
    assert!(store.begin(&a, candidate(), START + 1).unwrap().is_none());
    other.set_recording(&a, true, START + 2).unwrap();
    store
        .finish(pending, Some(201), Some(50), START + 3)
        .unwrap();
    assert_eq!(store.view(&a, START + 3).unwrap().entries.len(), 1);
    let pending = store.begin(&a, candidate(), START + 3).unwrap().unwrap();
    assert!(other.clear(&a, START + 4).unwrap().recording);
    store
        .finish(pending, Some(202), Some(51), START + 5)
        .unwrap();
    assert!(store.view(&a, START + 5).unwrap().entries.is_empty());
    assert_eq!(store.view(&b, START + 5).unwrap().entries.len(), 1);
    add(&store, &a, START + 6, None);
    store.set_recording(&a, false, START + 7).unwrap();
    store.cleanup(START + MAX_AGE_SECONDS + 7).unwrap();
    assert!(store
        .view(&a, START + MAX_AGE_SECONDS + 7)
        .unwrap()
        .entries
        .is_empty());
}

#[test]
fn absent_status_and_time_stay_absent_and_ties_keep_newest_completion_first() {
    let root = tempdir().unwrap();
    let store = Store::new(root.path().join("history"));
    let id = project();
    store.set_recording(&id, true, START).unwrap();
    let failed = store
        .begin(
            &id,
            Candidate::new("POST", "{{baseUrl}}/private/{{id}}?secret={{token}}"),
            START,
        )
        .unwrap()
        .unwrap();
    store.finish(failed, None, None, START).unwrap();
    add(&store, &id, START, Some(20));
    let entries = store.view(&id, START).unwrap().entries;
    assert_eq!(entries[0].elapsed_ms, Some(20));
    assert_eq!(entries[1].status, None);
    assert_eq!(entries[1].elapsed_ms, None);
    assert_eq!(
        entries[1].url,
        "{{baseUrl}}/[redacted]/{{id}}?[redacted]={{token}}"
    );
    assert_eq!(
        serde_json::to_value(&entries[1]).unwrap()["status"],
        serde_json::Value::Null
    );
}

#[test]
fn corrupt_future_and_unsafe_data_fail_closed_and_only_explicit_clear_resets_them() {
    let root = tempdir().unwrap();
    let store = Store::new(root.path().join("history"));
    let id = project();
    store.set_recording(&id, true, START).unwrap();
    let path = store.path(&id).unwrap();
    let mut valid = serde_json::to_value(store.read(&id).unwrap().unwrap()).unwrap();
    valid["entries"] = serde_json::json!([{"recordedAt":START,"summary":{"url":"https://api.test/private-secret","method":"GET","status":200,"elapsedMs":4}}]);
    for raw in [
        "{".into(),
        "{\"schemaVersion\":99}".into(),
        valid.to_string(),
        "x".repeat(MAX_FILE_BYTES as usize + 1),
    ] {
        fs::write(&path, &raw).unwrap();
        assert!(store.view(&id, START).is_err());
        assert!(store.begin(&id, candidate(), START).is_err());
        assert!(store.cleanup(START + MAX_AGE_SECONDS).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), raw);
    }
    let reset = store.clear(&id, START).unwrap();
    assert!(!reset.recording);
    assert!(reset.entries.is_empty());
    assert!(store.path("../outside").is_err());
}

#[test]
fn private_lock_prevents_overwrites_and_reclaims_only_owned_staging_files() {
    let root = tempdir().unwrap();
    let store = Store::new(root.path().join("history"));
    let id = project();
    store.set_recording(&id, true, START).unwrap();
    let locked = store.lock(true).unwrap();
    assert!(Store::new(store.root.clone())
        .set_recording(&id, false, START)
        .is_err());
    drop(locked);
    let staged = store.root.join(".history-stage-synthetic.json");
    fs::write(&staged, b"synthetic stale metadata").unwrap();
    let unrelated = store.root.join("notes.txt");
    fs::write(&unrelated, b"retained").unwrap();
    store.cleanup(START).unwrap();
    assert!(!staged.exists());
    assert_eq!(fs::read(&unrelated).unwrap(), b"retained");
    // A blocked replacement preserves the previous document and gives an actionable error.
    let blocked = store.path(&project()).unwrap();
    fs::create_dir(&blocked).unwrap();
    assert!(store
        .set_recording(blocked.file_stem().unwrap().to_str().unwrap(), true, START)
        .is_err());
}

#[cfg(unix)]
#[test]
fn symlinks_never_read_or_replace_external_history_targets() {
    use std::os::unix::fs::symlink;
    let root = tempdir().unwrap();
    let outside = root.path().join("outside");
    fs::write(&outside, b"private outside content").unwrap();
    let store = Store::new(root.path().join("history"));
    let id = project();
    store.set_recording(&id, true, START).unwrap();
    fs::remove_file(store.path(&id).unwrap()).unwrap();
    symlink(&outside, store.path(&id).unwrap()).unwrap();
    assert!(store.view(&id, START).is_err());
    assert!(store.clear(&id, START).is_err());
    assert_eq!(fs::read(&outside).unwrap(), b"private outside content");
}
