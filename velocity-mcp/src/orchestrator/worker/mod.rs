pub mod artifacts;
pub mod runner;
pub mod scope;
pub mod types;
pub mod worktree;

pub use runner::*;
pub use types::*;

#[cfg(test)]
mod tests {
    use super::artifacts::*;
    use super::scope::*;
    #[allow(unused_imports)]
    use super::types::*;
    use super::*;
    use crate::agent::AiProvider;
    use crate::automation::instruction_registry::AgentTaskKind;
    use crate::automation::task_router::RoutedModelRoute;
    use crate::automation::MediatorArena;
    use crate::orchestrator::blueprint::Task;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;
    use velocity_ide::site_map::SiteMap;

    #[test]
    fn detects_new_files_inside_directory_scope() {
        let workspace = tempdir().unwrap();
        let workspace_root = workspace.path();
        fs::create_dir_all(workspace_root.join("src")).unwrap();
        let snapshot_root = workspace_root.join("snapshot");
        fs::create_dir_all(&snapshot_root).unwrap();

        let scoped_paths = collect_scoped_paths(workspace_root, &["src".to_string()]);
        let before = snapshot_scope(&scoped_paths, workspace_root, &snapshot_root).unwrap();

        fs::write(
            workspace_root.join("src").join("new_file.rs"),
            "fn main() {}\n",
        )
        .unwrap();

        let (changed, created, deleted): (Vec<String>, Vec<String>, Vec<String>) =
            detect_scoped_changes(&scoped_paths, &before, workspace_root).unwrap();
        assert!(changed.is_empty());
        assert!(deleted.is_empty());
        assert_eq!(
            created,
            vec![PathBuf::from("src")
                .join("new_file.rs")
                .display()
                .to_string()]
        );
    }

    #[test]
    fn ignores_new_files_outside_exact_file_scope() {
        let workspace = tempdir().unwrap();
        let workspace_root = workspace.path();
        fs::create_dir_all(workspace_root.join("src")).unwrap();
        let snapshot_root = workspace_root.join("snapshot");
        fs::create_dir_all(&snapshot_root).unwrap();

        let scoped_paths = collect_scoped_paths(workspace_root, &["src/lib.rs".to_string()]);
        let before = snapshot_scope(&scoped_paths, workspace_root, &snapshot_root).unwrap();

        fs::write(
            workspace_root.join("src").join("new_file.rs"),
            "pub fn helper() {}\n",
        )
        .unwrap();

        let (changed, created, deleted): (Vec<String>, Vec<String>, Vec<String>) =
            detect_scoped_changes(&scoped_paths, &before, workspace_root).unwrap();
        assert!(changed.is_empty());
        assert!(created.is_empty());
        assert!(deleted.is_empty());
    }

    #[test]
    fn detects_out_of_scope_created_files() {
        let workspace = tempdir().unwrap();
        let workspace_root = workspace.path();
        fs::create_dir_all(workspace_root.join("src")).unwrap();
        fs::create_dir_all(workspace_root.join("docs")).unwrap();
        let scoped_paths = collect_scoped_paths(workspace_root, &["src".to_string()]);
        let before_workspace = collect_workspace_files(workspace_root).unwrap();

        fs::write(
            workspace_root.join("src").join("in_scope.rs"),
            "fn scoped() {}\n",
        )
        .unwrap();
        fs::write(workspace_root.join("docs").join("rogue.md"), "rogue\n").unwrap();
        fs::create_dir_all(workspace_root.join(".velocity").join("agentic")).unwrap();
        fs::write(
            workspace_root
                .join(".velocity")
                .join("agentic")
                .join("ignored.txt"),
            "ignore\n",
        )
        .unwrap();

        let out_of_scope =
            detect_out_of_scope_created_files(&scoped_paths, &before_workspace, workspace_root)
                .unwrap();

        assert_eq!(
            out_of_scope,
            vec![PathBuf::from("docs").join("rogue.md").display().to_string()]
        );
    }

    #[test]
    fn acquires_directory_scope_locks_for_worker_runs() {
        let workspace = tempdir().unwrap();
        let workspace_root = workspace.path();
        let workspace_root_buf = workspace_root.to_path_buf();
        fs::create_dir_all(workspace_root.join("src")).unwrap();
        fs::write(
            workspace_root.join("src").join("lib.rs"),
            "pub fn demo() {}\n",
        )
        .unwrap();

        let mediator = std::sync::Arc::new(MediatorArena::new());
        let site_map = SiteMap::open(workspace_root, 0).unwrap();
        let locked = acquire_scope_locks(
            workspace_root,
            &["src".to_string()],
            &mediator,
            &site_map,
            crate::orchestrator::TaskId(7),
        )
        .unwrap();
        assert_eq!(locked, vec![workspace_root.join("src")]);

        let conflict = mediator.acquire_lock(
            workspace_root.join("src").join("lib.rs"),
            (1, usize::MAX / 4),
            "task-8".to_string(),
            &site_map,
        );
        assert!(conflict.is_err());

        for scope in &locked {
            mediator.release_lock(scope, "task-7");
        }

        let reacquired = acquire_scope_locks(
            &workspace_root_buf,
            &["src".to_string()],
            &mediator,
            &site_map,
            crate::orchestrator::TaskId(9),
        );
        assert!(reacquired.is_ok());
    }

    #[test]
    fn writes_execution_contract_as_nda() {
        let workspace = tempdir().unwrap();
        let assignment = WorkerAssignment {
            task: Task {
                id: crate::orchestrator::TaskId(1),
                title: "demo".to_string(),
                description: "demo task".to_string(),
                scope: vec!["src/main.rs".to_string()],
                dependencies: Vec::new(),
                output: None,
            },
            task_kind: AgentTaskKind::DesktopAutomation,
            workspace_root: workspace.path().to_path_buf(),
            instructions: "step one\nstep two".to_string(),
            planned_site_map_root: 42,
            provider: AiProvider::CloudflareWorkersAi,
            provider_label: "Workers AI".to_string(),
            model_id: "@cf/meta/llama-3.1-8b-instruct".to_string(),
            model_label: "Llama 3.1 8B".to_string(),
            thinking: true,
            fallback_chain: vec![RoutedModelRoute {
                provider: AiProvider::OpenRouter,
                model_id: "openrouter/sonnet".to_string(),
                model_label: "Sonnet".to_string(),
                thinking: false,
                score: 7,
            }],
            scoped_files: None,
        };

        write_execution_contract_artifacts(workspace.path(), &assignment).unwrap();
        let nda = fs::read_to_string(workspace.path().join("instructions.nda")).unwrap();
        let txt = fs::read_to_string(workspace.path().join("instructions.txt")).unwrap();

        assert!(nda.starts_with("worker-execution-contract version 2\n"));
        assert!(nda.contains("field\ttask_kind\tdesktop_automation"));
        assert!(nda.contains("field\tprovider\tWorkers AI"));
        assert!(nda.contains("field\tthinking\ttrue"));
        assert!(nda.contains("field\ttask_id\t1"));
        assert!(nda.contains("field\tplanned_site_map_root\t000000000000002a"));
        assert!(nda.contains("scope\t0\tsrc/main.rs"));
        assert!(nda.contains("fallback_route\t0"));
        assert!(nda.contains("fallback_route_field\t0\tprovider\tOpenRouter"));
        assert!(nda.contains("fallback_route_field\t0\tmodel\tSonnet"));
        assert!(nda.contains("instruction_line_count 2"));
        assert!(nda.contains("instruction_line\t0\tstep one"));
        assert!(nda.contains("instruction_line\t1\tstep two"));
        assert!(txt.contains("provider: Workers AI"));
        assert!(txt.contains("thinking: true"));
    }

    #[test]
    fn writes_execution_facts_as_nda() {
        let workspace = tempdir().unwrap();
        let outcome = ExecutionOutcome {
            success: true,
            task_kind: AgentTaskKind::DesktopAutomation,
            provider_label: "Workers AI".to_string(),
            model_label: "Llama 3.1 8B".to_string(),
            changed_files: vec!["src/main.rs".to_string()],
            created_files: vec!["src/new.rs".to_string()],
            deleted_files: vec!["src/old.rs".to_string()],
            out_of_scope_created_files: vec!["docs/rogue.md".to_string()],
            transcript: "done".to_string(),
            status_updates: vec!["updated scope".to_string()],
            attempts: vec![WorkerAttempt {
                provider_label: "Workers AI".to_string(),
                model_label: "Llama 3.1 8B".to_string(),
                model_id: "@cf/meta/llama-3.1-8b-instruct".to_string(),
                success: true,
                message: "Changed 1 file".to_string(),
            }],
            message: "Changed 1, created 1, deleted 1 via Workers AI / Llama 3.1 8B".to_string(),
        };

        write_execution_facts(workspace.path(), &outcome).unwrap();
        let facts = fs::read_to_string(workspace.path().join("facts.nda")).unwrap();

        assert!(facts.starts_with("worker-run-facts version 2\n"));
        assert!(facts.contains("field\ttask_kind\tdesktop_automation"));
        assert!(facts.contains("field\tresult\tsuccess"));
        assert!(facts.contains("field\tprovider\tWorkers AI"));
        assert!(facts.contains("changed_file\t0\tsrc/main.rs"));
        assert!(facts.contains("out_of_scope_created_file\t0\tdocs/rogue.md"));
        assert!(facts.contains("attempt\t0"));
        assert!(facts.contains("attempt_field\t0\tresult\tsuccess"));
        assert!(facts.contains("status\t0\tupdated scope"));
        assert!(facts.contains("wa_field\tevidence_lane\tdesktop_automation"));
        assert!(facts.contains("wa_field\tartifact_summary_present\ttrue"));
        assert!(facts.contains("transcript_line\t0\tdone"));
    }
}
