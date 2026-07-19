use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use sha2::{Digest, Sha256};
use velocity_ide::site_map::SiteMap;
use velocity_ide::site_map::verifier::NdaNode;

#[derive(Debug, Clone)]
pub struct AomElement {
    pub role: String,
    pub name: String,
    pub value: String,
    pub target_url: Option<String>,
}

/// Helper to extract an attribute value from a raw tag string.
fn extract_attr(tag: &str, attr_name: &str) -> Option<String> {
    let search = format!("{}=", attr_name);
    if let Some(idx) = tag.find(&search) {
        let after_eq = &tag[idx + search.len()..];
        if after_eq.is_empty() {
            return None;
        }
        let quote_char = after_eq.chars().next()?;
        if quote_char == '"' || quote_char == '\'' {
            let val_part = &after_eq[1..];
            if let Some(end_idx) = val_part.find(quote_char) {
                return Some(val_part[..end_idx].to_string());
            }
        } else {
            // Unquoted attribute value
            let end_idx = after_eq.find(|c: char| c.is_whitespace() || c == '/' || c == '>');
            if let Some(end) = end_idx {
                return Some(after_eq[..end].to_string());
            } else {
                return Some(after_eq.to_string());
            }
        }
    }
    None
}

/// Simple URL resolver for relative paths.
fn resolve_relative_url(base: &str, relative: &str) -> String {
    if relative.starts_with("http://") || relative.starts_with("https://") {
        return relative.to_string();
    }
    
    let base_trimmed = base.trim_end_matches('/');
    if relative.starts_with('/') {
        // Resolve to domain root
        if let Some(domain_end) = base_trimmed.find("://") {
            let domain_part = &base_trimmed[domain_end + 3..];
            if let Some(slash_idx) = domain_part.find('/') {
                let domain = &base_trimmed[..domain_end + 3 + slash_idx];
                return format!("{}{}", domain, relative);
            }
        }
        return format!("{}{}", base_trimmed, relative);
    }
    
    // Resolve relative to current directory
    if let Some(last_slash) = base_trimmed.rfind('/') {
        if last_slash > 8 { // skip https:// slash
            return format!("{}/{}", &base_trimmed[..last_slash], relative);
        }
    }
    format!("{}/{}", base_trimmed, relative)
}

/// Truncate text block to maximum length.
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

fn escape_nda_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}

fn crawl_facts_path(url: &str, sitemap_path: &Path) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    let digest = hasher.finalize();
    let capture_id = digest[..8]
        .iter()
        .map(|byte| format!("{:02x}", byte))
        .collect::<String>();
    sitemap_path
        .parent()
        .unwrap_or(sitemap_path)
        .join("browser-captures")
        .join(format!("{}.nda", capture_id))
}

fn write_crawl_facts(
    url: &str,
    title: &str,
    summary: &str,
    elements: &[AomElement],
    sitemap_path: &Path,
) -> Result<PathBuf, String> {
    let facts_path = crawl_facts_path(url, sitemap_path);
    if let Some(parent) = facts_path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create browser capture dir: {err}"))?;
    }

    let mut facts = Vec::new();
    facts.push("artifact:browser-capture kind page-crawl".to_string());
    facts.push(format!("artifact:browser-capture page_url \"{}\"", escape_nda_text(url)));
    facts.push(format!("artifact:browser-capture page_title \"{}\"", escape_nda_text(title)));
    facts.push(format!("artifact:browser-capture page_summary \"{}\"", escape_nda_text(summary)));
    facts.push(format!("artifact:browser-capture element_count {}", elements.len()));

    for (idx, element) in elements.iter().enumerate() {
        let element_id = format!("element:{}", idx + 1);
        facts.push(format!("artifact:browser-capture element {}", element_id));
        facts.push(format!("{} role \"{}\"", element_id, escape_nda_text(&element.role)));
        facts.push(format!("{} name \"{}\"", element_id, escape_nda_text(&element.name)));
        facts.push(format!("{} value \"{}\"", element_id, escape_nda_text(&element.value)));
        if let Some(target_url) = &element.target_url {
            facts.push(format!("{} target_url \"{}\"", element_id, escape_nda_text(target_url)));
        }
    }

    fs::write(&facts_path, facts.join("\n")).map_err(|err| format!("write browser capture facts: {err}"))?;
    Ok(facts_path)
}

pub fn crawl_and_sync_sitemap(url: &str, sitemap_path: &Path) -> Result<String, String> {
    // 1. Fetch HTML using ureq (already in workspace)
    let agent = ureq::Agent::new();
    let resp = agent.get(url)
        .set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.7727.138 Safari/537.36")
        .call()
        .map_err(|e| format!("HTTP request failed: {:?}", e))?;
    
    let html = resp.into_string()
        .map_err(|e| format!("Failed to read HTTP body: {:?}", e))?;

    // 2. Open SiteMap database client
    let mut sm = SiteMap::open(sitemap_path, 0)
        .map_err(|e| format!("Failed to open SiteMap: {:?}", e))?;

    // 3. Simple character state machine to scan HTML tags & text
    let mut elements = Vec::new();
    let mut title = "Untitled Page".to_string();
    let mut page_text = String::new();

    let chars: Vec<char> = html.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            let mut tag_content = String::new();
            i += 1;
            while i < chars.len() && chars[i] != '>' {
                tag_content.push(chars[i]);
                i += 1;
            }
            let trimmed = tag_content.trim();
            let lower = trimmed.to_lowercase();
            if lower.starts_with("title") {
                // Read title text
                i += 1;
                let mut t = String::new();
                while i < chars.len() && chars[i] != '<' {
                    t.push(chars[i]);
                    i += 1;
                }
                title = t.trim().to_string();
            } else if lower.starts_with("a ") || lower.starts_with("a>") {
                let href = extract_attr(trimmed, "href");
                let mut text_content = String::new();
                let mut j = i + 1;
                while j < chars.len() && chars[j] != '<' {
                    text_content.push(chars[j]);
                    j += 1;
                }
                let clean_text = text_content.trim().to_string();
                if let Some(h) = href {
                    let absolute_href = resolve_relative_url(url, &h);
                    elements.push(AomElement {
                        role: "link".to_string(),
                        name: if clean_text.is_empty() { absolute_href.clone() } else { clean_text },
                        value: absolute_href.clone(),
                        target_url: Some(absolute_href),
                    });
                }
            } else if lower.starts_with("button") {
                let mut text_content = String::new();
                let mut j = i + 1;
                while j < chars.len() && chars[j] != '<' {
                    text_content.push(chars[j]);
                    j += 1;
                }
                let clean_text = text_content.trim().to_string();
                if !clean_text.is_empty() {
                    elements.push(AomElement {
                        role: "button".to_string(),
                        name: clean_text,
                        value: "".to_string(),
                        target_url: None,
                    });
                }
            } else if lower.starts_with("input") {
                let input_type = extract_attr(trimmed, "type").unwrap_or_else(|| "text".to_string());
                let placeholder = extract_attr(trimmed, "placeholder").unwrap_or_default();
                let name_attr = extract_attr(trimmed, "name").unwrap_or_default();
                let value_attr = extract_attr(trimmed, "value").unwrap_or_default();
                
                let name = if !placeholder.is_empty() {
                    placeholder
                } else if !name_attr.is_empty() {
                    name_attr
                } else {
                    "Input Field".to_string()
                };

                let role = match input_type.as_str() {
                    "button" | "submit" => "button",
                    _ => "textbox",
                };

                elements.push(AomElement {
                    role: role.to_string(),
                    name,
                    value: value_attr,
                    target_url: None,
                });
            }
        } else {
            // Accumulate page text
            if chars[i] != '\r' && chars[i] != '\n' && chars[i] != '\t' {
                page_text.push(chars[i]);
            }
            i += 1;
        }
    }

    // 4. Save structured Merkle Triples directly into the local SiteMap
    let page_hash = sm.register_string(url).map_err(|e| e.to_string())?;
    let title_hash = sm.register_string(&title).map_err(|e| e.to_string())?;
    let page_summary = truncate_string(&page_text, 1000);
    let summary_hash = sm.register_string(&page_summary).map_err(|e| e.to_string())?;

    // Page URL metadata triple
    sm.put_node(&NdaNode::Triple {
        subject_hash: page_hash,
        predicate_id: 10, // PredicateURL
        object_hash: page_hash,
    }).map_err(|e| e.to_string())?;

    // Page Title metadata triple
    sm.put_node(&NdaNode::Triple {
        subject_hash: page_hash,
        predicate_id: 11, // PredicateTitle
        object_hash: title_hash,
    }).map_err(|e| e.to_string())?;

    // Page Summary metadata triple
    sm.put_node(&NdaNode::Triple {
        subject_hash: page_hash,
        predicate_id: 12, // PredicateSummary
        object_hash: summary_hash,
    }).map_err(|e| e.to_string())?;

    let mut aom_node_hashes = Vec::new();

    // Compile AOM elements
    for el in &elements {
        let el_role_hash = sm.register_string(&el.role).map_err(|e| e.to_string())?;
        let el_name_hash = sm.register_string(&el.name).map_err(|e| e.to_string())?;
        let el_val_hash = sm.register_string(&el.value).map_err(|e| e.to_string())?;

        // Generate unique structural hash for this instance
        let mut hasher = Sha256::new();
        hasher.update(page_hash.to_le_bytes());
        hasher.update(el.role.as_bytes());
        hasher.update(el.name.as_bytes());
        let digest = hasher.finalize();
        let el_hash = u64::from_le_bytes(digest[0..8].try_into().unwrap());

        // Store role
        sm.put_node(&NdaNode::Triple {
            subject_hash: el_hash,
            predicate_id: 16, // PredicateRole
            object_hash: el_role_hash,
        }).map_err(|e| e.to_string())?;

        // Store name
        sm.put_node(&NdaNode::Triple {
            subject_hash: el_hash,
            predicate_id: 17, // PredicateName
            object_hash: el_name_hash,
        }).map_err(|e| e.to_string())?;

        // Store value
        sm.put_node(&NdaNode::Triple {
            subject_hash: el_hash,
            predicate_id: 18, // PredicateValue
            object_hash: el_val_hash,
        }).map_err(|e| e.to_string())?;

        aom_node_hashes.push(el_hash);

        // If it's a link, save PredicateLinksTo
        if let Some(ref target) = el.target_url {
            let target_hash = sm.register_string(target).map_err(|e| e.to_string())?;
            sm.put_node(&NdaNode::Triple {
                subject_hash: page_hash,
                predicate_id: 1, // PredicateLinksTo
                object_hash: target_hash,
            }).map_err(|e| e.to_string())?;
        }
    }

    // Build the page's AOM root children Scope node
    if !aom_node_hashes.is_empty() {
        let mut scope_children = Vec::new();
        for h in aom_node_hashes {
            scope_children.push(NdaNode::Call { target: h });
        }
        let aom_root_node = NdaNode::Scope { children: scope_children };
        let root_hash = sm.put_node(&aom_root_node).map_err(|e| e.to_string())?;

        // Link page to AOM root
        sm.put_node(&NdaNode::Triple {
            subject_hash: page_hash,
            predicate_id: 6, // PredicateHasAomRoot
            object_hash: root_hash,
        }).map_err(|e| e.to_string())?;
    }

    sm.flush().map_err(|e| e.to_string())?;
    let facts_path = write_crawl_facts(url, &title, &page_summary, &elements, sitemap_path)?;

    Ok(format!(
        "Crawler finished.\nURL: {}\nTitle: {}\nInteractive Elements: {}\nRegistered in Merkle SiteMap at {:?}\nNDA Facts: {:?}",
        url, title, elements.len(), sitemap_path, facts_path
    ))
}

#[cfg(test)]
mod tests {
    use super::{crawl_facts_path, write_crawl_facts, AomElement};
    use std::fs;

    #[test]
    fn writes_browser_capture_facts() {
        let temp = tempfile::tempdir().unwrap();
        let sitemap_path = temp.path().join("site_map");
        let facts_path = write_crawl_facts(
            "https://example.com/docs",
            "Docs",
            "Documentation landing page",
            &[
                AomElement {
                    role: "link".to_string(),
                    name: "API".to_string(),
                    value: "https://example.com/api".to_string(),
                    target_url: Some("https://example.com/api".to_string()),
                },
                AomElement {
                    role: "button".to_string(),
                    name: "Search".to_string(),
                    value: "".to_string(),
                    target_url: None,
                },
            ],
            &sitemap_path,
        )
        .unwrap();

        assert_eq!(facts_path, crawl_facts_path("https://example.com/docs", &sitemap_path));
        let facts = fs::read_to_string(facts_path).unwrap();
        assert!(facts.contains("artifact:browser-capture kind page-crawl"));
        assert!(facts.contains("artifact:browser-capture page_title \"Docs\""));
        assert!(facts.contains("artifact:browser-capture element_count 2"));
        assert!(facts.contains("element:1 role \"link\""));
        assert!(facts.contains("element:2 role \"button\""));
    }
}
