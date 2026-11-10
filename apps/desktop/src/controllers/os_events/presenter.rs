//! Applies presentations to the OS media controls on a worker thread, so artwork reads and OS
//! calls never block the UI.

use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;

use super::presentation::{Artwork, Presentation};

/// The OS side: SMTC on Windows, a recorder in tests.
pub trait MediaSink: Send + 'static {
    /// `artwork` always belongs to `presentation`'s entry.
    fn show(&mut self, presentation: &Presentation, artwork: Option<&[u8]>);
}

pub type ArtworkLoader = Box<dyn FnMut(&Artwork) -> Option<Arc<Vec<u8>>> + Send>;

#[derive(Debug)]
pub struct Presenter {
    sender: Option<mpsc::Sender<Presentation>>,
    thread: Option<JoinHandle<()>>,
}

impl Presenter {
    pub fn start(sink: impl MediaSink, load_artwork: ArtworkLoader) -> Self {
        let (sender, receiver) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("parabolica-media-controls".into())
            .spawn(move || run(&receiver, sink, load_artwork))
            .expect("the media controls thread starts");
        Self {
            sender: Some(sender),
            thread: Some(thread),
        }
    }

    pub fn present(&self, presentation: Presentation) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(presentation);
        }
    }
}

impl Drop for Presenter {
    /// Clears the OS controls before returning, so nothing remains after the app quits.
    fn drop(&mut self) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(Presentation::CLOSED);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(
    receiver: &mpsc::Receiver<Presentation>,
    mut sink: impl MediaSink,
    mut load_artwork: ArtworkLoader,
) {
    let mut shown: Option<Presentation> = None;
    let mut artwork: (Artwork, Option<Arc<Vec<u8>>>) = (Artwork::None, None);
    let Ok(mut next) = receiver.recv() else {
        return;
    };
    loop {
        // Only the newest presentation matters.
        while let Ok(newer) = receiver.try_recv() {
            next = newer;
        }
        if shown.as_ref() != Some(&next) {
            if *next.artwork() != artwork.0 {
                artwork = (next.artwork().clone(), load_artwork(next.artwork()));
            }
            // Reading artwork can take a while; a newer entry replaces this one unseen.
            if let Ok(newer) = receiver.try_recv() {
                next = newer;
                continue;
            }
            sink.show(&next, artwork.1.as_deref().map(Vec::as_slice));
            shown = Some(next.clone());
        }
        match receiver.recv() {
            Ok(newer) => next = newer,
            Err(_) => return,
        }
    }
}
