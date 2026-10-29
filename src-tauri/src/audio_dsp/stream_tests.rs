use super::stream_reference::ReferenceStreamDsp;
use super::*;

#[test]
fn finish_preserves_duration_for_short_partial_and_resampled_input() {
    for rate in [0, 8_000, 16_000, 22_050, 44_100, 48_000, 96_000, 192_000] {
        let actual_rate = if rate == 0 { 48_000 } else { rate };
        for length in [0, 1, 2, 3, 159, 160, 479, 480, 481, 997, 80_000] {
            for denoise_enabled in [false, true] {
                let input: Vec<_> = (0..length).map(|i| (i % 37) as f32 * 0.001).collect();
                let params = DspParams { denoise_enabled, ..Default::default() };
                let mut whole = StreamDsp::new(params.clone(), rate);
                let mut expected = whole.process(&input);
                expected.extend(whole.finish());
                assert_eq!(expected.len() / 2,
                    (length * RATE_16K as usize + actual_rate as usize / 2) / actual_rate as usize,
                    "rate={rate}, length={length}, denoise={denoise_enabled}");
                let mut split = StreamDsp::new(params, rate);
                let mut actual = Vec::new();
                for packet in input.chunks(137) { actual.extend(split.process(packet)); }
                actual.extend(split.finish());
                assert_eq!(actual, expected, "finish must not depend on input packet boundaries");
                assert!(split.finish().is_empty());
                assert!(split.process(&[0.1; 480]).is_empty());
            }
        }
    }
}

#[test]
fn finish_outputs_actual_tail_with_existing_dsp_processing() {
    // 不是仅补出正确字节数：尾部必须与继续输入时的实际处理样本一致。
    for rate in [16_000, 44_100, 48_000] {
        for params in [DspParams::default(), DspParams { denoise_enabled: false, ..Default::default() }] {
            let input: Vec<_> = (0..rate + 97).map(|i| ((i % 79) as f32 - 39.0) * 0.003).collect();
            let mut actual = StreamDsp::new(params.clone(), rate);
            let mut reference = ReferenceStreamDsp::new(params, rate);
            assert_eq!(actual.process(&input), reference.process(&input));
            let tail = actual.finish();
            // 非48k先延展最后一个输入样本以完成线性插值，其后补零仅供完整帧运算。
            let mut continued = if rate == RATE_48K { Vec::new() }
                else { reference.process(&[*input.last().unwrap()]) };
            continued.extend(reference.process(&vec![0.0; rate as usize / 100 + 1]));
            // 48k 不需要插值，补零帧与 finish 必须逐字节一致。
            if rate == RATE_48K { assert_eq!(tail, continued[..tail.len()]); }
            assert!(!tail.is_empty());
            if !actual.params.denoise_enabled { assert!(tail.iter().any(|b| *b != 0)); }
        }
    }
}

#[test]
fn fixed_frames_match_legacy_output_across_rates_settings_and_packet_boundaries() {
    for rate in [0, 8_000, 16_000, 22_050, 44_100, 48_000, 96_000, 192_000] {
        let actual_rate = if rate == 0 { 48_000 } else { rate };
        let input: Vec<f32> = (0..actual_rate * 2 + 997)
            .map(|i| {
                let level = if i < actual_rate / 2 { 0.0005 } else { 0.3 };
                ((i * 1777 % 65535) as f32 / 32768.0 - 1.0) * level
            })
            .collect();
        for params in [
            DspParams {
                denoise_enabled: false,
                bass_gain_db: 3.0,
                treble_gain_db: -2.0,
                ..Default::default()
            },
            DspParams::default(),
            DspParams {
                denoise_strength: 0.65,
                vad_gate: 0.02,
                bass_gain_db: -3.0,
                treble_gain_db: 2.0,
                target_lufs: -14.0,
                max_gain_db: 20.0,
                peak_limit_dbfs: -3.0,
                ..Default::default()
            },
        ] {
            for packets in [
                vec![input.len()],
                vec![4096],
                vec![1, 7, 479, 480, 481, 8193],
            ] {
                let mut reference = ReferenceStreamDsp::new(params.clone(), rate);
                let mut actual = StreamDsp::new(params.clone(), rate);
                let mut offset = 0;
                let mut round = 0;
                while offset < input.len() {
                    let length = packets[round % packets.len()].min(input.len() - offset);
                    let part = &input[offset..offset + length];
                    assert_eq!(
                        actual.process(part),
                        reference.process(part),
                        "rate={rate}, offset={offset}, length={length}, params={params:?}"
                    );
                    assert!(actual.filled48 < FRAME);
                    assert!(actual.process(&[]).is_empty());
                    offset += length;
                    round += 1;
                }
                // 用后续输入跨过最后一个未完成帧，检查尾部既没补零也没丢失。
                let tail = vec![0.01; actual_rate as usize / 10 + 1];
                assert_eq!(actual.process(&tail), reference.process(&tail));
            }
        }
    }
}

#[test]
fn one_sample_packets_and_partial_frames_preserve_sign_and_silence() {
    for rate in [8_000, 44_100, 48_000, 192_000] {
        let params = DspParams {
            denoise_enabled: false,
            ..Default::default()
        };
        let mut reference = ReferenceStreamDsp::new(params.clone(), rate);
        let mut actual = StreamDsp::new(params, rate);
        for index in 0..6001 {
            let input = [if index % 2 == 0 { -0.0 } else { 0.001 }];
            assert_eq!(actual.process(&input), reference.process(&input));
            assert!(actual.filled48 < FRAME);
        }
    }
}
