pub mod browser;
pub mod drone;
pub mod system;
pub mod team;
pub mod wa;

use super::types::Tool;

pub fn get_tools() -> Vec<Tool> {
    let mut tools = system::get_system_tools();
    tools.extend(browser::get_browser_tools());
    tools.extend(wa::get_wa_tools());
    tools.extend(team::get_team_tools());
    tools.extend(drone::get_drone_tools());
    // Include dynamically registered custom tools.
    let root = std::env::current_dir().unwrap_or_default();
    tools.extend(super::custom_tools::list_tools(&root));
    tools
}
