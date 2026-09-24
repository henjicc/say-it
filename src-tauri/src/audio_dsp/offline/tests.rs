use super::*;

#[test]
fn replacing_a_processing_request_stops_at_the_next_chunk() {
    use std::cell::Cell;
    let mut input = AudioBuffer::default();
    input.append(&vec![0.25; 800017]).unwrap();
    // 分别在内存期、第一遍已经落盘、第二遍已部分改写时取消。
    for stop_at in [3, 40, 56] {
        let checks = Cell::new(0);
        let result = process_cancellable(&input, 48000, &DspParams::default(), || {
            checks.set(checks.get() + 1);
            checks.get() == stop_at
        });
        assert!(result.is_err());
        assert_eq!(checks.get(), stop_at);
        assert_eq!(input.len(), 800017);
        assert!(input.to_vec().iter().all(|s| *s == 0.25));
    }
}

fn compare(input: &[f32], rate: u32, params: &DspParams) {
    let old = process_offline(input, rate, params);
    let mut source = AudioBuffer::default();
    for chunk in input.chunks(997) {
        source.append(chunk).unwrap();
    }
    let new = process(&source, rate, params).unwrap();
    let output = new.processed.to_vec();
    assert_eq!(output.len(), old.processed.len());
    for (i, (a, b)) in output.iter().zip(&old.processed).enumerate() {
        assert_eq!(
            a.to_bits(),
            b.to_bits(),
            "rate={rate} length={} index={i}",
            input.len()
        );
    }
    for (a, b) in [
        (new.in_lufs, old.in_lufs),
        (new.out_lufs, old.out_lufs),
        (new.in_peak_db, old.in_peak_db),
        (new.out_peak_db, old.out_peak_db),
    ] {
        assert_eq!(
            a.to_bits(),
            b.to_bits(),
            "统计不一致 rate={rate} length={}",
            input.len()
        );
    }
    assert_eq!(
        new.clipped_samples,
        old.processed.iter().filter(|s| s.abs() >= 0.999).count()
    );
}

#[test]
fn partitioned_offline_matches_reference_bits_for_rates_frames_and_parameters() {
    for rate in [8000, 16000, 44100, 48000, 96000] {
        for length in [0, 1, 479, 480, 481, 15359, 15360, 15361, 48017] {
            let input: Vec<_> = (0..length)
                .map(|i| ((i % 997) as f32 / 997.0 - 0.5) * 0.2)
                .collect();
            for params in [
                DspParams {
                    denoise_enabled: false,
                    ..Default::default()
                },
                DspParams {
                    denoise_strength: 0.35,
                    bass_gain_db: 5.0,
                    treble_gain_db: -3.0,
                    vad_gate: 0.2,
                    ..Default::default()
                },
            ] {
                compare(&input, rate, &params);
            }
        }
    }
}

#[test]
fn long_spilled_audio_silence_and_clipping_preserve_reference() {
    let input: Vec<_> = (0..600017)
        .map(|i| (i % 541) as f32 / 200.0 - 1.0)
        .collect();
    compare(&input, 48000, &DspParams::default());
    compare(
        &input,
        44100,
        &DspParams {
            denoise_enabled: false,
            target_lufs: -1.0,
            peak_limit_dbfs: 0.0,
            ..Default::default()
        },
    );
    compare(&[0.0; 48017], 16000, &DspParams::default());
    compare(
        &[-0.0; 48017],
        48000,
        &DspParams {
            denoise_enabled: false,
            ..Default::default()
        },
    );
}
