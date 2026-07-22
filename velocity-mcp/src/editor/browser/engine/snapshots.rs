use super::*;
use std::fs;
use std::path::{Path, PathBuf};
use sha2::{Digest, Sha256};
use velocity_ide::site_map::{NdaNode, SiteMap, VcTriple};

pub fn read_snapshot(url: &str, sitemap_path: &Path) -> Result<BrowserPageSnapshot, String> {
    load_snapshot_json(url, sitemap_path)
}

pub fn read_visual_fallback_report(
    url: &str,
    sitemap_path: &Path,
) -> Result<BrowserVisualFallbackReadReport, String> {
    let html = load_html_fallback(url, sitemap_path)?;
    let path = browser_html_fallback_path(url, sitemap_path);
    Ok(BrowserVisualFallbackReadReport {
        url: url.to_string(),
        html_path: path.display().to_string(),
        byte_count: html.len(),
    })
}

pub fn read_visual_fallback(url: &str, sitemap_path: &Path) -> Result<String, String> {
    load_html_fallback(url, sitemap_path)
}

pub fn read_snapshot_report(
    url: &str,
    sitemap_path: &Path,
) -> Result<BrowserSnapshotReadReport, String> {
    let snapshot = read_snapshot(url, sitemap_path)?;
    let html_fallback_path = browser_html_fallback_path(url, sitemap_path);
    Ok(BrowserSnapshotReadReport {
        snapshot: summarize_snapshot(snapshot),
        json_path: browser_snapshot_path(url, sitemap_path)
            .display()
            .to_string(),
        html_fallback_path: html_fallback_path
            .exists()
            .then(|| html_fallback_path.display().to_string()),
    })
}

pub fn summarize_snapshot(snapshot: BrowserPageSnapshot) -> BrowserSnapshotSummary {
    BrowserSnapshotSummary {
        network_summary: summarize_network_activity(&snapshot.protocol_events),
        url: snapshot.url,
        title: snapshot.title,
        element_count: snapshot.elements.len(),
        form_count: snapshot.forms.len(),
        cookie_count: snapshot.cookies.len(),
        request_count: snapshot.requests.len(),
        settle_signal_count: snapshot.settle_signals.len(),
        runtime_state_count: snapshot.runtime_state.len(),
        protocol_event_count: snapshot.protocol_events.len(),
        json_path: None,
    }
}

pub fn summarize_snapshot_diff(diff: &BrowserSnapshotDiff) -> String {
    render_snapshot_diff(diff)
}

pub fn summarize_snapshot_diff_report(
    report: BrowserSnapshotDiffReport,
) -> BrowserSnapshotDiffSummary {
    BrowserSnapshotDiffSummary {
        before_url: report.before_url,
        after_url: report.after_url,
        summary: report.summary,
    }
}

pub fn read_snapshot_diff_report(
    before_url: &str,
    after_url: &str,
    sitemap_path: &Path,
) -> Result<BrowserSnapshotDiffReadReport, String> {
    let report = diff_saved_snapshots(before_url, after_url, sitemap_path)?;
    Ok(BrowserSnapshotDiffReadReport {
        diff: summarize_snapshot_diff_report(report),
        before_json_path: browser_snapshot_path(before_url, sitemap_path)
            .display()
            .to_string(),
        after_json_path: browser_snapshot_path(after_url, sitemap_path)
            .display()
            .to_string(),
    })
}

pub fn diff_saved_snapshots(
    before_url: &str,
    after_url: &str,
    sitemap_path: &Path,
) -> Result<BrowserSnapshotDiffReport, String> {
    let before = load_snapshot_json(before_url, sitemap_path)?;
    let after = load_snapshot_json(after_url, sitemap_path)?;
    let diff = diff_snapshots(&before, &after);
    Ok(BrowserSnapshotDiffReport {
        before_url: before.url,
        after_url: after.url,
        summary: summarize_snapshot_diff(&diff),
        diff,
    })
}

pub fn list_snapshots(
    sitemap_path: &Path,
    url_contains: Option<&str>,
    title_contains: Option<&str>,
    limit: Option<usize>,
    sort_direction: BrowserListSortDirection,
) -> Result<Vec<BrowserSnapshotSummary>, String> {
    let dir = sitemap_path
        .parent()
        .unwrap_or(sitemap_path)
        .join("browser-snapshots");
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut items = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|err| format!("read browser snapshot dir: {err}"))? {
        let entry: fs::DirEntry = match entry {
            Ok(e) => e,
            Err(err) => return Err(format!("read browser snapshot dir entry: {err}")),
        };
        let path = entry.path();
        if path.extension().and_then(|ext: &std::ffi::OsStr| ext.to_str()) != Some("json") {
            continue;
        }
        let raw = fs::read(&path).map_err(|err| format!("read browser snapshot: {err}"))?;
        let snapshot: BrowserPageSnapshot =
            serde_json::from_slice(&raw).map_err(|err| format!("parse browser snapshot: {err}"))?;
        let mut summary = summarize_snapshot(snapshot);
        summary.json_path = Some(path.display().to_string());
        if url_contains
            .map(|needle| contains_case_insensitive(&summary.url, needle))
            .unwrap_or(true)
            && title_contains
                .map(|needle| contains_case_insensitive(&summary.title, needle))
                .unwrap_or(true)
        {
            items.push(summary);
        }
    }
    finalize_list(&mut items, sort_direction, limit, |left, right| {
        left.url.cmp(&right.url)
    });
    Ok(items)
}

pub fn write_crawl_facts(
    url: &str,
    title: &str,
    summary: &str,
    elements: &[AomElement],
    forms: &[BrowserForm],
    cookies: &[BrowserCookie],
    storage: &[BrowserStorageBucket],
    mutations: &[String],
    requests: &[BrowserRequestRecord],
    settle_signals: &[String],
    runtime_state: &[BrowserRuntimeState],
    protocol_events: &[BrowserProtocolEvent],
    sitemap_path: &Path,
) -> Result<PathBuf, String> {
    let facts_path = crawl_facts_path(url, sitemap_path);
    if let Some(parent) = facts_path.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("create browser capture dir: {err}"))?;
    }

    let storage_entry_count = storage
        .iter()
        .map(|bucket| bucket.entries.len())
        .sum::<usize>();
    let mut facts = vec![
        "browser-capture version 9".to_string(),
        "field_count 10".to_string(),
        "field\tkind\tpage-crawl".to_string(),
        format!("field\telement_count\t{}", elements.len()),
        format!("field\tform_count\t{}", forms.len()),
        format!("field\tcookie_count\t{}", cookies.len()),
        format!("field\tstorage_entry_count\t{}", storage_entry_count),
        format!("field\tmutation_count\t{}", mutations.len()),
        format!("field\trequest_count\t{}", requests.len()),
        format!("field\tsettle_signal_count\t{}", settle_signals.len()),
        format!("field\truntime_state_count\t{}", runtime_state.len()),
        format!("field\tprotocol_event_count\t{}", protocol_events.len()),
        "page_field_count 3".to_string(),
        format!("page_field\turl\t{}", encode_nda_text(url)),
        format!("page_field\ttitle\t{}", encode_nda_text(title)),
        format!("page_field\tsummary\t{}", encode_nda_text(summary)),
    ];

    for (idx, element) in elements.iter().enumerate() {
        facts.push(format!("element\t{}", idx));
        facts.push(format!(
            "element_field\t{}\trole\t{}",
            idx,
            encode_nda_text(&element.role)
        ));
        facts.push(format!(
            "element_field\t{}\tname\t{}",
            idx,
            encode_nda_text(&element.name)
        ));
        facts.push(format!(
            "element_field\t{}\tvalue\t{}",
            idx,
            encode_nda_text(&element.value)
        ));
        facts.push(format!(
            "element_field\t{}\ttarget_url\t{}",
            idx,
            encode_nda_text(element.target_url.as_deref().unwrap_or("-")),
        ));
    }

    for (form_idx, form) in forms.iter().enumerate() {
        facts.push(format!("form\t{}", form_idx));
        facts.push(format!(
            "form_field\t{}\tid\t{}",
            form_idx,
            encode_nda_text(&form.id)
        ));
        facts.push(format!(
            "form_field\t{}\taction\t{}",
            form_idx,
            encode_nda_text(&form.action)
        ));
        facts.push(format!(
            "form_field\t{}\tmethod\t{}",
            form_idx,
            encode_nda_text(&form.method)
        ));
        if let Some(submit_label) = &form.submit_label {
            facts.push(format!(
                "form_field\t{}\tsubmit_label\t{}",
                form_idx,
                encode_nda_text(submit_label)
            ));
        }
        for (field_idx, field) in form.fields.iter().enumerate() {
            facts.push(format!("form_input\t{}\t{}", form_idx, field_idx));
            facts.push(format!(
                "form_input_field\t{}\t{}\tname\t{}",
                form_idx,
                field_idx,
                encode_nda_text(&field.name)
            ));
            facts.push(format!(
                "form_input_field\t{}\t{}\tlabel\t{}",
                form_idx,
                field_idx,
                encode_nda_text(&field.label)
            ));
            facts.push(format!(
                "form_input_field\t{}\t{}\ttype\t{}",
                form_idx,
                field_idx,
                encode_nda_text(&field.input_type)
            ));
        }
    }

    for (idx, cookie) in cookies.iter().enumerate() {
        facts.push(format!("cookie\t{}", idx));
        facts.push(format!(
            "cookie_field\t{}\tname\t{}",
            idx,
            encode_nda_text(&cookie.name)
        ));
        facts.push(format!(
            "cookie_field\t{}\tvalue\t{}",
            idx,
            encode_nda_text(&cookie.value)
        ));
    }

    for (bucket_idx, bucket) in storage.iter().enumerate() {
        facts.push(format!("storage\t{}", bucket_idx));
        facts.push(format!(
            "storage_field\t{}\tscope\t{}",
            bucket_idx,
            encode_nda_text(&bucket.scope)
        ));
        for (entry_idx, (key, value)) in bucket.entries.iter().enumerate() {
            facts.push(format!("storage_entry\t{}\t{}", bucket_idx, entry_idx));
            facts.push(format!(
                "storage_entry_field\t{}\t{}\tkey\t{}",
                bucket_idx,
                entry_idx,
                encode_nda_text(key)
            ));
            facts.push(format!(
                "storage_entry_field\t{}\t{}\tvalue\t{}",
                bucket_idx,
                entry_idx,
                encode_nda_text(value)
            ));
        }
    }

    for (idx, mutation) in mutations.iter().enumerate() {
        facts.push(format!("mutation\t{}", idx));
        facts.push(format!(
            "mutation_field\t{}\tlabel\t{}",
            idx,
            encode_nda_text(mutation)
        ));
    }

    for (idx, request) in requests.iter().enumerate() {
        facts.push(format!("request\t{}", idx));
        facts.push(format!(
            "request_field\t{}\tmethod\t{}",
            idx,
            encode_nda_text(&request.method)
        ));
        facts.push(format!(
            "request_field\t{}\turl\t{}",
            idx,
            encode_nda_text(&request.url)
        ));
        facts.push(format!(
            "request_field\t{}\tstatus_code\t{}",
            idx, request.status_code
        ));
        facts.push(format!(
            "request_field\t{}\tresource\t{}",
            idx,
            encode_nda_text(&request.resource)
        ));
    }

    for (idx, settle) in settle_signals.iter().enumerate() {
        facts.push(format!("settle_signal\t{}", idx));
        facts.push(format!(
            "settle_signal_field\t{}\tlabel\t{}",
            idx,
            encode_nda_text(settle)
        ));
    }

    for (idx, entry) in runtime_state.iter().enumerate() {
        facts.push(format!("runtime_state\t{}", idx));
        facts.push(format!(
            "runtime_state_field\t{}\tscope\t{}",
            idx,
            encode_nda_text(&entry.scope)
        ));
        facts.push(format!(
            "runtime_state_field\t{}\tkey\t{}",
            idx,
            encode_nda_text(&entry.key)
        ));
        facts.push(format!(
            "runtime_state_field\t{}\tvalue\t{}",
            idx,
            encode_nda_text(&entry.value)
        ));
    }

    for (idx, event) in protocol_events.iter().enumerate() {
        facts.push(format!("protocol_event\t{}", idx));
        facts.push(format!(
            "protocol_event_field\t{}\tkind\t{}",
            idx,
            encode_nda_text(&event.kind)
        ));
        facts.push(format!(
            "protocol_event_field\t{}\tphase\t{}",
            idx,
            encode_nda_text(&event.phase)
        ));
        facts.push(format!(
            "protocol_event_field\t{}\ttarget\t{}",
            idx,
            encode_nda_text(&event.target)
        ));
        facts.push(format!(
            "protocol_event_field\t{}\tdetail\t{}",
            idx,
            encode_nda_text(&event.detail)
        ));
    }

    fs::write(&facts_path, facts.join("\n") + "\n")
        .map_err(|err| format!("write browser capture facts: {err}"))?;
    Ok(facts_path)
}

pub fn persist_snapshot_to_sitemap(
    snapshot: &BrowserPageSnapshot,
    sitemap_path: &Path,
) -> Result<(), String> {
    let mut sm =
        SiteMap::open(sitemap_path, 0).map_err(|e| format!("Failed to open SiteMap: {:?}", e))?;
    let page_hash = sm
        .register_string(&snapshot.url)
        .map_err(|e| e.to_string())?;
    let title_hash = sm
        .register_string(&snapshot.title)
        .map_err(|e| e.to_string())?;
    let summary_hash = sm
        .register_string(&snapshot.summary)
        .map_err(|e| e.to_string())?;

    let mut live_triples = vec![
        VcTriple {
            subject_hash: page_hash,
            predicate_id: 10,
            object_hash: page_hash,
        },
        VcTriple {
            subject_hash: page_hash,
            predicate_id: 11,
            object_hash: title_hash,
        },
        VcTriple {
            subject_hash: page_hash,
            predicate_id: 12,
            object_hash: summary_hash,
        },
    ];

    for triple in &live_triples {
        sm.put_node(&NdaNode::Triple {
            subject_hash: triple.subject_hash,
            predicate_id: triple.predicate_id,
            object_hash: triple.object_hash,
        })
        .map_err(|e| e.to_string())?;
    }

    let mut aom_node_hashes = Vec::new();
    for el in &snapshot.elements {
        let el_role_hash = sm.register_string(&el.role).map_err(|e| e.to_string())?;
        let el_name_hash = sm.register_string(&el.name).map_err(|e| e.to_string())?;
        let el_val_hash = sm.register_string(&el.value).map_err(|e| e.to_string())?;

        let mut hasher = Sha256::new();
        hasher.update(page_hash.to_le_bytes());
        hasher.update(el.role.as_bytes());
        hasher.update(el.name.as_bytes());
        let digest = hasher.finalize();
        let el_hash = u64::from_le_bytes(digest[0..8].try_into().unwrap());

        for triple in [
            VcTriple {
                subject_hash: el_hash,
                predicate_id: 16,
                object_hash: el_role_hash,
            },
            VcTriple {
                subject_hash: el_hash,
                predicate_id: 17,
                object_hash: el_name_hash,
            },
            VcTriple {
                subject_hash: el_hash,
                predicate_id: 18,
                object_hash: el_val_hash,
            },
        ] {
            sm.put_node(&NdaNode::Triple {
                subject_hash: triple.subject_hash,
                predicate_id: triple.predicate_id,
                object_hash: triple.object_hash,
            })
            .map_err(|e| e.to_string())?;
            live_triples.push(triple);
        }

        if let Some(target) = &el.target_url {
            let target_hash = sm.register_string(target).map_err(|e| e.to_string())?;
            let triple = VcTriple {
                subject_hash: page_hash,
                predicate_id: 1,
                object_hash: target_hash,
            };
            sm.put_node(&NdaNode::Triple {
                subject_hash: triple.subject_hash,
                predicate_id: triple.predicate_id,
                object_hash: triple.object_hash,
            })
            .map_err(|e| e.to_string())?;
            live_triples.push(triple);
        }

        aom_node_hashes.push(el_hash);
    }

    if !aom_node_hashes.is_empty() {
        let aom_root_node = NdaNode::Scope {
            children: aom_node_hashes
                .iter()
                .copied()
                .map(|target| NdaNode::Call { target })
                .collect(),
        };
        let root_hash = sm.put_node(&aom_root_node).map_err(|e| e.to_string())?;
        let triple = VcTriple {
            subject_hash: page_hash,
            predicate_id: 6,
            object_hash: root_hash,
        };
        sm.put_node(&NdaNode::Triple {
            subject_hash: triple.subject_hash,
            predicate_id: triple.predicate_id,
            object_hash: triple.object_hash,
        })
        .map_err(|e| e.to_string())?;
        live_triples.push(triple);
    }

    sm.put_file_snapshot(&format!("browser:{}", snapshot.url), &live_triples)
        .map_err(|e| e.to_string())?;
    sm.flush().map_err(|e| e.to_string())
}

