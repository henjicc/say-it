use super::*;
#[test]
fn floating_point_bits_roundtrip_across_segments_and_files_are_owned() {
    let mut writer = Writer::default();
    let mut reader = Reader::default();
    let mut samples = vec![0.25; 4096];
    samples[..6].copy_from_slice(&[
        f32::from_bits(0x7fc01234),
        f32::INFINITY,
        f32::NEG_INFINITY,
        -0.0,
        0.0,
        -0.75,
    ]);
    let mut packets = Vec::new();
    for _ in 0..520 {
        packets.push(writer.write(&samples, || false).unwrap());
    }
    assert_eq!(writer.paths.lock().unwrap().len(), 2);
    let paths = writer.paths.lock().unwrap().clone();
    drop(writer);
    for packet in packets {
        let output = reader.read(packet).unwrap();
        assert_eq!(
            output.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
            samples.iter().map(|x| x.to_bits()).collect::<Vec<_>>()
        );
    }
    drop(reader);
    assert!(paths.iter().all(|path| !path.exists()));
}
#[test]
fn cancellation_and_truncated_file_are_errors_not_partial_audio() {
    let mut writer = Writer::default();
    assert!(writer.write(&[0.25; 4096], || true).is_err());
    let packet = writer.write(&[0.25; 4096], || false).unwrap();
    let path = packet.segment.path.clone();
    drop(writer);
    OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(4)
        .unwrap();
    let mut reader = Reader::default();
    assert!(reader.read(packet).unwrap_err().contains("不完整"));
    drop(reader);
    assert!(!path.exists());
}

#[test]
fn disk_budget_counts_files_retained_by_any_reader_or_ticket() {
    let mut writer = Writer::default();
    let budget = writer.disk_bytes.clone();
    let packet = writer.write(&[0.25; 4096], || false).unwrap();
    let path = packet.segment.path.clone();
    drop(writer);
    assert_eq!(budget.load(Ordering::Acquire), 4096 * 4);
    drop(packet);
    assert_eq!(budget.load(Ordering::Acquire), 0);
    assert!(!path.exists());

    let mut writer = Writer::default();
    writer.disk_bytes.store(DISK_BYTES, Ordering::Release);
    assert!(writer
        .write(&[0.25; 4096], || false)
        .err()
        .unwrap()
        .contains("磁盘预算"));
    let path = writer.output.as_ref().unwrap().segment.path.clone();
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
    drop(writer);
    assert!(!path.exists());
}
