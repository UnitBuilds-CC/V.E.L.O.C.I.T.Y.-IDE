#[cfg(test)]
use std::fs;
#[cfg(test)]
use tempfile::TempDir;

#[cfg(test)]
use super::store::SiteMap;
#[cfg(test)]
use super::types::VcTriple;
#[cfg(test)]
use super::verifier::NdaNode;
#[cfg(test)]
use crate::nda_int::NdaVec;

#[cfg(test)]
fn make_ndavec(len: usize, val: u8) -> NdaVec {
    let bytes = len.div_ceil(8);
    NdaVec {
        len,
        log2_scale: 0,
        sign: vec![val; bytes].into(),
        extra: vec![val; bytes].into(),
    }
}

#[test]
fn round_trip_kv() {
    let dir = TempDir::new().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0xDEAD).unwrap();
    let k = make_ndavec(16, 0xAA);
    let v = make_ndavec(16, 0x55);
    let hash = sm.put_kv(42, 0, k.clone(), v.clone()).unwrap();
    let (kr, vr) = sm.get_kv(42, 0).unwrap();
    assert_eq!(kr.sign, k.sign);
    assert_eq!(vr.sign, v.sign);
    let hash2 = sm.put_kv(42, 0, k, v).unwrap();
    assert_eq!(hash, hash2);
}

#[test]
fn put_node_is_idempotent() {
    let dir = TempDir::new().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();
    let n = NdaNode::Int { value: 99 };
    let h1 = sm.put_node(&n).unwrap();
    let h2 = sm.put_node(&n).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(sm.len(), 1);
}

#[test]
fn flush_and_reload() {
    let dir = TempDir::new().unwrap();
    {
        let mut sm = SiteMap::open(dir.path(), 0).unwrap();
        let k = make_ndavec(8, 0xFF);
        let v = make_ndavec(8, 0x00);
        sm.put_kv(7, 0, k, v).unwrap();
        sm.flush().unwrap();
    }
    let mut sm2 = SiteMap::open(dir.path(), 0).unwrap();
    assert_eq!(sm2.len(), 1);
    assert!(sm2.get_kv(7, 0).is_some());
}

#[test]
fn verify_detects_corruption() {
    let dir = TempDir::new().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();
    let k = make_ndavec(8, 0xAA);
    let v = make_ndavec(8, 0xBB);
    sm.put_kv(1, 0, k, v).unwrap();
    sm.flush().unwrap();
    let entry = sm.index.values().next().unwrap();
    let path = dir.path().join(&entry.file);
    let mut data = fs::read(&path).unwrap();
    data[4] ^= 0xFF;
    fs::write(&path, &data).unwrap();
    assert_eq!(sm.verify(), 1);
}

#[test]
fn weight_root_change_invalidates_token_hashes() {
    let dir = TempDir::new().unwrap();
    let sm1 = SiteMap::open(dir.path(), 0x0001).unwrap();
    let sm2 = SiteMap::open(dir.path(), 0x0002).unwrap();
    assert_ne!(sm1.token_hash(42, 0), sm2.token_hash(42, 0));
}

#[test]
fn stats_display() {
    let dir = TempDir::new().unwrap();
    let sm = SiteMap::open(dir.path(), 0).unwrap();
    let s = sm.stats();
    println!("{s}");
    assert_eq!(s.kv, 0);
}

#[test]
fn persists_weight_root_to_nda_metadata() {
    let dir = TempDir::new().unwrap();
    let sm = SiteMap::open(dir.path(), 0x1234_ABCD).unwrap();
    sm.flush().unwrap();

    let metadata = fs::read_to_string(dir.path().join("metadata.nda")).unwrap();
    assert!(metadata.contains("metadata version 2"));
    assert!(metadata.contains("field_count 1"));
    assert!(metadata.contains("field\tweight_root\t000000001234abcd"));
    assert_eq!(
        SiteMap::read_persisted_weight_root(dir.path()),
        Some(0x1234_ABCD)
    );
}

#[test]
fn prefers_nda_weight_root_over_json_metadata() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("metadata.nda"),
        "metadata version 2\nfield_count 1\nfield\tweight_root\t00000000000000aa\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("metadata.json"),
        "{\n  \"weight_root\": \"00000000000000bb\"\n}",
    )
    .unwrap();

    assert_eq!(SiteMap::read_persisted_weight_root(dir.path()), Some(0xAA));
}

#[test]
fn falls_back_to_json_weight_root_metadata() {
    let dir = TempDir::new().unwrap();
    fs::write(
        dir.path().join("metadata.json"),
        "{\n  \"weight_root\": \"00000000000000cc\"\n}",
    )
    .unwrap();

    assert_eq!(SiteMap::read_persisted_weight_root(dir.path()), Some(0xCC));
}

#[test]
fn round_trip_triple_node() {
    let dir = TempDir::new().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();
    let n = NdaNode::Triple {
        subject_hash: 0xAAAA_BBBB_CCCC_DDDD,
        predicate_id: 42,
        object_hash: 0x1111_2222_3333_4444,
    };
    let hash = sm.put_node(&n).unwrap();
    let n_decoded = sm.get_node(hash).unwrap();
    match n_decoded {
        NdaNode::Triple {
            subject_hash,
            predicate_id,
            object_hash,
        } => {
            assert_eq!(subject_hash, 0xAAAA_BBBB_CCCC_DDDD);
            assert_eq!(predicate_id, 42);
            assert_eq!(object_hash, 0x1111_2222_3333_4444);
        }
        _ => panic!("Decoded node is not a Triple!"),
    }
}

#[test]
fn test_graph_query_engine() {
    let dir = TempDir::new().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();

    let t1 = NdaNode::Triple {
        subject_hash: 1,
        predicate_id: 2,
        object_hash: 2,
    };
    let t2 = NdaNode::Triple {
        subject_hash: 2,
        predicate_id: 2,
        object_hash: 3,
    };
    let t3 = NdaNode::Triple {
        subject_hash: 1,
        predicate_id: 2,
        object_hash: 3,
    };

    let program = NdaNode::Scope {
        children: vec![t1, t2, t3],
    };
    sm.put_node(&program).unwrap();

    let triples = sm.find_triples(Some(1), Some(2), None);
    assert_eq!(triples.len(), 2);

    sm.put_file_snapshot(
        "src/main.rs",
        &[
            VcTriple {
                subject_hash: 1,
                predicate_id: 2,
                object_hash: 2,
            },
            VcTriple {
                subject_hash: 1,
                predicate_id: 2,
                object_hash: 3,
            },
        ],
    )
    .unwrap();
    let live_triples = sm.find_live_triples(Some(1), Some(2), None);
    assert_eq!(live_triples.len(), 2);

    let callers = sm.get_callers(3);
    assert_eq!(callers.len(), 1);
    assert!(callers.contains(&1));

    let deps = sm.get_dependencies(1);
    assert_eq!(deps.len(), 2);
    assert!(deps.contains(&2));
    assert!(deps.contains(&3));
}

#[test]
fn file_snapshots_replace_live_semantic_state() {
    let dir = TempDir::new().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();

    sm.put_file_snapshot(
        "src/main.rs",
        &[VcTriple {
            subject_hash: 10,
            predicate_id: 2,
            object_hash: 20,
        }],
    )
    .unwrap();
    assert_eq!(sm.find_live_triples(Some(10), Some(2), None).len(), 1);

    sm.put_file_snapshot(
        "src/main.rs",
        &[VcTriple {
            subject_hash: 10,
            predicate_id: 2,
            object_hash: 30,
        }],
    )
    .unwrap();
    let live = sm.find_live_triples(Some(10), Some(2), None);
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].object_hash, 30);

    assert!(sm.remove_file_snapshot("src/main.rs").unwrap());
    assert!(sm.find_live_triples(Some(10), Some(2), None).is_empty());
}

#[test]
fn batch_kv_insert_and_root_recomputed_once() {
    let dir = TempDir::new().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();
    let items: Vec<_> = (0..5)
        .map(|i| (i as u32, 0u32, make_ndavec(8, i as u8), make_ndavec(8, (i + 10) as u8)))
        .collect();
    let keys = sm.put_kv_batch(&items).unwrap();
    assert_eq!(keys.len(), 5);
    assert_eq!(sm.stats().kv, 5);
    // All keys should be retrievable
    for (i, key) in keys.iter().enumerate() {
        let (k, v) = sm.get_kv(i as u32, 0).unwrap();
        assert_eq!(k.sign, items[i].2.sign);
        assert_eq!(v.sign, items[i].3.sign);
    }
    // Root should be non-zero after inserts
    assert_ne!(sm.root(), 0);
}

#[test]
fn batch_nodes_insert() {
    let dir = TempDir::new().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();
    let n1 = NdaNode::Int { value: 1 };
    let n2 = NdaNode::Int { value: 2 };
    let n3 = NdaNode::Int { value: 3 };
    let keys = sm.put_nodes_batch(&[&n1, &n2, &n3]).unwrap();
    assert_eq!(keys.len(), 3);
    assert_eq!(sm.len(), 3);
    // Each node should be retrievable
    for (i, node) in [&n1, &n2, &n3].iter().enumerate() {
        let retrieved = sm.get_node(keys[i]).unwrap();
        match (&retrieved, node) {
            (NdaNode::Int { value: a }, NdaNode::Int { value: b }) => assert_eq!(a, b),
            _ => panic!("unexpected node type"),
        }
    }
}

#[test]
fn batch_register_strings() {
    let dir = TempDir::new().unwrap();
    let sm = SiteMap::open(dir.path(), 0).unwrap();
    let strings = vec!["hello", "world", "foo", "bar"];
    let hashes = sm.register_strings_batch(&strings).unwrap();
    assert_eq!(hashes.len(), 4);
    // Each hash should resolve back
    let resolved = sm.resolve_strings_batch(&hashes);
    for (i, opt) in resolved.iter().enumerate() {
        assert_eq!(opt.as_deref(), Some(strings[i]));
    }
    // Re-registering same strings should return same hashes
    let hashes2 = sm.register_strings_batch(&strings).unwrap();
    assert_eq!(hashes, hashes2);
}

#[test]
fn batch_file_snapshots() {
    let dir = TempDir::new().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();
    let h1 = sm.register_string("src/a.rs").unwrap();
    let h2 = sm.register_string("src/b.rs").unwrap();
    let sym1 = sm.register_string("fn_a").unwrap();
    let sym2 = sm.register_string("fn_b").unwrap();
    let snap_a = vec![VcTriple {
        subject_hash: h1,
        predicate_id: 1,
        object_hash: sym1,
    }];
    let snap_b = vec![VcTriple {
        subject_hash: h2,
        predicate_id: 1,
        object_hash: sym2,
    }];
    let keys = sm
        .put_file_snapshots_batch(&[("src/a.rs", &snap_a), ("src/b.rs", &snap_b)])
        .unwrap();
    assert_eq!(keys.len(), 2);
    assert_eq!(sm.stats().snapshots, 2);
    // Live triples should include both
    let live = sm.find_live_triples(None, Some(1), None);
    assert_eq!(live.len(), 2);
}

#[test]
fn entries_by_kind_and_largest() {
    let dir = TempDir::new().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();
    // Insert 2 KV and 1 node
    sm.put_kv(1, 0, make_ndavec(8, 0xAA), make_ndavec(8, 0xBB))
        .unwrap();
    sm.put_kv(2, 0, make_ndavec(8, 0xCC), make_ndavec(8, 0xDD))
        .unwrap();
    let n = NdaNode::Int { value: 42 };
    sm.put_node(&n).unwrap();

    let kv_entries = sm.entries_by_kind(&super::types::EntryKind::Kv);
    assert_eq!(kv_entries.len(), 2);
    let node_entries = sm.entries_by_kind(&super::types::EntryKind::Node);
    assert_eq!(node_entries.len(), 1);

    let top2 = sm.largest_entries(2);
    assert_eq!(top2.len(), 2);
    assert!(top2[0].size >= top2[1].size);
}

#[test]
fn stats_include_cache_and_dict_sizes() {
    let dir = TempDir::new().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();
    sm.register_string("test_str").unwrap();
    sm.put_kv(1, 0, make_ndavec(8, 0), make_ndavec(8, 0))
        .unwrap();
    let s = sm.stats();
    assert_eq!(s.kv_cache_size, 1);
    assert_eq!(s.string_dict_size, 1);
    assert_eq!(s.total_entries, 1);
    // Serialisable
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("kv_cache_size"));
    assert!(json.contains("string_dict_size"));
}

#[test]
fn sitemap_validate_empty() {
    let dir = tempfile::tempdir().unwrap();
    let sm = SiteMap::open(dir.path(), 0).unwrap();
    let w = sm.validate();
    assert!(!w.is_empty()); // empty index triggers warning
    assert!(w[0].contains("empty"));
}

#[test]
fn sitemap_validate_with_entries() {
    let dir = tempfile::tempdir().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();
    sm.put_kv(1, 0, make_ndavec(8, 0), make_ndavec(8, 0))
        .unwrap();
    let w = sm.validate();
    assert!(w.is_empty(), "expected no warnings, got: {:?}", w);
}

#[test]
fn sitemap_info_and_summary() {
    let dir = tempfile::tempdir().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();
    sm.put_kv(1, 0, make_ndavec(8, 0), make_ndavec(8, 0))
        .unwrap();
    let info = sm.info();
    assert_eq!(info.total_entries, 1);
    assert_eq!(info.kv_count, 1);
    assert!(info.validation_issues.is_empty());

    let summary = sm.export_summary();
    assert!(summary.validation_clean);
    let json = serde_json::to_string(&summary).unwrap();
    assert!(json.contains("validation_clean"));
}

#[test]
fn sitemap_verify_detailed_clean() {
    let dir = tempfile::tempdir().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();
    sm.put_kv(1, 0, make_ndavec(8, 0), make_ndavec(8, 0))
        .unwrap();
    let report = sm.verify_detailed();
    assert_eq!(report.checked, 1);
    assert_eq!(report.valid, 1);
    assert_eq!(report.corrupt, 0);
    assert_eq!(report.missing, 0);
}

#[test]
fn sitemap_cache_utilization() {
    let dir = tempfile::tempdir().unwrap();
    let sm = SiteMap::open(dir.path(), 0).unwrap();
    assert_eq!(sm.cache_utilization(), 0.0);
}

#[test]
fn sitemap_info_serializes() {
    let dir = tempfile::tempdir().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();
    sm.put_kv(1, 0, make_ndavec(8, 0), make_ndavec(8, 0))
        .unwrap();
    let info = sm.info();
    let json = serde_json::to_string(&info).unwrap();
    assert!(json.contains("\"kv_count\":1"));
    assert!(json.contains("\"total_entries\":1"));
}

#[test]
fn find_live_by_predicate() {
    let dir = TempDir::new().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();
    sm.put_file_snapshot(
        "src/a.rs",
        &[
            VcTriple { subject_hash: 1, predicate_id: 2, object_hash: 10 },
            VcTriple { subject_hash: 2, predicate_id: 3, object_hash: 20 },
            VcTriple { subject_hash: 3, predicate_id: 2, object_hash: 30 },
        ],
    ).unwrap();
    let pred2 = sm.find_live_by_predicate(2);
    assert_eq!(pred2.len(), 2);
    assert!(pred2.iter().all(|t| t.predicate_id == 2));
    let pred3 = sm.find_live_by_predicate(3);
    assert_eq!(pred3.len(), 1);
    let pred99 = sm.find_live_by_predicate(99);
    assert!(pred99.is_empty());
}

#[test]
fn find_live_by_subjects_batch() {
    let dir = TempDir::new().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();
    sm.put_file_snapshot(
        "src/a.rs",
        &[
            VcTriple { subject_hash: 1, predicate_id: 2, object_hash: 10 },
            VcTriple { subject_hash: 2, predicate_id: 2, object_hash: 20 },
            VcTriple { subject_hash: 3, predicate_id: 2, object_hash: 30 },
        ],
    ).unwrap();
    let results = sm.find_live_by_subjects(&[1, 3]);
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|t| t.subject_hash == 1 || t.subject_hash == 3));
}

#[test]
fn find_live_by_objects_batch() {
    let dir = TempDir::new().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();
    sm.put_file_snapshot(
        "src/a.rs",
        &[
            VcTriple { subject_hash: 1, predicate_id: 2, object_hash: 10 },
            VcTriple { subject_hash: 2, predicate_id: 2, object_hash: 20 },
            VcTriple { subject_hash: 3, predicate_id: 2, object_hash: 10 },
        ],
    ).unwrap();
    let results = sm.find_live_by_objects(&[10]);
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|t| t.object_hash == 10));
}

#[test]
fn find_live_by_subject_and_predicate() {
    let dir = TempDir::new().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();
    sm.put_file_snapshot(
        "src/a.rs",
        &[
            VcTriple { subject_hash: 1, predicate_id: 2, object_hash: 10 },
            VcTriple { subject_hash: 1, predicate_id: 3, object_hash: 20 },
            VcTriple { subject_hash: 2, predicate_id: 2, object_hash: 30 },
        ],
    ).unwrap();
    let results = sm.find_live_by_subject_and_predicate(1, 2);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].object_hash, 10);
}

#[test]
fn find_live_by_predicate_and_object() {
    let dir = TempDir::new().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();
    sm.put_file_snapshot(
        "src/a.rs",
        &[
            VcTriple { subject_hash: 1, predicate_id: 2, object_hash: 10 },
            VcTriple { subject_hash: 2, predicate_id: 2, object_hash: 10 },
            VcTriple { subject_hash: 3, predicate_id: 3, object_hash: 10 },
        ],
    ).unwrap();
    let results = sm.find_live_by_predicate_and_object(2, 10);
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|t| t.predicate_id == 2 && t.object_hash == 10));
}

#[test]
fn invalidate_predicate_index_works() {
    let dir = TempDir::new().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();
    sm.put_file_snapshot(
        "src/a.rs",
        &[VcTriple { subject_hash: 1, predicate_id: 2, object_hash: 10 }],
    ).unwrap();
    // Build the index
    let r1 = sm.find_live_by_predicate(2);
    assert_eq!(r1.len(), 1);
    // Invalidate and re-query
    sm.invalidate_predicate_index();
    let r2 = sm.find_live_by_predicate(2);
    assert_eq!(r2.len(), 1);
}

#[test]
fn entry_kind_name_works() {
    assert_eq!(SiteMap::entry_kind_name(&super::types::EntryKind::Kv), "KV");
    assert_eq!(SiteMap::entry_kind_name(&super::types::EntryKind::Node), "Node");
    assert_eq!(SiteMap::entry_kind_name(&super::types::EntryKind::Program), "Program");
    assert_eq!(SiteMap::entry_kind_name(&super::types::EntryKind::Snapshot), "Snapshot");
}

#[test]
fn validate_entry_clean() {
    let entry = super::types::SiteMapEntry {
        kind: super::types::EntryKind::Kv,
        hash: 0x1234,
        file: "kv/0000000000001234.kv".to_string(),
        file_sha: "abcdef1234567890".to_string(),
        size: 100,
    };
    let issues = SiteMap::validate_entry(&entry);
    assert!(issues.is_empty());
}

#[test]
fn validate_entry_empty_fields() {
    let entry = super::types::SiteMapEntry {
        kind: super::types::EntryKind::Kv,
        hash: 0x1234,
        file: "".to_string(),
        file_sha: "".to_string(),
        size: 0,
    };
    let issues = SiteMap::validate_entry(&entry);
    assert_eq!(issues.len(), 3);
}

#[test]
fn triple_analysis_with_snapshots() {
    let dir = TempDir::new().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();
    sm.put_file_snapshot(
        "src/a.rs",
        &[
            VcTriple { subject_hash: 1, predicate_id: 2, object_hash: 10 },
            VcTriple { subject_hash: 2, predicate_id: 2, object_hash: 20 },
            VcTriple { subject_hash: 1, predicate_id: 3, object_hash: 30 },
        ],
    ).unwrap();
    let analysis = sm.triple_analysis();
    assert_eq!(analysis.total_triples, 3);
    assert_eq!(analysis.unique_predicates, 2);
    assert_eq!(analysis.unique_subjects, 2);
    assert_eq!(analysis.unique_objects, 3);
    // Predicate 2 should have highest count
    assert_eq!(analysis.predicate_distribution[0].predicate_id, 2);
    assert_eq!(analysis.predicate_distribution[0].count, 2);
}

#[test]
fn triple_analysis_empty() {
    let dir = TempDir::new().unwrap();
    let sm = SiteMap::open(dir.path(), 0).unwrap();
    let analysis = sm.triple_analysis();
    assert_eq!(analysis.total_triples, 0);
    assert_eq!(analysis.unique_predicates, 0);
}

#[test]
fn put_kv_reported_works() {
    let dir = TempDir::new().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();
    let k = make_ndavec(8, 0xAA);
    let v = make_ndavec(8, 0xBB);
    let (key, report) = sm.put_kv_reported(1, 0, k, v).unwrap();
    assert!(key != 0);
    assert_eq!(report.operation, "put_kv");
    assert_eq!(report.entries_affected, 1);
    assert_eq!(report.total_entries_after, 1);
    assert!(report.elapsed_us >= 1);
}

#[test]
fn put_node_reported_works() {
    let dir = TempDir::new().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();
    let n = NdaNode::Int { value: 42 };
    let (key, report) = sm.put_node_reported(&n).unwrap();
    assert!(key != 0);
    assert_eq!(report.operation, "put_node");
    assert_eq!(report.total_entries_after, 1);
}

#[test]
fn flush_reported_works() {
    let dir = TempDir::new().unwrap();
    let mut sm = SiteMap::open(dir.path(), 0).unwrap();
    sm.put_kv(1, 0, make_ndavec(8, 0), make_ndavec(8, 0)).unwrap();
    let report = sm.flush_report().unwrap();
    assert_eq!(report.operation, "flush");
    assert_eq!(report.entries_affected, 1);
}

#[test]
fn store_operation_report_serializes() {
    let report = super::store::StoreOperationReport {
        operation: "put_kv".to_string(),
        elapsed_us: 123,
        entries_affected: 1,
        total_entries_after: 10,
    };
    let json = serde_json::to_string(&report).unwrap();
    assert!(json.contains("\"operation\":\"put_kv\""));
    assert!(json.contains("\"elapsed_us\":123"));
}

#[test]
fn triple_analysis_serializes() {
    let analysis = super::store::TripleAnalysis {
        total_triples: 10,
        unique_predicates: 2,
        predicate_distribution: vec![
            super::store::PredicateCount { predicate_id: 2, count: 7 },
            super::store::PredicateCount { predicate_id: 3, count: 3 },
        ],
        unique_subjects: 5,
        unique_objects: 8,
    };
    let json = serde_json::to_string(&analysis).unwrap();
    assert!(json.contains("\"total_triples\":10"));
    assert!(json.contains("\"unique_predicates\":2"));
}

#[test]
fn all_strings_returns_registered() {
    let dir = TempDir::new().unwrap();
    let sm = SiteMap::open(dir.path(), 0).unwrap();
    sm.register_string("hello").unwrap();
    sm.register_string("world").unwrap();
    let all = sm.all_strings();
    assert_eq!(all.len(), 2);
    assert!(all.values().any(|v| v == "hello"));
    assert!(all.values().any(|v| v == "world"));
}
