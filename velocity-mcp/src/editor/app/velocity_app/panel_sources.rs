//! Data sources for the dock panels whose content lives on disk.
//!
//! Every dock panel renders a field on `VelocityApp`, so a panel is only as
//! real as whatever writes that field. Several of them rendered an empty list
//! for the whole session and told the person the reason was something they had
//! not done yet ("add projects to …", "click the Record button …") while the
//! loader that reads exactly that data already existed in the crate and was
//! merely never called from the editor. This module is the single place where a
//! panel's disk source is named, so "which panel reads what" is answerable from
//! one file - and testable, because a refresh method is callable without an
//! egui frame or a running editor.
//!
//! Only disk-backed *read* sources belong here. Panels whose content is typed
//! by the person (favorites, bookmarks) or produced in-session (chat, agent
//! timeline) are written by their own input paths, not by a poll.

use std::time::Duration;

use super::struct_def::VelocityApp;

/// Re-read the disk-backed panel sources at most this often. The panels are
/// opened by hand, so this sits far below "would the person notice a stale row"
/// while keeping the directory scans off the per-frame path.
const PANEL_SOURCE_POLL: Duration = Duration::from_secs(5);

/// How many handoffs the Continuation Ledger panel keeps on screen. Runs older
/// than that stay on disk; the panel is a "what is being carried right now"
/// list, not an archive browser.
const MAX_CONTINUATION_LEDGERS: usize = 12;

impl VelocityApp {
    /// The frame hook beside the other throttled polls. Throttled internally,
    /// so the call site can stay unconditional.
    pub fn poll_disk_panel_sources(&mut self) {
        if self
            .last_panel_source_check
            .is_some_and(|seen| seen.elapsed() < PANEL_SOURCE_POLL)
        {
            return;
        }
        self.last_panel_source_check = Some(std::time::Instant::now());
        self.refresh_skill_files();
        self.refresh_recordings();
        self.refresh_targets();
        self.refresh_continuation_ledgers();
    }

    /// Skill Files panel: the `.nda` skills under `.velocity/skills/` - the
    /// same directory the team subsystem's `list_skills` tool answers from, so
    /// the panel and the agent cannot disagree about what the workspace has.
    pub fn refresh_skill_files(&mut self) {
        self.skill_files = crate::editor::skill_file::list_skill_files(&self.workspace_root);
    }

    /// Recordings panel: the saved WA scripts under `.velocity/wa-scripts/`.
    /// A workspace that has never recorded anything has no such directory, and
    /// listing it must not create one - `wa::list_scripts` would, so the
    /// directory is checked first and an absent one simply means no recordings.
    pub fn refresh_recordings(&mut self) {
        self.recordings = wa_script_names(&self.workspace_root);
    }

    /// Targets panel: the site each saved browser workflow starts from. These
    /// are the only "registered site targets" the workspace actually stores;
    /// the panel used to point at a `.velocity_projects.nda` file that no code
    /// in the crate reads or writes.
    pub fn refresh_targets(&mut self) {
        self.target_entries = browser_target_entries(&self.workspace_root);
    }

    /// Continuation Ledger panel: the handoffs a worker has persisted, newest
    /// write first. `ContinuationLedger::persist` owns the file, so the panel
    /// shows the same bytes a replacing model is handed rather than a parallel
    /// idea of what a ledger is. A workspace that has never handed off a task
    /// has no runs directory, and reading must not create one.
    pub fn refresh_continuation_ledgers(&mut self) {
        self.continuation_ledgers = crate::editor::continuation_ledger::latest_ledger_summaries(
            &self.workspace_root,
            MAX_CONTINUATION_LEDGERS,
        );
    }
}

/// Names of the WA scripts saved in this workspace, in the order the panel
/// shows them. The listing is the same one the `wa_list_scripts` tool answers
/// with, so the panel and the agent cannot disagree - which also means a store
/// that fails to list (one unreadable script, since the shared loader stops at
/// the first error) shows as no recordings at all. That failure is logged.
fn wa_script_names(workspace_root: &std::path::Path) -> Vec<String> {
    if !workspace_root.join(".velocity").join("wa-scripts").is_dir() {
        return Vec::new();
    }
    match crate::wa::list_scripts(
        workspace_root,
        None,
        None,
        crate::wa::model::WaListSortDirection::Asc,
    ) {
        Ok(reports) => reports.into_iter().map(|r| r.script.name).collect(),
        Err(err) => {
            log::warn!("panel sources: WA script listing failed: {err}");
            Vec::new()
        }
    }
}

/// One row per saved browser workflow: the workflow's name labels the site it
/// works against, and its start URL is the target. `browser_list_workflows` is
/// the same source the agent reads, and like that listing one workflow the store
/// cannot parse ends the listing, which is logged rather than shown as a row.
fn browser_target_entries(
    workspace_root: &std::path::Path,
) -> Vec<crate::editor::sidebar_tabs::TargetEntry> {
    if !workspace_root
        .join(".velocity")
        .join("browser-workflows")
        .is_dir()
    {
        return Vec::new();
    }
    match crate::editor::browser::list_workflows(
        workspace_root,
        None,
        None,
        None,
        crate::editor::browser::BrowserListSortDirection::Asc,
    ) {
        Ok(workflows) => workflows
            .into_iter()
            .map(|w| crate::editor::sidebar_tabs::TargetEntry {
                url: w.start_url,
                label: w.name,
                last_visited: None,
            })
            .collect(),
        Err(err) => {
            log::warn!("panel sources: workflow listing failed: {err}");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::skill_file::{save_skill_file, SkillFile};
    use std::path::{Path, PathBuf};

    /// A workspace directory that is unique per test: `VelocityApp::test_stub()`
    /// shares one fixed temp path across threads, so a fixture that writes into
    /// the stub's own root would race with every other test using it.
    fn workspace(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "velocity_panel_src_{tag}_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("fixture workspace");
        dir
    }

    fn app_at(root: PathBuf) -> VelocityApp {
        let mut app = VelocityApp::test_stub();
        app.workspace_root = root;
        app
    }

    fn seed_skill(root: &Path, id: &str, name: &str) {
        save_skill_file(
            root,
            &SkillFile::new(id, name, "seeded for the panel test", "step one"),
        );
    }

    #[test]
    fn the_skill_files_panel_shows_the_skills_on_disk() {
        let root = workspace("skills");
        seed_skill(&root, "netcode", "Netcode Expert");
        seed_skill(&root, "shaders", "Shader Expert");
        let mut app = app_at(root);

        app.refresh_skill_files();

        let ids: Vec<&str> = app.skill_files.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["netcode", "shaders"]);
        assert_eq!(app.skill_files[0].name, "Netcode Expert");
    }

    #[test]
    fn refreshing_clears_skills_that_were_removed_from_disk() {
        // The panel claims to show the workspace's skills, so a refresh must
        // agree with the directory and not with whatever it saw last time:
        // an assignment, never an append.
        let root = workspace("skills_cleared");
        seed_skill(&root, "netcode", "Netcode Expert");
        let mut app = app_at(root.clone());

        app.refresh_skill_files();
        assert_eq!(app.skill_files.len(), 1);

        std::fs::remove_dir_all(crate::editor::skill_file::skills_dir(&root))
            .expect("remove skills dir");
        app.refresh_skill_files();
        assert!(
            app.skill_files.is_empty(),
            "a deleted skill must not linger in the panel"
        );
    }

    #[test]
    fn the_recordings_panel_lists_the_saved_wa_scripts() {
        let root = workspace("recordings");
        crate::wa::save_script_report(
            &root,
            "Checkout flow",
            Some("https://example.test"),
            Vec::new(),
        )
        .expect("save script");
        let mut app = app_at(root);

        app.refresh_recordings();

        assert_eq!(app.recordings, vec!["Checkout flow".to_string()]);
    }

    #[test]
    fn a_workspace_that_never_recorded_has_no_recordings_and_no_new_directory() {
        let root = workspace("recordings_absent");
        let mut app = app_at(root.clone());

        app.refresh_recordings();

        assert!(app.recordings.is_empty());
        assert!(
            !root.join(".velocity").join("wa-scripts").exists(),
            "listing recordings must not create the directory it lists"
        );
    }

    #[test]
    fn the_targets_panel_shows_the_start_url_of_each_saved_workflow() {
        let root = workspace("targets");
        crate::editor::browser::save_workflow(
            &root,
            &crate::editor::browser::BrowserWorkflow {
                name: "Shop".into(),
                start_url: "https://shop.example.test".into(),
                variables: std::collections::HashMap::new(),
                steps: Vec::new(),
            },
        )
        .expect("save workflow");
        let mut app = app_at(root);

        app.refresh_targets();

        assert_eq!(app.target_entries.len(), 1);
        assert_eq!(app.target_entries[0].label, "Shop");
        assert_eq!(app.target_entries[0].url, "https://shop.example.test");
    }

    /// Write one handoff the way the worker does: capture from a real scoped
    /// file, then persist it into that task's run directory.
    fn persist_ledger(root: &Path, task: &str, goal: &str) {
        let source = root.join("src").join("handshake.rs");
        std::fs::create_dir_all(source.parent().expect("source parent")).expect("source dir");
        std::fs::write(&source, "fn handshake() {}\n").expect("source file");
        let ledger = crate::editor::continuation_ledger::ContinuationLedger::capture(
            task,
            goal,
            "refactor",
            &[source],
            root,
            7,
            "Decision: keep the retry loop",
            &[],
            &["step one".to_string()],
            "OpenRouter",
            "Claude Sonnet",
            "claude-x",
            Duration::from_secs(5),
            false,
        );
        let run_dir = root
            .join(".velocity")
            .join("agentic")
            .join("runs")
            .join(task);
        std::fs::create_dir_all(&run_dir).expect("run dir");
        ledger.persist(&run_dir).expect("persist ledger");
    }

    #[test]
    fn the_continuation_ledger_panel_shows_the_handoff_the_worker_persisted() {
        let root = workspace("ledgers");
        persist_ledger(
            &root,
            "task-4",
            "Refactor the TLS handshake\nand keep the retry loop",
        );
        let mut app = app_at(root);

        app.refresh_continuation_ledgers();

        assert_eq!(app.continuation_ledgers.len(), 1);
        let row = &app.continuation_ledgers[0];
        assert_eq!(row.run_name, "task-4");
        assert_eq!(row.id, "task-4-attempt-1");
        assert_eq!(row.goal_first_line, "Refactor the TLS handshake");
        assert_eq!(row.goal_lines, 2, "the multi-line goal lost a line");
        assert_eq!(row.scoped_files, 1);
        assert_eq!(row.attempts[0].model_label, "Claude Sonnet");
    }

    #[test]
    fn a_workspace_that_never_handed_off_has_no_ledgers_and_no_new_directory() {
        let root = workspace("ledgers_absent");
        let mut app = app_at(root.clone());

        app.refresh_continuation_ledgers();

        assert!(app.continuation_ledgers.is_empty());
        assert!(
            !root.join(".velocity").join("agentic").join("runs").exists(),
            "listing the ledgers must not create the run directory it lists"
        );
    }

    #[test]
    fn the_continuation_ledger_panel_names_the_file_it_reads() {
        // The panel rendered `self.continuation_ledger`, an Option no code ever
        // wrote, and its empty state described a ledger "created when handing
        // off" without naming where it went. Both halves are pinned here: the
        // field the poll fills, and the path the text tells the person.
        let panel = std::fs::read_to_string(format!(
            "{}/src/editor/app/velocity_app/tier3_orchestration.rs",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("tier3_orchestration.rs");
        let body = panel
            .split("fn render_continuation_ledger_panel")
            .nth(1)
            .and_then(|rest| rest.split("fn render_background_agents_panel").next())
            .expect("continuation ledger renderer");
        assert!(
            body.contains("self.continuation_ledgers"),
            "the panel renders a field the poll never fills"
        );
        assert!(
            body.contains(".velocity/agentic/runs"),
            "the empty ledger panel must name the file it reads"
        );
    }

    #[test]
    fn the_recordings_and_targets_panels_name_the_stores_they_read() {
        // Both panels used to tell the person to do something that would not
        // fill them: the Recordings text pointed at a toolbar button that never
        // writes `self.recordings`, and the Targets text pointed at a
        // `.velocity_projects.nda` file no code in the crate reads.
        let sidebar = std::fs::read_to_string(format!(
            "{}/src/editor/sidebar_tabs.rs",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("sidebar_tabs.rs");
        assert!(
            !sidebar.contains(".velocity_projects.nda"),
            "the Targets panel points the person at a file no loader reads"
        );
        let recording_hint = sidebar
            .split("fn render_recordings_content")
            .nth(1)
            .and_then(|rest| rest.split("fn render_logs_content").next())
            .expect("recordings renderer");
        assert!(
            recording_hint.contains(".velocity/wa-scripts"),
            "the empty Recordings panel must name the directory it reads"
        );
        let targets_hint = sidebar
            .split("fn render_targets_content")
            .nth(1)
            .and_then(|rest| rest.split("fn render_recordings_content").next())
            .expect("targets renderer");
        assert!(
            targets_hint.contains("browser workflow"),
            "the empty Targets panel must say where its rows come from"
        );
    }

    #[test]
    fn the_poll_runs_every_source_once_and_then_waits_for_the_window() {
        let root = workspace("poll");
        seed_skill(&root, "netcode", "Netcode Expert");
        let mut app = app_at(root.clone());

        app.poll_disk_panel_sources();
        assert_eq!(app.skill_files.len(), 1);

        // Same window: a skill saved now is not visible yet, which is what
        // makes the call cheap enough to sit on the frame path at all.
        seed_skill(&root, "shaders", "Shader Expert");
        app.poll_disk_panel_sources();
        assert_eq!(app.skill_files.len(), 1, "the poll must throttle");

        // Window expired: it is visible, and the other sources ran with it.
        app.last_panel_source_check =
            Some(std::time::Instant::now() - (PANEL_SOURCE_POLL + Duration::from_secs(1)));
        app.poll_disk_panel_sources();
        assert_eq!(app.skill_files.len(), 2);
    }

    #[test]
    fn the_frame_loop_calls_the_poll() {
        // The audited defect was a loader nothing called. A test that only
        // exercises the loader would still pass with the frame hook deleted,
        // so the hook itself is pinned to the frame.
        let frame = std::fs::read_to_string(format!(
            "{}/src/editor/app/velocity_app/ui_render.rs",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("ui_render.rs");
        assert!(
            frame.contains("poll_disk_panel_sources()"),
            "the disk panel sources are never refreshed from the frame loop"
        );
    }

    #[test]
    fn every_panel_refreshed_by_the_poll_assigns_its_field_from_disk() {
        // Guards the shape of the seam: a refresh method added to the poll but
        // left empty would keep the panel sourceless and pass no other test.
        let src = std::fs::read_to_string(format!(
            "{}/src/editor/app/velocity_app/panel_sources.rs",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("panel_sources.rs");
        let poll_body = src
            .split("fn poll_disk_panel_sources")
            .nth(1)
            .expect("poll method")
            .split("\n    }")
            .next()
            .expect("poll body");

        let mut refreshed: Vec<String> = Vec::new();
        for line in src.lines() {
            if let Some(rest) = line.trim().strip_prefix("pub fn refresh_") {
                let name = rest.split('(').next().expect("method name");
                refreshed.push(format!("refresh_{name}"));
            }
        }
        assert!(
            !refreshed.is_empty(),
            "the seam defines no refresh methods, so no panel has a disk source"
        );

        for name in &refreshed {
            assert!(
                poll_body.contains(&format!("self.{name}();")),
                "{name} is defined but the poll never calls it"
            );
            let body = src
                .split(&format!("fn {name}"))
                .nth(1)
                .expect("method text")
                .split("\n    }")
                .next()
                .expect("method body");
            assert!(
                body.lines()
                    .any(|line| line.trim_start().starts_with("self.") && line.contains(" = ")),
                "{name} assigns no VelocityApp field, so the panel it feeds stays sourceless"
            );
        }
    }

    #[test]
    fn the_skills_panels_name_the_directory_the_loader_reads() {
        let on_disk = crate::editor::skill_file::skills_dir(Path::new("/w"))
            .to_string_lossy()
            .replace('\\', "/");
        assert!(on_disk.ends_with("/.velocity/skills"));

        for file in [
            "velocity-mcp/src/editor/app/velocity_app/tier3_plugins.rs",
            "velocity-mcp/src/editor/app/velocity_app/tier3_subpanels.rs",
        ] {
            let src =
                std::fs::read_to_string(format!("{}/../{}", env!("CARGO_MANIFEST_DIR"), file))
                    .unwrap_or_else(|err| panic!("{file}: {err}"));
            assert!(
                !src.contains(".qoder/skills"),
                "{file} tells the person skills live in .qoder/skills/, a directory no loader reads"
            );
        }
    }
}
