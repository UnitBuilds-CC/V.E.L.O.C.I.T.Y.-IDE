//! Favorites: the input path behind the Favorites panel.
//!
//! The panel has always had rows and a remove button, and its empty state told
//! the person to "star files from the editor tab context menu" - a menu with no
//! star item. So `favorite_files` had one mutator in the whole crate (`Clear`,
//! and the per-row X) and no way to add anything: the panel could only ever be
//! emptied. These two methods are that missing input path, and the menu item
//! that calls them is in [`crate::editor::app::render`], the editor tab's
//! context menu, exactly where the panel said it was.
//!
//! Favorites are the person's own choices, not data on disk, so they belong to
//! the workspace's state file rather than to a poll - see
//! [`WorkspacePreferences::favorites`](super::struct_def::WorkspacePreferences).

use std::path::Path;

use super::struct_def::VelocityApp;

/// How many files one workspace can mark as favorites. The panel draws every
/// row it is given, so this is the bound that keeps a long-lived preference
/// list from growing without limit; the oldest star is the one that goes.
pub const MAX_FAVORITES: usize = 64;

impl VelocityApp {
    /// Whether `path` is starred. Compared the way the editor compares every
    /// other path ([`crate::editor::file_ops::same_editor_path`]), because the
    /// tree, the sitemap and the restored preference file do not all spell the
    /// same file the same way on Windows, and a star that misses by a separator
    /// is a duplicate row.
    pub fn is_favorite(&self, path: &Path) -> bool {
        self.favorite_files
            .iter()
            .any(|f| crate::editor::file_ops::same_editor_path(f, path))
    }

    /// Star `path`, or un-star it if it is already starred, and return whether
    /// it is starred afterwards. The newest star goes last; past
    /// [`MAX_FAVORITES`] the oldest is dropped.
    ///
    /// Writing the whole workspace preference file per click is deliberate: the
    /// same call already happens for every sidebar toggle, and a favorite that
    /// only lives until the next crash is a favorite that cannot be trusted.
    pub fn toggle_favorite(&mut self, path: &Path) -> bool {
        if let Some(at) = self
            .favorite_files
            .iter()
            .position(|f| crate::editor::file_ops::same_editor_path(f, path))
        {
            self.favorite_files.remove(at);
            self.save_workspace_preferences();
            return false;
        }
        if self.favorite_files.len() >= MAX_FAVORITES {
            self.favorite_files.remove(0);
        }
        self.favorite_files.push(path.to_path_buf());
        self.save_workspace_preferences();
        true
    }

    /// The panel's "Clear" button. Persisted, because the other two mutators
    /// are and a Clear that comes back on restart is not a Clear.
    pub fn clear_favorites(&mut self) {
        if self.favorite_files.is_empty() {
            return;
        }
        self.favorite_files.clear();
        self.save_workspace_preferences();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "velocity_favorites_{tag}_{}_{}",
            std::process::id(),
            format!("{:?}", std::thread::current().id()).replace(['(', ')', '\"'], "")
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create workspace");
        root
    }

    fn app_at(root: &Path) -> VelocityApp {
        let mut app = VelocityApp::test_stub();
        app.workspace_root = root.to_path_buf();
        app
    }

    fn file_in(root: &Path, name: &str) -> std::path::PathBuf {
        let p = root.join(name);
        std::fs::write(&p, "fn main() {}\n").expect("write file");
        p
    }

    #[test]
    fn starring_a_file_adds_it_and_starring_it_again_removes_it() {
        let root = workspace("toggle");
        let file = file_in(&root, "main.rs");
        let mut app = app_at(&root);

        assert!(app.toggle_favorite(&file), "first click stars");
        assert_eq!(app.favorite_files, vec![file.clone()]);
        assert!(app.is_favorite(&file));

        assert!(!app.toggle_favorite(&file), "second click un-stars");
        assert!(app.favorite_files.is_empty());
        assert!(!app.is_favorite(&file));
    }

    /// Windows volumes fold case and accept both separators, so these three
    /// spellings name the file the first row already shows.
    #[cfg(windows)]
    #[test]
    fn one_file_spelled_two_ways_is_one_row_not_two() {
        let root = workspace("spelling");
        let file = file_in(&root, "main.rs");
        let mut app = app_at(&root);

        // The explorer tree, the sitemap and a hand-typed path do not all spell
        // one file the same way on Windows. Compared exactly, the second click
        // would add a second row for the file the first row already names.
        app.toggle_favorite(&file);
        let slashed = file.to_string_lossy().replace('\\', "/");
        app.toggle_favorite(Path::new(&slashed));
        assert!(
            app.favorite_files.is_empty(),
            "a separator difference un-starred a copy of the path instead of the row the panel shows"
        );

        let other = file_in(&root, "lib.rs");
        app.toggle_favorite(&other);
        let respelled = other.parent().unwrap().join("LIB.RS");
        app.toggle_favorite(&respelled);
        assert!(
            app.favorite_files.is_empty(),
            "a case difference un-starred a copy of the path instead of the row the panel shows"
        );
    }

    /// The same assertion cannot run on CI's Linux runner, where the case
    /// difference is a second file: `lib.rs` and `LIB.RS` are two rows there
    /// because they are two names on disk, and a star that swallowed the second
    /// would hide a file the person can still open. Reading the fold as a
    /// universal rule is what let this test pass on the machine that wrote it and
    /// fail on every Linux job.
    #[cfg(not(windows))]
    #[test]
    fn a_case_sensitive_volume_keeps_two_casings_as_two_rows() {
        let root = workspace("case_sensitive");
        let other = file_in(&root, "lib.rs");
        let mut app = app_at(&root);

        app.toggle_favorite(&other);
        let respelled = other.parent().unwrap().join("LIB.RS");
        assert!(
            app.toggle_favorite(&respelled),
            "a second name is a second star"
        );
        assert_eq!(
            app.favorite_files,
            vec![other.clone(), respelled.clone()],
            "the panel lost a row by folding two names that one volume distinguishes"
        );
        assert!(app.is_favorite(&other));
        assert!(app.is_favorite(&respelled));
        assert!(!app.is_favorite(&other.parent().unwrap().join("Lib.rs")));
    }

    #[test]
    fn the_favorite_list_keeps_the_newest_and_stays_bounded() {
        let root = workspace("cap");
        let mut app = app_at(&root);

        for i in 0..=(MAX_FAVORITES) {
            let file = file_in(&root, &format!("f{i}.rs"));
            app.toggle_favorite(&file);
        }

        assert_eq!(
            app.favorite_files.len(),
            MAX_FAVORITES,
            "the panel draws every row it is handed, so the list needs a bound"
        );
        assert!(
            app.is_favorite(&file_in(&root, &format!("f{MAX_FAVORITES}.rs"))),
            "the newest star must survive the cap"
        );
        assert!(
            !app.is_favorite(&file_in(&root, "f0.rs")),
            "the cap drops from the oldest end"
        );
    }

    #[test]
    fn a_favorite_survives_the_session_that_made_it() {
        let root = workspace("persist");
        let file = file_in(&root, "main.rs");
        let mut app = app_at(&root);

        app.toggle_favorite(&file);

        let saved = VelocityApp::load_workspace_preferences(&root).expect("saved preferences");
        assert_eq!(
            saved.favorites,
            vec![file.to_string_lossy().to_string()],
            "the star was never written to the workspace state file"
        );

        let mut restarted = app_at(&root);
        restarted.restore_workspace_preferences();
        assert_eq!(
            restarted.favorite_files,
            vec![file.clone()],
            "the panel opens the next session empty although the file says otherwise"
        );
    }

    #[test]
    fn a_favorite_whose_file_is_gone_is_not_brought_back() {
        let root = workspace("vanished");
        let file = file_in(&root, "main.rs");
        let mut app = app_at(&root);
        app.toggle_favorite(&file);
        std::fs::remove_file(&file).expect("delete file");

        let mut restarted = app_at(&root);
        restarted.restore_workspace_preferences();

        assert!(
            restarted.favorite_files.is_empty(),
            "a row the person cannot open is not a favorite, it is a broken link"
        );
    }

    #[test]
    fn un_starring_rewrites_the_file_not_just_the_memory() {
        let root = workspace("unstar");
        let a = file_in(&root, "a.rs");
        let b = file_in(&root, "b.rs");
        let mut app = app_at(&root);
        app.toggle_favorite(&a);
        app.toggle_favorite(&b);

        app.toggle_favorite(&a);

        let saved = VelocityApp::load_workspace_preferences(&root).expect("saved preferences");
        assert_eq!(
            saved.favorites,
            vec![b.to_string_lossy().to_string()],
            "the panel forgot the star but the workspace file kept it"
        );
    }

    #[test]
    fn clearing_the_panel_empties_the_file_behind_it() {
        let root = workspace("clear");
        let a = file_in(&root, "a.rs");
        let b = file_in(&root, "b.rs");
        let mut app = app_at(&root);
        app.toggle_favorite(&a);
        app.toggle_favorite(&b);

        app.clear_favorites();

        let saved = VelocityApp::load_workspace_preferences(&root).expect("saved preferences");
        assert!(
            saved.favorites.is_empty(),
            "a Clear that comes back on restart is not a Clear"
        );
    }

    #[test]
    fn the_star_lives_where_the_panel_says_it_does() {
        // The empty state names the editor tab context menu as the place a file
        // gets starred. That sentence was false for as long as the panel existed,
        // so it is only allowed to stay while the menu item is in that function.
        let render = std::fs::read_to_string(format!(
            "{}/src/editor/app/render.rs",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("render.rs");
        let tab_menu = render
            .split("fn context_menu")
            .nth(1)
            .and_then(|rest| rest.split("fn ui(").next())
            .expect("the editor tab context menu");
        assert!(
            tab_menu.contains("toggle_favorite"),
            "the Favorites panel tells the person to star a file from the editor tab menu, which has no star"
        );

        let subpanel = std::fs::read_to_string(format!(
            "{}/src/editor/app/velocity_app/tier3_subpanels.rs",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("tier3_subpanels.rs");
        assert!(
            subpanel.contains("Star files from the editor tab context menu"),
            "the Favorites panel no longer says where a favorite comes from"
        );

        let sidebar = std::fs::read_to_string(format!(
            "{}/src/editor/sidebar_tabs.rs",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("sidebar_tabs.rs");
        let favorites_tab = sidebar
            .split("fn render_favorites_content")
            .nth(1)
            .and_then(|rest| rest.split("\npub fn ").next())
            .expect("the Favorites sidebar tab");
        assert!(
            favorites_tab.contains("right-click its tab"),
            "the Favorites tab asks for a star click somewhere the star is not"
        );
    }
}
