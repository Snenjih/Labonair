// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use labonair_workspace_storage::{
    SpaceId, StoredTab, TabId, WorkspaceChange, WorkspaceInstanceId, WorkspaceStorage,
};
use uuid::Uuid;

const NOW: i64 = 1_800_000_000;
const LEASE_TTL: Duration = Duration::from_secs(60);
const CHILD_MODE: &str = "LABONAIR_WORKSPACE_PROCESS_MODE";

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is before UNIX epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "labonair-workspace-process-{}-{unique}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        fs::create_dir(&path).expect("create temporary workspace directory");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                .expect("restrict temporary workspace directory permissions");
        }
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn separate_processes_claim_and_join_persisted_workspaces() {
    if std::env::var_os(CHILD_MODE).is_some() {
        run_child().expect("workspace process child operation");
        return;
    }

    let directory = TempDirectory::new();
    let database = directory.path().join("workspace.db");
    let store = WorkspaceStorage::open(&database).expect("initialize shared workspace database");
    drop(store);

    let gate = directory.path().join("start-claims");
    let mut children = Vec::new();
    for index in 0..2 {
        children.push(spawn_child(
            &database,
            "claim",
            &directory.path().join(format!("claim-{index}.result")),
            &directory.path().join(format!("claim-{index}.ready")),
            Some(&gate),
            None,
        ));
    }

    for index in 0..2 {
        wait_for_path(&directory.path().join(format!("claim-{index}.ready")));
    }
    fs::write(&gate, "go").expect("release child claims together");

    let mut claims = Vec::new();
    for (index, child) in children.into_iter().enumerate() {
        wait_child(child, &format!("claim child {index}"));
        let content = fs::read_to_string(directory.path().join(format!("claim-{index}.result")))
            .expect("read child claim result");
        let mut fields = content.lines();
        let instance_id = fields
            .next()
            .expect("claim result instance id")
            .parse::<WorkspaceInstanceId>()
            .expect("valid instance id");
        let space_id = fields
            .next()
            .expect("claim result Space id")
            .parse::<SpaceId>()
            .expect("valid Space id");
        let tab_id = fields
            .next()
            .expect("claim result tab id")
            .parse::<TabId>()
            .expect("valid tab id");
        claims.push((instance_id, space_id, tab_id));
    }
    assert_ne!(claims[0].1, claims[1].1);

    let store = WorkspaceStorage::open(&database).expect("reopen database in parent process");
    let spaces = store.list_spaces(NOW).expect("list durable Spaces");
    assert_eq!(spaces.len(), 2);
    for (_, space_id, expected_tab) in &claims {
        let stored = store
            .load_space(*space_id, NOW)
            .expect("load child-created Space")
            .expect("child-created Space exists");
        assert_eq!(stored.participant_count, 1);
        assert_eq!(stored.tabs.len(), 1);
        assert_eq!(stored.tabs[0].id, *expected_tab);
    }
    drop(store);

    let (_, shared_space, expected_tab) = claims[0];
    let join_result = directory.path().join("join.result");
    let joiner = spawn_child(
        &database,
        "join",
        &join_result,
        &directory.path().join("join.ready"),
        None,
        Some((WorkspaceInstanceId::new(), shared_space)),
    );
    wait_child(joiner, "join child");
    assert_eq!(
        fs::read_to_string(&join_result)
            .expect("read join result")
            .trim(),
        expected_tab.to_string()
    );

    let store =
        WorkspaceStorage::open(&database).expect("reopen database after cross-process join");
    let shared = store
        .load_space(shared_space, NOW)
        .expect("read joined Space")
        .expect("joined Space exists");
    assert_eq!(shared.participant_count, 2);
    assert_eq!(shared.tabs[0].id, expected_tab);
    assert_eq!(shared.revision, 2);
    assert_eq!(shared.name, "Joined from another process");
}

fn spawn_child(
    database: &Path,
    mode: &str,
    result: &Path,
    ready: &Path,
    gate: Option<&Path>,
    join: Option<(WorkspaceInstanceId, SpaceId)>,
) -> Child {
    let mut command = Command::new(std::env::current_exe().expect("current test executable"));
    command
        .args([
            "--exact",
            "separate_processes_claim_and_join_persisted_workspaces",
            "--nocapture",
        ])
        .env(CHILD_MODE, mode)
        .env("LABONAIR_WORKSPACE_PROCESS_DB", database)
        .env("LABONAIR_WORKSPACE_PROCESS_RESULT", result)
        .env("LABONAIR_WORKSPACE_PROCESS_READY", ready)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if let Some(gate) = gate {
        command.env("LABONAIR_WORKSPACE_PROCESS_GATE", gate);
    }
    if let Some((instance, space)) = join {
        command
            .env("LABONAIR_WORKSPACE_PROCESS_INSTANCE", instance.to_string())
            .env("LABONAIR_WORKSPACE_PROCESS_SPACE", space.to_string());
    }
    command.spawn().expect("spawn workspace child process")
}

fn run_child() -> Result<(), Box<dyn std::error::Error>> {
    let mode = std::env::var(CHILD_MODE)?;
    let database = required_path("LABONAIR_WORKSPACE_PROCESS_DB")?;
    let result = required_path("LABONAIR_WORKSPACE_PROCESS_RESULT")?;
    let ready = required_path("LABONAIR_WORKSPACE_PROCESS_READY")?;
    let store = WorkspaceStorage::open(database)?;

    match mode.as_str() {
        "claim" => {
            let gate = required_path("LABONAIR_WORKSPACE_PROCESS_GATE")?;
            fs::write(ready, "ready")?;
            wait_for_path(&gate);
            let instance_id = WorkspaceInstanceId::new();
            let (lease, _) = store.claim_startup(instance_id, NOW, LEASE_TTL, true)?;
            let tab_id = TabId::new();
            store.apply_change(
                &lease,
                0,
                WorkspaceChange::AddTab {
                    tab: StoredTab {
                        id: tab_id,
                        payload: serde_json::json!({"kind": "terminal"}),
                        scrollback_keys: Vec::new(),
                    },
                    position: 0,
                },
                NOW,
            )?;
            fs::write(
                result,
                format!("{instance_id}\n{}\n{tab_id}\n", lease.space_id),
            )?;
        }
        "join" => {
            let instance_id = std::env::var("LABONAIR_WORKSPACE_PROCESS_INSTANCE")?.parse()?;
            let space_id = std::env::var("LABONAIR_WORKSPACE_PROCESS_SPACE")?.parse()?;
            fs::write(ready, "ready")?;
            let (lease, loaded) = store.join_space(instance_id, space_id, NOW, LEASE_TTL)?;
            let tab = loaded
                .tabs
                .first()
                .ok_or("joined Space did not contain its persisted tab")?;
            store.apply_change(
                &lease,
                loaded.revision,
                WorkspaceChange::RenameSpace {
                    name: "Joined from another process".to_owned(),
                },
                NOW,
            )?;
            fs::write(result, tab.id.to_string())?;
        }
        other => return Err(format!("unknown child mode: {other}").into()),
    }
    Ok(())
}

fn required_path(name: &str) -> Result<PathBuf, std::env::VarError> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .ok_or(std::env::VarError::NotPresent)
}

fn wait_for_path(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {}",
            path.display()
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_child(child: Child, label: &str) {
    let output = child.wait_with_output().expect("wait for workspace child");
    assert!(
        output.status.success(),
        "{label} failed with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}
