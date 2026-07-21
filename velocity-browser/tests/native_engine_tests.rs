use velocity_browser::engine::DeviceProfile;

#[test]
fn test_device_profile_export() {
    let profile = DeviceProfile::velocity_native();
    let triples = profile.export_profile_nda("sess_profile");
    assert_eq!(triples.len(), 4);
    assert_eq!(triples[0].predicate_id, 110);
}
