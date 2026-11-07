use super::*;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wav/s16_44100.wav")
}

fn all(decoder: &mut Decoder) -> Vec<PcmSample> {
    let mut output = Vec::new();
    while decoder.next_block(&mut output).unwrap() > 0 {}
    output
}

// Simulate a reader reporting playable length and trimming metadata that its codec ignores.
fn untrimmed_reader(delay: u32, padding: u32) -> Decoder {
    let mut decoder = Decoder::open(&fixture()).unwrap();
    decoder.manual_delay = u64::from(delay);
    decoder.skip = u64::from(delay);
    decoder.manual_padding = padding as usize;
    decoder.info.frames = decoder
        .info
        .frames
        .map(|frames| frames - u64::from(delay + padding));
    decoder.info.encoder_delay = EncoderDelay {
        delay_frames: Some(delay),
        padding_frames: Some(padding),
        trimmed: true,
    };
    decoder
}

#[test]
fn reported_untrimmed_delay_and_padding_are_removed_across_blocks() {
    let reference = all(&mut Decoder::open(&fixture()).unwrap());
    for (delay, padding) in [(0, 17), (123, 0), (123, 5000)] {
        let mut decoder = untrimmed_reader(delay, padding);
        let actual = all(&mut decoder);
        assert_eq!(
            actual,
            reference[delay as usize * 2..reference.len() - padding as usize * 2]
        );
        assert_eq!(decoder.position(), decoder.info().frames.unwrap());
        assert!(decoder.padding_tail.is_empty());
    }
}

#[test]
fn seeking_reported_untrimmed_audio_uses_audible_positions() {
    let reference = all(&mut Decoder::open(&fixture()).unwrap());
    let mut decoder = untrimmed_reader(123, 5000);
    assert_eq!(decoder.seek(1000).unwrap(), 1000);
    assert_eq!(
        all(&mut decoder),
        reference[(123 + 1000) * 2..reference.len() - 5000 * 2]
    );
}
