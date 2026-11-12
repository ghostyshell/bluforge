//! End-to-end check of the Cinavia pipeline on a synthetic ridge field.
//!
//! Licensed discs cannot ship in the repo, so the test builds the artifact the
//! detector keys on: narrow ridges in 300-600 Hz over a quiet noise bed, at a
//! level the measurement treats as a watermark. It then strips them and
//! requires the score to fall.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;

use keepsake_core::cinavia;

fn ffmpeg_works() -> bool {
    Command::new(keepsake_core::rip::ffmpeg_path())
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn write_wav(path: &Path, samples: &[f32], rate: u32) -> std::io::Result<()> {
    let mut f = std::fs::File::create(path)?;
    let data = samples.len() as u32 * 4;
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&3u16.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&rate.to_le_bytes())?;
    f.write_all(&(rate * 4).to_le_bytes())?;
    f.write_all(&4u16.to_le_bytes())?;
    f.write_all(&32u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data.to_le_bytes())?;
    for s in samples {
        f.write_all(&s.to_le_bytes())?;
    }
    Ok(())
}

fn synth(rate: u32, secs: f32) -> Vec<f32> {
    let n = (rate as f32 * secs) as usize;
    let mut state = 0x2545_F491u32;
    let mut rnd = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        (state as f32 / u32::MAX as f32) - 0.5
    };
    // 12 ridges between 320 and 584 Hz, as the watermark's sidebands sit.
    let carriers: Vec<f32> = (0..12).map(|i| 320.0 + i as f32 * 24.0).collect();
    let mut last = 0.0f32;
    (0..n)
        .map(|i| {
            let t = i as f32 / rate as f32;
            // A slow envelope keeps the field from being perfectly stationary.
            let env = 0.6 + 0.4 * (2.0 * std::f32::consts::PI * 0.5 * t).sin();
            // Tilt the bed down with frequency so the in-band/high-band ratio
            // lands where real material sits.
            let bed = (rnd() * 0.05 + last * 0.995) * 0.5;
            last = bed;
            let mark: f32 = carriers
                .iter()
                .map(|f| (2.0 * std::f32::consts::PI * f * t).sin() * 0.05)
                .sum();
            bed + mark * env
        })
        .collect()
}

fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("keepsake-cinavia-test-{name}.wav"))
}

#[test]
fn detects_and_strips_a_ridge_field() {
    if !ffmpeg_works() {
        eprintln!("ffmpeg not available, skipping");
        return;
    }
    let src = tmp("src");
    let out = tmp("out");
    write_wav(&src, &synth(48_000, 8.0), 48_000).expect("write source");

    let before = cinavia::analyze(&src, 0, 1, Some(6)).expect("analyze source");
    assert!(
        before.density >= cinavia::LIKELY,
        "expected ridges, got {}",
        before.density
    );
    assert!(
        before.score >= cinavia::MASS,
        "expected mass, got {}",
        before.score
    );
    assert_ne!(before.verdict, cinavia::Verdict::Clean);

    let band = before.band.expect("band");
    let mut spec = cinavia::BandSpec::default_for(band);
    spec.depth_db = 14.0;
    cinavia::strip(&src, 0, &out, spec, 1, &AtomicBool::new(false), |_| {}).expect("strip");

    let after = cinavia::analyze(&out, 0, 1, Some(6)).expect("analyze output");
    assert!(
        after.band_ratio_db < before.band_ratio_db - 8.0,
        "in-band level should fall: {} -> {}",
        before.band_ratio_db,
        after.band_ratio_db
    );
    assert!(
        after.density < before.density,
        "ridge count should fall: {} -> {}",
        before.density,
        after.density
    );

    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&out);
}

#[test]
fn a_clean_bed_reads_clean() {
    if !ffmpeg_works() {
        eprintln!("ffmpeg not available, skipping");
        return;
    }
    let src = tmp("clean");
    let mut state = 0x9e37_79b9u32;
    let samples: Vec<f32> = (0..48_000 * 6)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            ((state as f32 / u32::MAX as f32) - 0.5) * 0.01
        })
        .collect();
    write_wav(&src, &samples, 48_000).expect("write");

    let report = cinavia::analyze(&src, 0, 1, Some(4)).expect("analyze");
    assert!(
        report.band_ratio_db < cinavia::RATIO_CLEAN,
        "plain noise should not trip the detector, scored {}",
        report.score
    );
    assert_eq!(report.verdict, cinavia::Verdict::Clean);
    let _ = std::fs::remove_file(&src);
}
