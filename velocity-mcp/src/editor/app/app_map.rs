//! The app's navigable surface, as data.
//!
//! ## Why this exists
//!
//! A driver could open the eight activity-bar rails and nothing else: the rail
//! names lived in a private array restated inside two GUI commands, the 33
//! sub-tabs then under them lived in local arrays inside their render
//! functions, and 36 of the 52 dock panels could only be reached by clicking a
//! top-menu item.
//! So "did you test every button?" had no answer -- there was no list of
//! buttons, and no way to press one that was not already wired to a shortcut.
//!
//! Here every surface is one table, the render loops read *these* tables rather
//! than a copy, and the map is derived from them. The consequence is that the
//! graph below cannot claim a control exists when it does not: an edge's target
//! is built from the same `RailSpec`/`PanelSpec` the frame draws.
//!
//! ## Shape of the graph
//!
//! `app` is the root. Rails, menus, modes and panels hang off it directly;
//! sub-tabs hang off their rail; submenus hang off their parent menu. A panel
//! reached only from `Tools > Agents` is therefore three hops away, which is the
//! honest description of what a user does to see it.

use crate::editor::app::types::{PanelSpec, ALL_PANELS};
use crate::editor::theme::WorkspaceProfile;
use egui_phosphor::regular as ph;

// ═══════════════════════════════════════════════════════════════════════════
// Activity bar rails and their sub-tabs
// ═══════════════════════════════════════════════════════════════════════════

/// One sub-tab inside an activity-bar rail (`activity_sub_panel[rail]`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SubTabSpec {
    pub slug: &'static str,
    pub label: &'static str,
    pub icon: &'static str,
}

/// One activity-bar rail (`activity_bar_selection`), with the sub-tabs it owns.
#[derive(Clone, Copy, Debug)]
pub struct RailSpec {
    pub slug: &'static str,
    pub label: &'static str,
    /// Shown under the icon; the strip is 56px wide so this is abbreviated.
    pub short_label: &'static str,
    pub icon: &'static str,
    pub shortcut: &'static str,
    pub sub_tabs: &'static [SubTabSpec],
}

impl RailSpec {
    /// The rail's index into `activity_bar_selection`. Derived from position in
    /// [`RAILS`] so the table order *is* the wire order.
    pub fn index(&self) -> usize {
        RAILS
            .iter()
            .position(|rail| rail.slug == self.slug)
            .unwrap_or(0)
    }

    pub fn sub_tab_index(&self, slug: &str) -> Option<usize> {
        self.sub_tabs
            .iter()
            .position(|sub| sub.slug == slug || sub.label.eq_ignore_ascii_case(slug))
    }
}

/// The eight rails, in the order the strip draws them. This replaced an
/// anonymous tuple array built inside the render closure, which meant the
/// bridge's copy of the names could (and did) drift from it.
pub const RAILS: &[RailSpec] = &[
    RailSpec {
        slug: "files",
        label: "Files",
        short_label: "Files",
        icon: ph::FOLDER,
        shortcut: "Ctrl+E",
        sub_tabs: &[
            SubTabSpec {
                slug: "files",
                label: "Files",
                icon: ph::FOLDER,
            },
            SubTabSpec {
                slug: "bookmarks",
                label: "Bookmarks",
                icon: ph::BOOKMARK,
            },
            SubTabSpec {
                slug: "favorites",
                label: "Favorites",
                icon: ph::STAR,
            },
        ],
    },
    RailSpec {
        slug: "search",
        label: "Search",
        short_label: "Search",
        icon: ph::MAGNIFYING_GLASS,
        shortcut: "Ctrl+Shift+F",
        sub_tabs: &[
            SubTabSpec {
                slug: "search",
                label: "Search",
                icon: ph::MAGNIFYING_GLASS,
            },
            SubTabSpec {
                slug: "semantic",
                label: "Semantic",
                icon: ph::BRAIN,
            },
            SubTabSpec {
                slug: "code-graph",
                label: "Code Graph",
                icon: ph::TREE_STRUCTURE,
            },
        ],
    },
    RailSpec {
        slug: "git",
        label: "Git",
        short_label: "Git",
        icon: ph::GIT_BRANCH,
        shortcut: "Ctrl+G",
        sub_tabs: &[
            SubTabSpec {
                slug: "changes",
                label: "Changes",
                icon: ph::FILE_TEXT,
            },
            SubTabSpec {
                slug: "branches",
                label: "Branches",
                icon: ph::GIT_BRANCH,
            },
            SubTabSpec {
                slug: "commits",
                label: "Commits",
                icon: ph::GIT_COMMIT,
            },
        ],
    },
    RailSpec {
        slug: "chat",
        label: "Chat",
        short_label: "Chat",
        icon: ph::CHAT_CIRCLE,
        shortcut: "Ctrl+J",
        sub_tabs: &[
            SubTabSpec {
                slug: "chat",
                label: "Chat",
                icon: ph::CHAT_CIRCLE,
            },
            SubTabSpec {
                slug: "voice",
                label: "Voice",
                icon: ph::MICROPHONE,
            },
            SubTabSpec {
                slug: "multimodal",
                label: "Multimodal",
                icon: ph::PAPERCLIP,
            },
            SubTabSpec {
                slug: "browser",
                label: "Browser",
                icon: ph::GLOBE,
            },
        ],
    },
    RailSpec {
        slug: "build",
        label: "Build",
        short_label: "Build",
        icon: ph::HAMMER,
        shortcut: "Ctrl+B",
        sub_tabs: &[
            SubTabSpec {
                slug: "build",
                label: "Build",
                icon: ph::HAMMER,
            },
            SubTabSpec {
                slug: "test",
                label: "Test",
                icon: ph::FLASK,
            },
            SubTabSpec {
                slug: "deploy",
                label: "Deploy",
                icon: ph::ROCKET_LAUNCH,
            },
            SubTabSpec {
                slug: "debug",
                label: "Debug",
                icon: ph::BUG,
            },
            SubTabSpec {
                slug: "lsp",
                label: "LSP",
                icon: ph::PLUGS,
            },
        ],
    },
    RailSpec {
        slug: "agents",
        label: "Agents",
        short_label: "Agents",
        icon: ph::ROBOT,
        shortcut: "Ctrl+D",
        sub_tabs: &[
            SubTabSpec {
                slug: "activity",
                label: "Activity",
                icon: ph::PULSE,
            },
            SubTabSpec {
                slug: "roster",
                label: "Roster",
                icon: ph::USERS,
            },
            SubTabSpec {
                slug: "orchestration",
                label: "Orchestration",
                icon: ph::GRAPH,
            },
            SubTabSpec {
                slug: "memory",
                label: "Memory",
                icon: ph::BRAIN,
            },
            SubTabSpec {
                slug: "timeline",
                label: "Timeline",
                icon: ph::CLOCK,
            },
            SubTabSpec {
                slug: "metrics",
                label: "Metrics",
                icon: ph::CHART_BAR,
            },
        ],
    },
    RailSpec {
        slug: "knowledge",
        label: "Knowledge",
        short_label: "Know",
        icon: ph::BOOK_OPEN,
        shortcut: "Ctrl+K",
        sub_tabs: &[
            SubTabSpec {
                slug: "wiki",
                label: "Wiki",
                icon: ph::BOOK_OPEN,
            },
            SubTabSpec {
                slug: "knowledge-base",
                label: "Knowledge Base",
                icon: ph::DATABASE,
            },
            SubTabSpec {
                slug: "snippets",
                label: "Snippets",
                icon: ph::CODE,
            },
            SubTabSpec {
                slug: "nda",
                label: "NDA",
                icon: ph::LOCK,
            },
        ],
    },
    RailSpec {
        slug: "workspace",
        label: "Workspace",
        short_label: "Work",
        icon: ph::SQUARES_FOUR,
        shortcut: "Ctrl+Shift+X",
        sub_tabs: &[
            SubTabSpec {
                slug: "extensions",
                label: "Extensions",
                icon: ph::PUZZLE_PIECE,
            },
            SubTabSpec {
                slug: "plugins",
                label: "Plugins",
                icon: ph::PLUG,
            },
            SubTabSpec {
                slug: "skills",
                label: "Skills",
                icon: ph::LIGHTNING,
            },
            SubTabSpec {
                slug: "team-studio",
                label: "Team Studio",
                icon: ph::USERS,
            },
            SubTabSpec {
                slug: "usage",
                label: "Usage",
                icon: ph::GAUGE,
            },
            SubTabSpec {
                slug: "governance",
                label: "Governance",
                icon: ph::SHIELD,
            },
        ],
    },
];

/// Look up a rail by slug, case-insensitively.
pub fn rail_from_name(name: &str) -> Option<&'static RailSpec> {
    let needle = name.trim().to_ascii_lowercase();
    RAILS.iter().find(|rail| rail.slug == needle.as_str())
}

/// The sub-tabs the given rail actually has, clamped to a valid index. The
/// stored `activity_sub_panel[rail]` comes from a previous session and could
/// name a tab that has since been removed, so every read goes through here.
pub fn sub_tab(rail_index: usize, sub_index: usize) -> Option<&'static SubTabSpec> {
    RAILS.get(rail_index)?.sub_tabs.get(sub_index)
}

/// How many rails there are, for bounds-checking a restored preference.
pub fn rail_count() -> usize {
    RAILS.len()
}

// ═══════════════════════════════════════════════════════════════════════════
// Menu routes
// ═══════════════════════════════════════════════════════════════════════════

/// Top-level menus in the unified header, in draw order. A `PanelSpec` may only
/// route through one of these; the drift test enforces it, so the map cannot
/// invent a menu bar that was never built.
pub const TOP_MENUS: &[&str] = &["File", "Navigate", "Build", "Tools", "Workspace", "Help"];

/// Submenus, as `(parent, name)`. Only Tools nests today.
pub const SUB_MENUS: &[(&str, &str)] = &[
    ("Tools", "Agents"),
    ("Tools", "Knowledge"),
    ("Tools", "Automation"),
];

fn menu_node_id(menu: &str, sub: &str) -> String {
    if sub.is_empty() {
        format!("menu:{menu}")
    } else {
        format!("menu:{menu}:{sub}")
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// The map
// ═══════════════════════════════════════════════════════════════════════════

/// The id every path starts from: the running app, before anything is opened.
pub const ROOT: &str = "app";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Root,
    Menu,
    Rail,
    RailSection,
    Panel,
    Mode,
    Command,
}

impl NodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::Menu => "menu",
            Self::Rail => "rail",
            Self::RailSection => "rail-section",
            Self::Panel => "panel",
            Self::Mode => "mode",
            Self::Command => "command",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapNode {
    pub id: String,
    pub kind: NodeKind,
    pub label: String,
    /// Extra context worth showing a driver: a panel's menu route, a rail's
    /// keyboard shortcut, a section's position within its rail.
    pub detail: Option<String>,
}

/// What has to happen to cross an edge. Each variant maps to exactly one bridge
/// call, so `gui_navigate_to` can replay a path without guessing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapAction {
    /// Select an activity-bar rail (`NavigatePanel`).
    SelectRail,
    /// Pick a sub-tab within the selected rail.
    SelectSubTab,
    /// Open or focus a dock panel (`TogglePanel`'s non-destructive half).
    FocusPanel,
    /// Switch workspace profile (`Mode: …` command).
    SwitchMode,
    /// Reveal a menu. Menus are transient in egui, so this is documentation of
    /// the human route rather than something the bridge performs.
    OpenMenu,
    /// Invoke a command palette entry (`RunCommand`).
    RunCommand,
}

impl MapAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SelectRail => "select_rail",
            Self::SelectSubTab => "select_sub_tab",
            Self::FocusPanel => "focus_panel",
            Self::SwitchMode => "switch_mode",
            Self::OpenMenu => "open_menu",
            Self::RunCommand => "run_command",
        }
    }

    /// The bridge call a driver makes to cross this edge. `None` for `OpenMenu`,
    /// which has no programmatic equivalent -- and does not need one, because
    /// `FocusPanel` reaches the leaf through the same `focus_panel` the menu
    /// item calls.
    pub fn bridge_call(self) -> Option<&'static str> {
        match self {
            Self::SelectRail => Some("gui_navigate_panel"),
            Self::SelectSubTab => Some("gui_select_sub_tab"),
            Self::FocusPanel => Some("gui_toggle_panel"),
            Self::SwitchMode => Some("gui_run_command"),
            Self::RunCommand => Some("gui_run_command"),
            Self::OpenMenu => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapEdge {
    pub from: String,
    pub to: String,
    pub action: MapAction,
    /// The parameter the action takes: a rail slug, a panel slug, a mode name.
    pub target: String,
    pub detail: String,
}

/// Everything reachable, and how. Built on demand from the tables above; it is
/// a few hundred nodes and costs microseconds, so there is no reason to cache a
/// copy that could go stale mid-session.
#[derive(Clone, Debug)]
pub struct AppMap {
    pub nodes: Vec<MapNode>,
    pub edges: Vec<MapEdge>,
}

impl AppMap {
    pub fn build() -> Self {
        let mut nodes = vec![MapNode {
            id: ROOT.to_string(),
            kind: NodeKind::Root,
            label: "Velocity".to_string(),
            detail: None,
        }];
        let mut edges: Vec<MapEdge> = Vec::new();

        // ── Rails, and the sub-tabs under them ─────────────────────────────
        for rail in RAILS {
            let rail_id = format!("rail:{}", rail.slug);
            nodes.push(MapNode {
                id: rail_id.clone(),
                kind: NodeKind::Rail,
                label: rail.label.to_string(),
                detail: Some(format!("activity bar #{}", rail.index())),
            });
            edges.push(MapEdge {
                from: ROOT.to_string(),
                to: rail_id.clone(),
                action: MapAction::SelectRail,
                target: rail.slug.to_string(),
                detail: rail.label.to_string(),
            });

            for sub in rail.sub_tabs {
                let sub_id = format!("{rail_id}/{}", sub.slug);
                nodes.push(MapNode {
                    id: sub_id.clone(),
                    kind: NodeKind::RailSection,
                    label: sub.label.to_string(),
                    detail: Some(format!("under the {} rail", rail.label)),
                });
                edges.push(MapEdge {
                    from: rail_id.clone(),
                    to: sub_id,
                    action: MapAction::SelectSubTab,
                    target: format!("{}:{}", rail.slug, sub.slug),
                    detail: sub.label.to_string(),
                });
            }
        }

        // ── Menus, then the panels hanging off them ────────────────────────
        for menu in TOP_MENUS {
            let id = menu_node_id(menu, "");
            nodes.push(MapNode {
                id: id.clone(),
                kind: NodeKind::Menu,
                label: (*menu).to_string(),
                detail: Some("top-level menu".to_string()),
            });
            // The header is always on screen, so every menu is one click from
            // the root. Without this the orphan check would flag the menus as
            // controls nothing can reach.
            edges.push(MapEdge {
                from: ROOT.to_string(),
                to: id,
                action: MapAction::OpenMenu,
                target: (*menu).to_string(),
                detail: format!("open the {menu} menu"),
            });
        }
        for (parent, sub) in SUB_MENUS {
            let parent_id = menu_node_id(parent, "");
            let id = menu_node_id(parent, sub);
            nodes.push(MapNode {
                id: id.clone(),
                kind: NodeKind::Menu,
                label: (*sub).to_string(),
                detail: Some(format!("inside {parent}")),
            });
            edges.push(MapEdge {
                from: parent_id,
                to: id,
                action: MapAction::OpenMenu,
                target: (*sub).to_string(),
                detail: format!("{parent} > {sub}"),
            });
        }

        for spec in ALL_PANELS {
            let panel_id = format!("panel:{}", spec.slug);
            let route = spec.menu.map(|(menu, sub)| menu_node_id(menu, sub));
            nodes.push(MapNode {
                id: panel_id.clone(),
                kind: NodeKind::Panel,
                label: spec.label(),
                detail: route
                    .as_ref()
                    .map(|r| r.strip_prefix("menu:").unwrap_or(r).replace(':', " > ")),
            });

            // A menu route is the human path; record it so the map says how
            // many clicks the panel costs.
            if let Some(menu_id) = route {
                edges.push(MapEdge {
                    from: menu_id,
                    to: panel_id.clone(),
                    action: MapAction::OpenMenu,
                    target: spec.slug.to_string(),
                    detail: spec.label(),
                });
            }

            // Every panel is also one call from the root, because `focus_panel`
            // does not need the menu to be open. Without this edge a panel with
            // no menu route (Chat tab in a Build-mode workspace, say) would be
            // unreachable on paper while being reachable in fact.
            edges.push(MapEdge {
                from: ROOT.to_string(),
                to: panel_id,
                action: MapAction::FocusPanel,
                target: spec.slug.to_string(),
                detail: spec.label(),
            });
        }

        // ── Workspace profiles ─────────────────────────────────────────────
        for profile in WorkspaceProfile::ALL {
            let id = format!("mode:{}", profile_label(profile).to_ascii_lowercase());
            nodes.push(MapNode {
                id: id.clone(),
                kind: NodeKind::Mode,
                label: profile_label(profile).to_string(),
                detail: Some("work mode".to_string()),
            });
            edges.push(MapEdge {
                from: ROOT.to_string(),
                to: id,
                action: MapAction::SwitchMode,
                target: profile_label(profile).to_string(),
                detail: format!("Mode: {}", profile.label()),
            });
        }

        Self { nodes, edges }
    }

    pub fn node(&self, id: &str) -> Option<&MapNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Resolve whatever a caller typed into a node id. Accepts an exact id, a
    /// bare slug, a label, or a `panel:`/`rail:` prefixed id -- in that order,
    /// so an unambiguous prefix always wins over a fuzzy one.
    pub fn resolve(&self, needle: &str) -> Option<&MapNode> {
        let trimmed = needle.trim();
        if trimmed.is_empty() {
            return None;
        }
        if let Some(node) = self.nodes.iter().find(|n| n.id == trimmed) {
            return Some(node);
        }
        let down = trimmed.to_ascii_lowercase();
        // A rail wins over a panel of the same name: `chat` on the rail is what
        // a user means by "the chat section", and `panel:chat` is spellable.
        if let Some(node) = self
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Rail && n.id == format!("rail:{down}"))
        {
            return Some(node);
        }
        if let Some(node) = self
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Panel && n.id == format!("panel:{down}"))
        {
            return Some(node);
        }
        if let Some(node) = self
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Mode && n.id == format!("mode:{down}"))
        {
            return Some(node);
        }
        if trimmed.starts_with("cmd:") {
            return self.nodes.iter().find(|n| n.id.eq_ignore_ascii_case(&down));
        }
        self.nodes
            .iter()
            .find(|n| n.label.to_ascii_lowercase() == down || n.id.to_ascii_lowercase() == down)
    }

    /// Breadth-first shortest path, in edge order. Ties break toward the
    /// cheaper action: a `FocusPanel` hop before an `OpenMenu` hop, so the
    /// reported route is the one the bridge can replay rather than the one that
    /// merely describes a mouse gesture.
    pub fn path(&self, from: &str, to: &str) -> Option<Vec<MapEdge>> {
        if from == to {
            return Some(Vec::new());
        }
        let start = self.resolve(from)?.id.clone();
        let goal = self.resolve(to)?.id.clone();

        let mut visited = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::new();
        visited.insert(start.clone());
        queue.push_back((start, Vec::new()));

        while let Some((current, trail)) = queue.pop_front() {
            // Stable, predictable expansion: direct routes first.
            let mut candidates: Vec<&MapEdge> =
                self.edges.iter().filter(|e| e.from == current).collect();
            candidates.sort_by_key(|e| action_rank(e.action));
            for edge in candidates {
                if visited.contains(&edge.to) {
                    continue;
                }
                let mut next_trail = trail.clone();
                next_trail.push(edge.clone());
                if edge.to == goal {
                    return Some(next_trail);
                }
                visited.insert(edge.to.clone());
                queue.push_back((edge.to.clone(), next_trail));
            }
        }
        None
    }

    /// Nodes with no incoming edge. Anything other than the root showing up
    /// here is a control the app builds but nothing can reach.
    pub fn orphans(&self) -> Vec<&MapNode> {
        let reachable: std::collections::HashSet<&str> =
            self.edges.iter().map(|e| e.to.as_str()).collect();
        self.nodes
            .iter()
            .filter(|n| n.id != ROOT && !reachable.contains(n.id.as_str()))
            .collect()
    }

    pub fn summary(&self) -> serde_json::Value {
        let mut kinds = std::collections::BTreeMap::new();
        for node in &self.nodes {
            *kinds.entry(node.kind.as_str()).or_insert(0) += 1;
        }
        serde_json::json!({
            "nodes": self.nodes.len(),
            "edges": self.edges.len(),
            "by_kind": kinds,
            "orphans": self.orphans().iter().map(|n| n.id.clone()).collect::<Vec<_>>(),
        })
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "summary": self.summary(),
            "nodes": self.nodes.iter().map(|n| serde_json::json!({
                "id": n.id, "kind": n.kind.as_str(), "label": n.label, "detail": n.detail,
            })).collect::<Vec<_>>(),
            "edges": self.edges.iter().map(edge_json).collect::<Vec<_>>(),
        })
    }

    /// Attach the command palette to the map, so a sweep can enumerate every
    /// entry with its risk tier instead of guessing at labels. Separate from
    /// [`AppMap::build`] because the palette is built per-app (`commands()`
    /// takes `&self`) while the rest of the graph is static.
    pub fn add_commands(&mut self, commands: &[CommandSpec]) {
        for cmd in commands {
            let id = command_node_id(&cmd.label);
            if self.node(&id).is_some() {
                continue;
            }
            self.nodes.push(MapNode {
                id: id.clone(),
                kind: NodeKind::Command,
                label: cmd.label.clone(),
                detail: Some(cmd.describe()),
            });
            self.edges.push(MapEdge {
                from: ROOT.to_string(),
                to: id,
                action: MapAction::RunCommand,
                target: cmd.label.clone(),
                detail: cmd.category.clone(),
            });
        }
    }
}

/// Command node id. Labels carry spaces, ellipses and a `Mode: ` prefix, so
/// slugging them keeps ids free of characters a caller has to quote.
pub fn command_node_id(label: &str) -> String {
    let slug: String = label
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').replace("--", "-");
    format!("cmd:{slug}")
}

fn action_rank(action: MapAction) -> u8 {
    match action {
        MapAction::FocusPanel => 0,
        MapAction::SelectRail => 1,
        MapAction::SwitchMode => 2,
        MapAction::SelectSubTab => 3,
        MapAction::RunCommand => 4,
        MapAction::OpenMenu => 5,
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Command risk tiers
// ═══════════════════════════════════════════════════════════════════════

/// What a command can cost, and therefore whether the bridge will run it
/// unasked.
///
/// A driver sweeping the UI should be able to press all 40-odd navigation
/// commands without side effects. It should not be able to kick off a build,
/// spend tokens on a plan, or approve every pending tool call by accident -- so
/// those need an explicit opt-in on the request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CommandRisk {
    /// Moves what is on screen. No document, process, network or permission
    /// effect.
    Navigate,
    /// Changes buffers or files, or throws away unsaved work.
    Modify,
    /// Runs something outside the editor, costs a model call, or changes what
    /// the agent is permitted to do.
    Execute,
}

impl CommandRisk {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Navigate => "navigate",
            Self::Modify => "modify",
            Self::Execute => "execute",
        }
    }

    /// Whether a plain `gui_run_command` may perform this. Tiering is
    /// `>=`, so raising a command's tier can only ever tighten it.
    pub fn needs_opt_in(self) -> bool {
        self != Self::Navigate
    }
}

/// Commands that open a native modal and block the frame until the user answers.
/// Not dangerous, but a driver cannot dismiss one, so they are reported as such.
const INTERACTIVE_COMMANDS: &[&str] = &["Open File\u{2026}", "Save As\u{2026}"];

/// Tier a command falls into, keyed on the action it performs.
///
/// Labels are matched before categories because the two disagree: "Deploy
/// Pipeline" sounds like the riskiest thing in the palette and only opens a
/// panel, while "Run Selected Flow" ships an automation. The category is the
/// fallback, and an unknown category is `Execute` -- a command nobody has
/// classified yet is treated as the expensive one until proven otherwise.
pub fn command_risk(category: &str, label: &str) -> CommandRisk {
    let label = label.trim();
    match label {
        "Build"
        | "Run"
        | "Run Selected Flow"
        | "Rollback Deploy"
        | "Plan Sub-Agents"
        | "Request Inline Suggestion"
        | "Refresh Models"
        | "Approve All Tools"
        | "Decline All Tools"
        // `open_nda_viewer` does `create_dir_all` + `fs::write` of
        // `.velocity/nda_viewer.html` and then spawns the system browser, so
        // it reaches outside the editor even though the label reads like a
        // viewer shortcut.
        | "NDA: Open Browser Viewer" => return CommandRisk::Execute,
        "Save"
        | "Save As\u{2026}"
        | "Save All"
        | "Close Tab"
        | "Close Other Tabs"
        | "Wiki: Export to Markdown"
        | "NDA: New Document"
        | "NDA: Import Active File" => return CommandRisk::Modify,
        _ => {}
    }
    // Commands whose category default mis-states them, in either direction.
    // Each was read at its action, not its label: "Deploy Pipeline" is a
    // `toggle_panel` despite sitting under Build, and "Research Browser" only
    // selects a sidebar section despite sitting under Workspace. Listing them
    // here rather than loosening the category rules keeps the conservative
    // default for anything added to these groups later.
    //
    // The File/Edit group below all reach `modify` through their category, but
    // every one of them was read at its action: they set an `open` flag, swap
    // the focused tab or move the cursor. Nothing writes. They matter most to a
    // driver precisely because they are the tab switching and the search box.
    //
    // Deliberately absent: the `Mode: *` entries. `set_work_mode` calls
    // `save_workspace_preferences`, which rewrites
    // `.velocity/workspace-preferences.json`, and `reset_current_mode_layout`
    // throws away the stored per-mode layout -- both land on Workspace's Modify
    // default, which is the honest tier. "Reopen Closed Tab" is absent for the
    // same reason "Close Tab" is: it edits the tab list.
    const NAVIGATE: &[&str] = &[
        "Deploy Pipeline",
        "Voice Commands",
        "Research Browser",
        "Go to Definition",
        "Find All References",
        "Show Hover Info",
        "Go to File\u{2026}",
        "Go to Line\u{2026}",
        "Go to Symbol\u{2026}",
        "Next Tab",
        "Previous Tab",
        "Go Back",
        "Go Forward",
        "Find",
        "Find / Replace",
    ];
    if NAVIGATE.contains(&label) {
        return CommandRisk::Navigate;
    }
    match category {
        "Build" | "Agent" => CommandRisk::Execute,
        "File" | "Edit" | "Workspace" => CommandRisk::Modify,
        "Panels" | "View" | "Knowledge" | "Automation" => CommandRisk::Navigate,
        // Unclassified category: assume the expensive tier rather than letting a
        // newly-added group become freely runnable.
        _ => CommandRisk::Execute,
    }
}

/// Whether a command will block on a native dialog.
pub fn command_is_interactive(label: &str) -> bool {
    INTERACTIVE_COMMANDS.contains(&label.trim())
}

/// A command as the map and the bridge see it: name, grouping, and what it can
/// cost. Built from `VelocityApp::commands()`, never restated.
#[derive(Clone, Debug, PartialEq)]
pub struct CommandSpec {
    pub label: String,
    pub category: String,
    pub shortcut: Option<String>,
    pub risk: CommandRisk,
    pub interactive: bool,
}

impl CommandSpec {
    pub fn describe(&self) -> String {
        let mut out = format!("{} [{}]", self.label, self.risk.as_str());
        if let Some(sc) = &self.shortcut {
            out.push_str(&format!(" ({sc})"));
        }
        if self.interactive {
            out.push_str(" \u{2014} opens a dialog");
        }
        out
    }
}

fn profile_label(profile: WorkspaceProfile) -> &'static str {
    match profile {
        WorkspaceProfile::Coder => "coder",
        WorkspaceProfile::AutomationOperator => "automation-operator",
        WorkspaceProfile::MissionControl => "mission-control",
        WorkspaceProfile::Accessibility => "accessibility",
    }
}

pub fn edge_json(edge: &MapEdge) -> serde_json::Value {
    serde_json::json!({
        "from": edge.from,
        "to": edge.to,
        "action": edge.action.as_str(),
        "target": edge.target,
        "detail": edge.detail,
        "call": edge.action.bridge_call(),
    })
}

/// The panel behind a slug, for callers that only have a name in hand.
pub fn panel_spec(slug: &str) -> Option<&'static PanelSpec> {
    ALL_PANELS.iter().find(|spec| spec.slug == slug)
}

/// Decode a [`NodeKind::RailSection`] id back into `(rail slug, sub-tab slug)`.
/// Lives beside [`AppMap::build`], which is the only writer of the format, so
/// the decoder cannot fall behind the encoder the way a re-parse would.
pub fn split_section_id(id: &str) -> Option<(&str, &str)> {
    let rest = id.strip_prefix("rail:")?;
    let (rail, sub) = rest.split_once('/')?;
    if rail.is_empty() || sub.is_empty() {
        return None;
    }
    Some((rail, sub))
}

/// Reverse of [`profile_label`], for a driver that only has a `mode:` node id.
pub fn profile_from_label(slug: &str) -> Option<WorkspaceProfile> {
    let needle = slug.trim().to_ascii_lowercase();
    WorkspaceProfile::ALL
        .into_iter()
        .find(|profile| profile_label(*profile) == needle)
}

/// The palette label that switches to a profile, so `gui_navigate_to` reaches a
/// mode through the same command a click runs instead of poking the fields.
pub fn mode_command_label(profile: WorkspaceProfile) -> String {
    format!("Mode: {}", profile.label())
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::app::types::TabKind;

    #[test]
    fn rails_cover_every_activity_category_name() {
        // The bridge validates a rail request against ACTIVITY_CATEGORY_NAMES
        // while the map builds from RAILS; if the two lists disagree, a name the
        // map advertises gets rejected by the command that performs it.
        for name in crate::editor::app::types::ACTIVITY_CATEGORY_NAMES {
            assert!(
                RAILS.iter().any(|rail| rail.slug == *name),
                "category '{name}' has no RailSpec"
            );
        }
        assert_eq!(
            RAILS.len(),
            crate::editor::app::types::ACTIVITY_CATEGORY_NAMES.len()
        );
        for (i, rail) in RAILS.iter().enumerate() {
            assert_eq!(
                rail.index(),
                i,
                "rail order drifted from ACTIVITY_CATEGORY_NAMES"
            );
            assert_eq!(
                crate::editor::app::types::activity_category_name(i),
                rail.slug
            );
        }
    }

    #[test]
    fn rail_slugs_and_sub_tab_slugs_are_unique() {
        let mut slugs = std::collections::HashSet::new();
        for rail in RAILS {
            assert!(slugs.insert(rail.slug), "duplicate rail slug");
            assert!(
                !rail.sub_tabs.is_empty(),
                "rail {} has no sub-tabs",
                rail.slug
            );
            let mut subs = std::collections::HashSet::new();
            for sub in rail.sub_tabs {
                assert!(
                    subs.insert(sub.slug),
                    "rail {} repeats sub-tab {}",
                    rail.slug,
                    sub.slug
                );
                assert!(!sub.label.is_empty());
            }
        }
    }

    #[test]
    fn every_rail_has_exactly_the_sub_tabs_its_renderer_indexes() {
        // `activity_sub_panel` is a fixed [usize; 8]; a ninth rail or a removed
        // sub-tab would silently index past what the render match arms handle.
        assert_eq!(RAILS.len(), 8);
        // 33 as originally inventoried, plus the `chat/browser` section that
        // mounts the previously orphaned `browse_panel`.
        assert_eq!(sub_tab_count(), 34);
    }

    fn sub_tab_count() -> usize {
        RAILS.iter().map(|r| r.sub_tabs.len()).sum()
    }

    #[test]
    fn sub_tab_lookup_clamps_out_of_range_indexes() {
        assert!(sub_tab(0, 0).is_some());
        assert!(sub_tab(0, 99).is_none());
        assert!(sub_tab(99, 0).is_none());
    }

    #[test]
    fn map_has_no_orphan_nodes() {
        let map = AppMap::build();
        let orphans = map.orphans();
        assert!(
            orphans.is_empty(),
            "unreachable controls: {:?}",
            orphans.iter().map(|n| &n.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn every_panel_is_one_hop_from_the_root() {
        let map = AppMap::build();
        for spec in ALL_PANELS {
            let id = format!("panel:{}", spec.slug);
            let path = map
                .path(ROOT, &id)
                .unwrap_or_else(|| panic!("{id} unreachable"));
            assert_eq!(path.len(), 1, "{id} should be reachable in one call");
            assert_eq!(path[0].action, MapAction::FocusPanel);
            assert_eq!(path[0].target, spec.slug);
        }
    }

    #[test]
    fn menu_only_panels_report_their_click_route() {
        let map = AppMap::build();
        // Live activity sits under Tools > Agents, so a human takes three hops
        // while the bridge takes one. The map should say the human route when
        // asked for it, and never claim a menu that does not exist.
        let route = map
            .path("menu:Tools:Agents", "panel:activity")
            .expect("activity should hang off Tools > Agents");
        assert_eq!(route.len(), 1);
        assert_eq!(route[0].action, MapAction::OpenMenu);
    }

    #[test]
    fn menu_routes_only_name_menus_that_are_built() {
        for spec in ALL_PANELS {
            let Some((menu, sub)) = spec.menu else {
                continue;
            };
            assert!(
                TOP_MENUS.contains(&menu),
                "panel {} routes through unknown menu '{menu}'",
                spec.slug
            );
            if sub.is_empty() {
                continue;
            }
            assert!(
                SUB_MENUS.contains(&(menu, sub)),
                "panel {} routes through unknown submenu '{menu} > {sub}'",
                spec.slug
            );
            // A submenu route has to hang off a top-level menu that exists, and
            // the parent edge is what makes the route walkable.
            let map = AppMap::build();
            assert!(
                map.node(&menu_node_id(menu, "")).is_some(),
                "submenu parent '{menu}' is not a node"
            );
        }
    }

    #[test]
    fn sub_tabs_are_two_hops_under_the_root_via_their_rail() {
        let map = AppMap::build();
        for rail in RAILS {
            for sub in rail.sub_tabs {
                let id = format!("rail:{}/{}", rail.slug, sub.slug);
                let path = map
                    .path(ROOT, &id)
                    .unwrap_or_else(|| panic!("{id} unreachable"));
                assert_eq!(path.len(), 2, "{id} needs rail then sub-tab");
                assert_eq!(path[0].action, MapAction::SelectRail);
                assert_eq!(path[0].target, rail.slug);
                assert_eq!(path[1].action, MapAction::SelectSubTab);
                assert_eq!(path[1].target, format!("{}:{}", rail.slug, sub.slug));
            }
        }
    }

    #[test]
    fn resolution_prefers_the_exact_id_then_the_rail_axis() {
        let map = AppMap::build();
        assert_eq!(map.resolve("panel:chat").unwrap().id, "panel:chat");
        assert_eq!(map.resolve("rail:chat").unwrap().id, "rail:chat");
        // `chat` names both a rail and a panel. The rail is what a user means
        // by the bare word, and it is the axis they see first.
        assert_eq!(map.resolve("chat").unwrap().id, "rail:chat");
        // `test-generator` is a panel only, so it resolves without a prefix.
        assert_eq!(
            map.resolve("test-generator").unwrap().id,
            "panel:test-generator"
        );
        assert!(map.resolve("nonsense").is_none());
        assert!(map.resolve("   ").is_none());
    }

    #[test]
    fn path_to_the_root_is_empty_and_unknown_ends_are_none() {
        let map = AppMap::build();
        assert_eq!(map.path(ROOT, ROOT), Some(Vec::new()));
        assert!(map.path(ROOT, "nowhere").is_none());
        assert!(map.path("nowhere", ROOT).is_none());
    }

    #[test]
    fn every_edge_target_names_a_real_parameter() {
        let map = AppMap::build();
        for edge in &map.edges {
            match edge.action {
                MapAction::SelectRail => assert!(
                    rail_from_name(&edge.target).is_some(),
                    "rail edge names {}",
                    edge.target
                ),
                MapAction::FocusPanel => {
                    assert!(
                        panel_spec(&edge.target).is_some(),
                        "panel edge names {}",
                        edge.target
                    )
                }
                MapAction::SelectSubTab => {
                    let (rail, sub) = edge
                        .target
                        .split_once(':')
                        .expect("sub-tab target is `rail:sub`");
                    let rail = rail_from_name(rail).expect("rail exists");
                    assert!(
                        rail.sub_tab_index(sub).is_some(),
                        "{} has no sub-tab {sub}",
                        rail.slug
                    );
                }
                MapAction::SwitchMode => assert!(
                    WorkspaceProfile::ALL
                        .iter()
                        .any(|p| profile_label(*p) == edge.target || p.label() == edge.target),
                    "mode edge names {}",
                    edge.target
                ),
                MapAction::OpenMenu | MapAction::RunCommand => {}
            }
        }
    }

    #[test]
    fn every_mode_is_reachable_by_name() {
        let map = AppMap::build();
        for profile in WorkspaceProfile::ALL {
            let id = format!("mode:{}", profile_label(profile));
            let path = map.path(ROOT, &id).expect("mode reachable");
            assert_eq!(path[0].action, MapAction::SwitchMode);
        }
    }

    #[test]
    fn summary_counts_match_the_built_graph() {
        let map = AppMap::build();
        let summary = map.summary();
        assert_eq!(summary["nodes"].as_u64().unwrap() as usize, map.nodes.len());
        assert_eq!(
            summary["by_kind"]["panel"].as_u64().unwrap() as usize,
            ALL_PANELS.len()
        );
        assert_eq!(
            summary["by_kind"]["rail"].as_u64().unwrap() as usize,
            RAILS.len()
        );
        assert_eq!(
            summary["by_kind"]["rail-section"].as_u64().unwrap() as usize,
            sub_tab_count()
        );
        assert!(summary["orphans"].as_array().unwrap().is_empty());
    }

    /// The map's panels and the dock's render arms must line up: a `TabKind`
    /// with no arm falls through to a placeholder, and a slug pointing at one
    /// would advertise a panel that draws nothing.
    #[test]
    fn panel_table_covers_the_dock_panels_the_menus_reference() {
        // Every kind the header menus offer has to be in the table, since the
        // table is now the only way a driver can reach them.
        for kind in [
            TabKind::TestGenerator,
            TabKind::Coverage,
            TabKind::Pipeline,
            TabKind::LanguageServers,
            TabKind::Snippets,
            TabKind::InlineSuggestions,
            TabKind::PrecompCache,
            TabKind::Activity,
            TabKind::BackgroundAgents,
            TabKind::LiveOrchestration,
            TabKind::Queue,
            TabKind::Timeline,
            TabKind::Metrics,
            TabKind::ConflictResolver,
            TabKind::ImprovementEngine,
            TabKind::ContinuationLedger,
            TabKind::SemanticSearch,
            TabKind::Bookmarks,
            TabKind::Favorites,
            TabKind::AgentMemory,
            TabKind::SharedMemory,
            TabKind::PersistentMemory,
            TabKind::Flows,
            TabKind::Targets,
            TabKind::Logs,
            TabKind::Recordings,
            TabKind::Voice,
            TabKind::Multimodal,
            TabKind::AccessibilityAudit,
            TabKind::PluginRegistry,
            TabKind::SkillFiles,
            TabKind::Collaboration,
            TabKind::Peers,
        ] {
            assert!(
                ALL_PANELS
                    .iter()
                    .any(|s| std::mem::discriminant(&s.kind) == std::mem::discriminant(&kind)),
                "{kind:?} is offered from a menu but cannot be named"
            );
        }
    }

    /// egui's `SelectableLabel` ids come from the visible text, so two sub-tabs
    /// in one rail sharing a label would make clicking one select the other.
    #[test]
    fn sub_tab_labels_are_unique_within_a_rail() {
        for rail in RAILS {
            let mut labels = std::collections::HashSet::new();
            for sub in rail.sub_tabs {
                assert!(
                    labels.insert(sub.label),
                    "rail {} shows {:?} twice",
                    rail.slug,
                    sub.label
                );
            }
        }
    }

    /// Guards the icons: `ph::*` constants that do not exist fail to compile,
    /// but a constant that resolves to an unrendered codepoint does not, so at
    /// least assert nothing is empty and nothing collapsed to a literal '?'.
    #[test]
    fn rail_icons_are_non_empty_glyphs() {
        for rail in RAILS {
            assert!(!rail.icon.is_empty());
            assert_ne!(rail.icon, "?");
            assert!(
                !rail.shortcut.is_empty(),
                "rail {} has no shortcut",
                rail.slug
            );
            for sub in rail.sub_tabs {
                assert!(!sub.icon.is_empty());
                assert_ne!(sub.icon, "?");
            }
        }
    }

    #[test]
    fn map_json_round_trips_through_serde() {
        let json = AppMap::build().to_json();
        assert!(json["nodes"].is_array());
        assert!(json["edges"].is_array());
        let edges = json["edges"].as_array().unwrap();
        assert!(!edges.is_empty());
        assert!(edges[0]["action"].is_string());
    }

    // ── Command risk tiers ───────────────────────────────────────────────

    #[test]
    fn commands_that_cost_something_are_never_navigate_tier() {
        // Each of these has a real consequence: a compiler run, a model call,
        // or handing every pending tool approval to the agent.
        for label in [
            "Build",
            "Run",
            "Run Selected Flow",
            "Rollback Deploy",
            "Plan Sub-Agents",
            "Request Inline Suggestion",
            "Refresh Models",
            "Approve All Tools",
            "Decline All Tools",
        ] {
            assert_eq!(
                command_risk("Agent", label),
                CommandRisk::Execute,
                "'{label}' must not be runnable by a blind sweep"
            );
        }
    }

    /// "Deploy Pipeline" and "Voice Commands" read like the risky ones and are
    /// only `toggle_panel` calls; "Run Selected Flow" reads harmless and ships
    /// an automation. Tiering has to follow the action, not the wording.
    #[test]
    fn sound_alike_labels_are_tiered_by_what_they_actually_do() {
        assert_eq!(
            command_risk("Build", "Deploy Pipeline"),
            CommandRisk::Navigate
        );
        assert_eq!(
            command_risk("Panels", "Voice Commands"),
            CommandRisk::Navigate
        );
        assert_eq!(
            command_risk("Automation", "Run Selected Flow"),
            CommandRisk::Execute
        );
        // The other direction: both sit under Workspace, which is a `Modify`
        // category, and both do more than the category implies or less.
        assert_eq!(
            command_risk("Workspace", "NDA: Open Browser Viewer"),
            CommandRisk::Execute,
            "it writes a file and spawns a browser"
        );
        assert_eq!(
            command_risk("Workspace", "Research Browser"),
            CommandRisk::Navigate,
            "it only selects a sidebar section"
        );
    }

    /// Switching a work mode rewrites `.velocity/workspace-preferences.json`,
    /// and resetting a layout throws the stored one away, so a blind sweep
    /// cannot be allowed to move the app between modes on its own.
    #[test]
    fn mode_commands_are_modify_tier_because_they_persist() {
        for label in [
            "Mode: Coder",
            "Mode: Automation Operator",
            "Mode: Mission Control",
            "Mode: Accessibility",
            "Mode: Reset Layout to Default",
        ] {
            assert_eq!(
                command_risk("Workspace", label),
                CommandRisk::Modify,
                "'{label}' writes workspace preferences"
            );
        }
    }

    /// The tab switcher, the search box and the go-to overlays all sit under
    /// File or Edit, which is a writing category, and none of them writes. They
    /// are also exactly what a driver poking at "did you test tab switching"
    /// needs to be able to press, so a tier that lumps them with Save makes the
    /// answer "no, not without opting in to Save too".
    #[test]
    fn editor_navigation_is_tiered_with_the_rest_of_the_navigation() {
        for label in [
            "Next Tab",
            "Previous Tab",
            "Go Back",
            "Go Forward",
            "Find",
            "Find / Replace",
            "Go to File\u{2026}",
            "Go to Line\u{2026}",
            "Go to Symbol\u{2026}",
        ] {
            assert_eq!(
                command_risk("File", label),
                CommandRisk::Navigate,
                "'{label}' only moves what is on screen"
            );
        }
        // The neighbours that genuinely edit the tab list stay behind the gate.
        for label in [
            "Close Tab",
            "Close Other Tabs",
            "Reopen Closed Tab",
            "New File",
        ] {
            assert_eq!(
                command_risk("File", label),
                CommandRisk::Modify,
                "'{label}' changes the open buffers"
            );
        }
    }

    #[test]
    fn an_unknown_category_tiers_as_execute_not_navigate() {
        // The safe default: a category added tomorrow is not freely runnable
        // until someone looks at it.
        assert_eq!(
            command_risk("Tomorrow", "Do Something New"),
            CommandRisk::Execute
        );
        assert_eq!(command_risk("", ""), CommandRisk::Execute);
    }

    #[test]
    fn every_known_category_has_a_tier() {
        // `command_risk` is total, so this asserts the *shape*: each palette
        // group lands somewhere, and no group silently defaults.
        for category in [
            "File",
            "Edit",
            "Build",
            "Agent",
            "Panels",
            "View",
            "Knowledge",
            "Automation",
            "Workspace",
        ] {
            let risk = command_risk(category, "");
            assert!(
                matches!(
                    risk,
                    CommandRisk::Navigate | CommandRisk::Modify | CommandRisk::Execute
                ),
                "category '{category}' produced no tier"
            );
        }
    }

    #[test]
    fn navigate_tier_is_the_only_one_needing_no_opt_in() {
        assert!(!CommandRisk::Navigate.needs_opt_in());
        assert!(CommandRisk::Modify.needs_opt_in());
        assert!(CommandRisk::Execute.needs_opt_in());
        // Ordering backs the `>=` comparison the bridge uses.
        assert!(CommandRisk::Navigate < CommandRisk::Modify);
        assert!(CommandRisk::Modify < CommandRisk::Execute);
    }

    #[test]
    fn dialog_commands_are_flagged_so_a_driver_does_not_stall() {
        assert!(command_is_interactive("Open File\u{2026}"));
        assert!(command_is_interactive("Save As\u{2026}"));
        assert!(!command_is_interactive("Save"));
        assert!(!command_is_interactive("Settings"));
    }

    #[test]
    fn command_ids_are_unique_across_the_whole_palette() {
        // Two labels slugging to one id would make `gui_run_command` ambiguous,
        // and the map would drop the second command on the floor.
        let labels: Vec<String> = [
            "New File",
            "Open File\u{2026}",
            "Go to File\u{2026}",
            "Save",
            "Save As\u{2026}",
            "Save All",
            "Settings",
            "Mode: Coder",
            "Mode: Mission Control",
            "Wiki: Export to Markdown",
            "NDA: New Document",
            "Toggle Sidebar",
            "Find",
            "Find / Replace",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let mut seen = std::collections::HashSet::new();
        for label in &labels {
            assert!(
                seen.insert(command_node_id(label)),
                "two palette entries share an id at '{label}'"
            );
        }
        assert_eq!(command_node_id("Mode: Coder"), "cmd:Mode-Coder");
        assert_eq!(command_node_id("  New File  "), "cmd:New-File");
    }

    #[test]
    fn adding_commands_makes_each_one_hop_from_the_root() {
        let mut map = AppMap::build();
        let commands = vec![
            CommandSpec {
                label: "Settings".to_string(),
                category: "Panels".to_string(),
                shortcut: Some("Ctrl+,".to_string()),
                risk: CommandRisk::Navigate,
                interactive: false,
            },
            CommandSpec {
                label: "Build".to_string(),
                category: "Build".to_string(),
                shortcut: Some("Ctrl+B".to_string()),
                risk: CommandRisk::Execute,
                interactive: false,
            },
        ];
        let before = map.nodes.len();
        map.add_commands(&commands);
        assert_eq!(map.nodes.len(), before + 2);
        // Adding them twice must not duplicate nodes or edges.
        map.add_commands(&commands);
        assert_eq!(map.nodes.len(), before + 2);

        for cmd in &commands {
            let id = command_node_id(&cmd.label);
            let path = map.path(ROOT, &id).expect("command reachable");
            assert_eq!(path.len(), 1);
            assert_eq!(path[0].action, MapAction::RunCommand);
            assert_eq!(path[0].target, cmd.label);
        }
        assert!(map.orphans().is_empty());
        let json = map.to_json();
        assert_eq!(json["summary"]["by_kind"]["command"].as_u64().unwrap(), 2);
    }

    #[test]
    fn command_detail_carries_the_tier_and_the_dialog_flag() {
        let plain = CommandSpec {
            label: "Save".to_string(),
            category: "File".to_string(),
            shortcut: Some("Ctrl+S".to_string()),
            risk: CommandRisk::Modify,
            interactive: false,
        };
        assert_eq!(plain.describe(), "Save [modify] (Ctrl+S)");
        let modal = CommandSpec {
            label: "Open File\u{2026}".to_string(),
            category: "File".to_string(),
            shortcut: Some("Ctrl+O".to_string()),
            risk: CommandRisk::Navigate,
            interactive: true,
        };
        assert!(modal.describe().contains("opens a dialog"));
    }
}
