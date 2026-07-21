use velocity_browser::agentic::ZeroAllocNdaWriter;

#[test]
fn test_zero_alloc_nda_writer() {
    let mut buf = [0u8; 128];
    let mut writer = ZeroAllocNdaWriter::new(&mut buf);
    let bytes_written = writer.write_triple(b"sess_1", 100, b"https://example.com").unwrap();
    assert!(bytes_written > 0);
    assert_eq!(u16::from_le_bytes([buf[0], buf[1]]), 6); // Subject len
}
