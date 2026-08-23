use crate::editor::expert_team::{slugify, ExpertTeam};

/// The member a task was routed to, plus a short human-readable reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutedMember {
    pub member_id: String,
    pub reason: String,
}

/// A parsed request to hand a task to a team.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamDirective {
    pub team_query: String,
    pub task: String,
}

const ROUTING_VERBS: &[&str] = &[
    "send", "route", "give", "have", "ask", "delegate", "hand", "dispatch", "assign", "pass",
];

const NAME_STOPWORDS: &[&str] = &[
    "the", "to", "a", "an", "this", "it", "that", "for", "our", "my", "your", "please",
];

fn is_meaningful_token(token: &str) -> bool {
    token.len() >= 4
}

/// Split a phrase into lower-case alphanumeric tokens.
fn tokens(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect()
}

/// Resolve a free-form team query to a team index.
/// Matches (in priority order) exact id, exact slug, exact name, then a
/// contains match on slug/name.
pub fn resolve_team(teams: &[ExpertTeam], query: &str) -> Option<usize> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return None;
    }
    let q_slug = slugify(query);

    if let Some(idx) = teams
        .iter()
        .position(|t| t.id.to_lowercase() == q || t.slug() == q_slug || t.name.to_lowercase() == q)
    {
        return Some(idx);
    }

    teams.iter().position(|t| {
        let slug = t.slug();
        let name = t.name.to_lowercase();
        (!q_slug.is_empty() && (slug.contains(&q_slug) || q_slug.contains(&slug)))
            || name.contains(&q)
    })
}

/// Detect a request that routes a task to a team.
///
/// Explicit prefixes are authoritative:
///   `@<slug> <task>` and `/team <name> [:] <task>`.
/// Natural language is intentionally conservative: it requires the word
/// "team", a routing verb, and a resolvable name before it will route.
pub fn parse_team_directive(input: &str) -> Option<TeamDirective> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    // `@slug rest`
    if let Some(rest) = trimmed.strip_prefix('@') {
        let mut split = rest.splitn(2, char::is_whitespace);
        let query = split.next().unwrap_or("").trim().to_string();
        let task = split.next().unwrap_or("").trim().to_string();
        if !query.is_empty() {
            return Some(TeamDirective {
                team_query: query,
                task,
            });
        }
    }

    // `/team name [:] task`
    let lower = trimmed.to_lowercase();
    if lower.starts_with("/team") {
        let after = trimmed["/team".len()..].trim_start();
        if let Some((name, task)) = after.split_once(':') {
            let query = name.trim().to_string();
            if !query.is_empty() {
                return Some(TeamDirective {
                    team_query: query,
                    task: task.trim().to_string(),
                });
            }
        }
        let mut split = after.splitn(2, char::is_whitespace);
        let query = split.next().unwrap_or("").trim().to_string();
        let task = split.next().unwrap_or("").trim().to_string();
        if !query.is_empty() {
            return Some(TeamDirective {
                team_query: query,
                task,
            });
        }
    }

    parse_natural_language(trimmed, &lower)
}

fn parse_natural_language(original: &str, lower: &str) -> Option<TeamDirective> {
    if !ROUTING_VERBS.iter().any(|v| contains_word(lower, v)) {
        return None;
    }

    // Locate the " team" keyword (as a standalone word).
    let team_pos = find_word(lower, "team")?;

    // Extract the name: the run of words immediately before "team", stopping at
    // a stopword or routing verb.
    let before = original[..team_pos].trim_end();
    let mut name_words: Vec<&str> = Vec::new();
    for word in before.split_whitespace().rev() {
        let clean = word.trim_matches(|c: char| !c.is_alphanumeric());
        if clean.is_empty() {
            continue;
        }
        let lc = clean.to_lowercase();
        if NAME_STOPWORDS.contains(&lc.as_str()) || ROUTING_VERBS.contains(&lc.as_str()) {
            break;
        }
        name_words.push(clean);
        if name_words.len() >= 4 {
            break;
        }
    }
    name_words.reverse();
    let team_query = name_words.join(" ");
    if team_query.is_empty() {
        return None;
    }

    // Task: text after "team", stripped of separators and common filler.
    let after = original[team_pos + "team".len()..].trim();
    let after = after.trim_start_matches([':', ',', '-', ' ']);
    let after = strip_leading_filler(after);
    let task = if after.trim().is_empty() {
        original.to_string()
    } else {
        after.trim().to_string()
    };

    Some(TeamDirective { team_query, task })
}

fn strip_leading_filler(text: &str) -> &str {
    let fillers = [
        "a request to ",
        "the request to ",
        "to please ",
        "and please ",
        "please ",
        "to ",
        "and ",
        "with ",
    ];
    let mut current = text.trim_start();
    loop {
        let lower = current.to_lowercase();
        let mut changed = false;
        for filler in fillers {
            if lower.starts_with(filler) {
                current = current[filler.len()..].trim_start();
                changed = true;
                break;
            }
        }
        if !changed {
            return current;
        }
    }
}

/// True if `needle` appears in `haystack` bounded by non-alphanumeric chars.
fn contains_word(haystack: &str, needle: &str) -> bool {
    find_word(haystack, needle).is_some()
}

fn find_word(haystack: &str, needle: &str) -> Option<usize> {
    let bytes = haystack.as_bytes();
    let mut start = 0;
    while let Some(rel) = haystack[start..].find(needle) {
        let pos = start + rel;
        let before_ok = pos == 0 || !bytes[pos - 1].is_ascii_alphanumeric();
        let after = pos + needle.len();
        let after_ok = after >= bytes.len() || !bytes[after].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return Some(pos);
        }
        start = pos + needle.len();
    }
    None
}

/// Choose the member best suited to `task`, using a hybrid strategy:
///   1. deterministic file-scope match,
///   2. deterministic keyword match (role/scope/skill/name),
///   3. an optional LLM router closure when matching is weak/ambiguous,
///   4. fall back to the team lead (first member).
///
/// `router`, when provided, receives the task text and returns a member id or
/// name that it considers the best fit.
pub fn route_member(
    team: &ExpertTeam,
    task: &str,
    files: &[String],
    router: Option<&dyn Fn(&str) -> Option<String>>,
) -> Option<RoutedMember> {
    if team.members.is_empty() {
        return None;
    }

    // 1. File-scope match is treated as confident. When several members' scopes
    //    match a file, the most specific (longest) pattern wins so that a narrow
    //    scope like `src/net/` beats a broad one like `src/`.
    for file in files {
        let best_match = team
            .members
            .iter()
            .filter_map(|m| m.scope_match_len(file).map(|len| (len, m)))
            .max_by_key(|(len, _)| *len);
        if let Some((_, member)) = best_match {
            return Some(RoutedMember {
                member_id: member.id.clone(),
                reason: format!("file scope match on {file}"),
            });
        }
    }

    // 2. Keyword scoring across role, scope patterns, skills, and name.
    let task_tokens = tokens(task);
    let mut best: Option<(usize, usize, String)> = None; // (member index, score, reason)
    for (idx, member) in team.members.iter().enumerate() {
        let mut score = 0usize;
        let mut reason = String::new();
        let mut consider = |field_tokens: Vec<String>, weight: usize, label: &str| {
            for token in field_tokens {
                if is_meaningful_token(&token) && task_tokens.iter().any(|t| t == &token) {
                    score += weight;
                    if reason.is_empty() {
                        reason = format!("{label} '{token}'");
                    }
                }
            }
        };
        consider(tokens(&member.role), 2, "role");
        consider(tokens(&member.name), 2, "name");
        for scope in &member.scope_patterns {
            consider(tokens(scope), 1, "scope");
        }
        for skill in &member.skills {
            consider(tokens(skill), 1, "skill");
        }

        if score > 0 && best.as_ref().map(|(_, bs, _)| score > *bs).unwrap_or(true) {
            best = Some((idx, score, reason));
        }
    }

    if let Some((idx, score, reason)) = &best {
        if *score >= 2 {
            return Some(RoutedMember {
                member_id: team.members[*idx].id.clone(),
                reason: format!("keyword match: {reason}"),
            });
        }
    }

    // 3. Weak or no keyword match: consult the LLM router if available.
    if let Some(router) = router {
        if let Some(raw) = router(task) {
            if let Some(member) = resolve_member(team, &raw) {
                return Some(RoutedMember {
                    member_id: member.id.clone(),
                    reason: "selected by router model".to_string(),
                });
            }
        }
    }

    // If a weak keyword match existed, prefer it over the blind lead fallback.
    if let Some((idx, _, reason)) = best {
        return Some(RoutedMember {
            member_id: team.members[idx].id.clone(),
            reason: format!("weak keyword match: {reason}"),
        });
    }

    // 4. Fall back to the team lead.
    team.members.first().map(|member| RoutedMember {
        member_id: member.id.clone(),
        reason: "no match; routed to team lead".to_string(),
    })
}

// ═══════════════════════════════════════════════════════════════════════════
// Routing Debug / Diagnostics
// ═══════════════════════════════════════════════════════════════════════════

/// Detailed routing decision information for debugging.
#[derive(Debug, Clone)]
pub struct RoutingDecision {
    pub stage: String,
    pub member_id: String,
    pub member_name: String,
    pub reason: String,
    pub scores: Vec<MemberScore>,
}

/// Keyword scoring result for a single member.
#[derive(Debug, Clone)]
pub struct MemberScore {
    pub member_id: String,
    pub member_name: String,
    pub score: usize,
    pub matched_tokens: Vec<String>,
}

/// Debug the routing decision for a task without actually routing it.
/// Returns detailed information about which stage matched and the scores.
pub fn debug_routing(
    team: &ExpertTeam,
    task: &str,
    files: &[String],
) -> RoutingDecision {
    if team.members.is_empty() {
        return RoutingDecision {
            stage: "error".to_string(),
            member_id: String::new(),
            member_name: String::new(),
            reason: "team has no members".to_string(),
            scores: Vec::new(),
        };
    }

    // Stage 1: File-scope match
    for file in files {
        let best_match = team
            .members
            .iter()
            .filter_map(|m| m.scope_match_len(file).map(|len| (len, m)))
            .max_by_key(|(len, _)| *len);
        if let Some((_, member)) = best_match {
            return RoutingDecision {
                stage: "file_scope_match".to_string(),
                member_id: member.id.clone(),
                member_name: member.name.clone(),
                reason: format!("file scope match on {}", file),
                scores: compute_keyword_scores(team, task),
            };
        }
    }

    // Stage 2: Keyword scoring
    let scores = compute_keyword_scores(team, task);
    if let Some(best) = scores.iter().max_by_key(|s| s.score) {
        if best.score >= 2 {
            return RoutingDecision {
                stage: "keyword_match".to_string(),
                member_id: best.member_id.clone(),
                member_name: best.member_name.clone(),
                reason: format!(
                    "keyword match: {}",
                    best.matched_tokens.first().unwrap_or(&String::new())
                ),
                scores,
            };
        }
    }

    // Stage 3: LLM router (not simulated in debug)
    // Stage 4: Fallback to team lead
    if let Some(lead) = team.members.first() {
        RoutingDecision {
            stage: "fallback_to_lead".to_string(),
            member_id: lead.id.clone(),
            member_name: lead.name.clone(),
            reason: "no strong match; routed to team lead".to_string(),
            scores,
        }
    } else {
        RoutingDecision {
            stage: "error".to_string(),
            member_id: String::new(),
            member_name: String::new(),
            reason: "no members available".to_string(),
            scores,
        }
    }
}

/// Compute keyword scores for all members without selecting a winner.
fn compute_keyword_scores(team: &ExpertTeam, task: &str) -> Vec<MemberScore> {
    let task_tokens = tokens(task);
    team.members
        .iter()
        .map(|member| {
            let mut score = 0usize;
            let mut matched_tokens = Vec::new();
            let mut consider = |field_tokens: Vec<String>, weight: usize, label: &str| {
                for token in field_tokens {
                    if is_meaningful_token(&token) && task_tokens.iter().any(|t| t == &token) {
                        score += weight;
                        matched_tokens.push(format!("{} '{}'", label, token));
                    }
                }
            };
            consider(tokens(&member.role), 2, "role");
            consider(tokens(&member.name), 2, "name");
            for scope in &member.scope_patterns {
                consider(tokens(scope), 1, "scope");
            }
            for skill in &member.skills {
                consider(tokens(skill), 1, "skill");
            }
            MemberScore {
                member_id: member.id.clone(),
                member_name: member.name.clone(),
                score,
                matched_tokens,
            }
        })
        .collect()
}

/// Resolve a router-provided string (id or name) to a team member.
fn resolve_member<'a>(
    team: &'a ExpertTeam,
    raw: &str,
) -> Option<&'a crate::editor::expert_team::ExpertMember> {
    let needle = raw.trim().to_lowercase();
    if needle.is_empty() {
        return None;
    }
    team.members
        .iter()
        .find(|m| m.id.to_lowercase() == needle || m.name.to_lowercase() == needle)
        .or_else(|| {
            team.members.iter().find(|m| {
                needle.contains(&m.id.to_lowercase()) || needle.contains(&m.name.to_lowercase())
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AiProvider;
    use crate::editor::expert_team::{ExpertMember, ExpertTeam};

    fn sample_team() -> ExpertTeam {
        ExpertTeam::new(
            "team_game",
            "Game Studio",
            "Game team",
            vec![
                ExpertMember::new(
                    "lead",
                    "Studio Lead",
                    "Architecture",
                    AiProvider::OpenRouter,
                    "m-lead",
                    vec![],
                    vec!["src/"],
                    "",
                ),
                ExpertMember::new(
                    "ui",
                    "UI Specialist",
                    "Interface Polish",
                    AiProvider::OpenRouter,
                    "m-ui",
                    vec![],
                    vec!["src/ui/"],
                    "",
                ),
                ExpertMember::new(
                    "net",
                    "Netcode Engineer",
                    "Networking",
                    AiProvider::OpenRouter,
                    "m-net",
                    vec!["netcode"],
                    vec!["src/net/"],
                    "",
                ),
            ],
            false,
        )
    }

    #[test]
    fn explicit_at_prefix() {
        let d = parse_team_directive("@game-studio polish the main menu").unwrap();
        assert_eq!(d.team_query, "game-studio");
        assert_eq!(d.task, "polish the main menu");
    }

    #[test]
    fn explicit_slash_team_with_colon() {
        let d = parse_team_directive("/team Game Studio: fix the netcode desync").unwrap();
        assert_eq!(d.team_query, "Game Studio");
        assert_eq!(d.task, "fix the netcode desync");
    }

    #[test]
    fn natural_language_route() {
        let d = parse_team_directive("have the game studio team polish the UI").unwrap();
        assert_eq!(d.team_query, "game studio");
        assert_eq!(d.task, "polish the UI");
    }

    #[test]
    fn natural_language_requires_verb() {
        assert!(parse_team_directive("the game studio team is great").is_none());
    }

    #[test]
    fn plain_prompt_is_not_a_directive() {
        assert!(parse_team_directive("please refactor the parser module").is_none());
    }

    #[test]
    fn resolve_team_by_slug() {
        let teams = vec![sample_team()];
        assert_eq!(resolve_team(&teams, "game studio"), Some(0));
        assert_eq!(resolve_team(&teams, "team_game"), Some(0));
        assert_eq!(resolve_team(&teams, "nope"), None);
    }

    #[test]
    fn route_keyword_picks_ui_member() {
        let team = sample_team();
        let routed = route_member(&team, "please polish the interface", &[], None).unwrap();
        assert_eq!(routed.member_id, "ui");
    }

    #[test]
    fn route_scope_match_wins() {
        let team = sample_team();
        let routed = route_member(&team, "anything", &["src/net/session.rs".into()], None).unwrap();
        assert_eq!(routed.member_id, "net");
    }

    #[test]
    fn route_falls_back_to_lead() {
        let team = sample_team();
        let routed = route_member(&team, "xyzzy", &[], None).unwrap();
        assert_eq!(routed.member_id, "lead");
    }

    #[test]
    fn route_uses_router_when_ambiguous() {
        let team = sample_team();
        let router = |_task: &str| Some("net".to_string());
        let routed = route_member(&team, "xyzzy", &[], Some(&router)).unwrap();
        assert_eq!(routed.member_id, "net");
        assert_eq!(routed.reason, "selected by router model");
    }
}
