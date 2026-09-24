use super::*;

#[test]
fn capture_snapshots_keep_their_prefix_when_recording_continues() {
    for count in [997, MEMORY_SAMPLES + 17] {
        let mut audio = AudioBuffer::default();
        audio.append(&vec![0.25; count]).unwrap();
        let snapshot = audio.snapshot();
        audio.append(&[-0.5; 17]).unwrap();
        assert_eq!(snapshot.len(), count);
        assert!(snapshot.to_vec().iter().all(|s| *s == 0.25));
        assert_eq!(&audio.to_vec()[count..], &[-0.5; 17]);
        drop(audio);
        assert!(snapshot.to_vec().iter().all(|s| *s == 0.25));
    }
}

#[test]
fn spilling_preserves_every_bit_and_bounds_memory_capacity() {
    let values = [
        0.0f32,
        -0.0,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::from_bits(0x7fc01234),
        0.25,
    ];
    let mut audio = AudioBuffer::default();
    let count = MEMORY_SAMPLES + 173;
    for start in (0..count).step_by(997) {
        let chunk: Vec<_> = (start..(start + 997).min(count))
            .map(|i| values[i % values.len()])
            .collect();
        audio.append(&chunk).unwrap();
        if let Storage::Memory(samples) = &audio.storage {
            assert!(samples.capacity() <= MEMORY_SAMPLES);
        }
    }
    let Storage::Disk(file) = &audio.storage else {
        panic!("长素材必须转存");
    };
    let path = file.path().to_owned();
    assert_eq!(file.len() as usize, count * 4);
    for (index, sample) in audio.to_vec().iter().enumerate() {
        assert_eq!(sample.to_bits(), values[index % values.len()].to_bits());
    }
    drop(audio);
    assert!(!path.exists());
}

#[test]
fn waveform_boundaries_and_cache_match_the_existing_projection() {
    for length in [0, 1, 479, 997, MEMORY_SAMPLES + 17] {
        let input: Vec<_> = (0..length)
            .map(|i| (i % 997) as f32 / 201.0 - 2.5)
            .collect();
        let mut audio = AudioBuffer::default();
        audio.append(&input).unwrap();
        for width in [1, 860, 1000] {
            let n = width.min(length);
            let expected: Vec<[f32; 2]> = (0..n)
                .map(|i| {
                    input[i * length / n..((i + 1) * length / n).max(i * length / n + 1)]
                        .iter()
                        .fold([1.0f32, -1.0f32], |[min, max], s| {
                            [min.min(*s), max.max(*s)]
                        })
                })
                .collect();
            assert_eq!(audio.waveform(width).unwrap(), expected);
            assert_eq!(audio.waveform(width).unwrap(), expected);
        }
        audio.append(&[-5.0]).unwrap();
        assert_eq!(audio.waveform(1).unwrap()[0][0], -5.0);
        audio.replace(audio.len() - 1, &[5.0]).unwrap();
        assert_eq!(audio.waveform(1).unwrap()[0][1], 5.0);
    }
}

#[test]
fn limits_truncation_and_write_errors_are_visible() {
    let mut audio = AudioBuffer::default();
    audio.len = MAX_SAMPLES;
    assert!(audio.append(&[0.1]).is_err());
    assert_eq!(audio.len(), MAX_SAMPLES);
    let mut audio = AudioBuffer {
        storage: Storage::Disk(Arc::new(TemporaryFile::read_only())),
        ..Default::default()
    };
    assert!(audio.append(&[0.1]).is_err());
    assert_eq!(audio.len(), 0);
    assert!(audio.replace(0, &[0.1]).is_err());
    let mut audio = AudioBuffer::default();
    audio.append(&vec![0.25; MEMORY_SAMPLES + 1]).unwrap();
    let Storage::Disk(file) = &audio.storage else {
        panic!()
    };
    file.truncate(0);
    assert!(audio.read(0, &mut [0.0; 1]).is_err());
    assert!(audio.waveform(860).is_err());
}
