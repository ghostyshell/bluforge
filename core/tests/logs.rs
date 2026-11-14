//! A failure before ffmpeg starts must still leave a reason in the log, not
//! just at the outer boundary.

use std::io::Write;
use std::sync::{Arc, Mutex};

use keepsake_core::rip::{AudioSel, Container, RipMode, RipRequest, SubSel};

#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Sink {
    type Writer = Sink;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[test]
fn an_unopenable_disc_logs_where_it_failed() {
    let sink = Sink::default();
    let writer = sink.clone();
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_writer(writer)
        .with_ansi(false)
        .try_init();

    let req = RipRequest {
        source: "/tmp/keepsake-no-such-disc.iso".into(),
        keydb: None,
        title: Some(0),
        output: Some("/tmp/keepsake-no-such-disc.mkv".into()),
        mode: RipMode::Remux {
            container: Container::Mkv,
        },
        chapters: false,
        poster: false,
        movie: None,
        audio: AudioSel::All,
        subs: SubSel::All,
        covers: false,
        tv: false,
        series: None,
        episodes: None,
        start_episode: None,
        cinavia: false,
        cinavia_codec: None,
    };
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let err = keepsake_core::rip::run(&req, &cancel, |_| {}).unwrap_err();

    let text = String::from_utf8_lossy(&sink.0.lock().unwrap().clone()).to_string();
    assert!(
        text.contains("opening the disc failed"),
        "the failure site did not log: {text}"
    );
    assert!(
        text.contains("keepsake-no-such-disc.iso"),
        "the path is missing from the log: {text}"
    );
    assert!(!err.to_string().is_empty());
}
