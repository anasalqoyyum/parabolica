//! Applies presentations to the OS media controls on a worker thread, so OS calls never block the
//! UI. Artwork loads on a second thread, so status, button and privacy changes never wait on file
//! reads.

use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;

use super::presentation::{Artwork, Presentation};

/// The OS side: SMTC on Windows, a recorder in tests.
pub trait MediaSink: Send + 'static {
    /// `artwork` always belongs to `presentation`'s entry.
    fn show(&mut self, presentation: &Presentation, artwork: Option<&[u8]>);
}

pub type ArtworkLoader = Box<dyn FnMut(&Artwork) -> Option<Arc<Vec<u8>>> + Send>;

enum Message {
    Present(Presentation),
    Loaded(Artwork, Option<Arc<Vec<u8>>>),
    Quit,
}

#[derive(Debug)]
pub struct Presenter {
    sender: mpsc::Sender<Message>,
    thread: Option<JoinHandle<()>>,
}

impl Presenter {
    pub fn start(sink: impl MediaSink, load_artwork: ArtworkLoader) -> Self {
        let (sender, receiver) = mpsc::channel();
        let (requests, pending) = mpsc::channel();
        let loaded = sender.clone();
        std::thread::Builder::new()
            .name("parabolica-media-artwork".into())
            .spawn(move || load(&pending, &loaded, load_artwork))
            .expect("the media artwork thread starts");
        let thread = std::thread::Builder::new()
            .name("parabolica-media-controls".into())
            .spawn(move || run(&receiver, &requests, sink))
            .expect("the media controls thread starts");
        Self {
            sender,
            thread: Some(thread),
        }
    }

    pub fn present(&self, presentation: Presentation) {
        let _ = self.sender.send(Message::Present(presentation));
    }
}

impl Drop for Presenter {
    /// Clears the OS controls before returning, so nothing remains after the app quits. A load
    /// still reading a file finishes on its own thread and is discarded.
    fn drop(&mut self) {
        let _ = self.sender.send(Message::Present(Presentation::CLOSED));
        let _ = self.sender.send(Message::Quit);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(
    receiver: &mpsc::Receiver<Message>,
    requests: &mpsc::Sender<Artwork>,
    mut sink: impl MediaSink,
) {
    let mut shown: Option<Presentation> = None;
    // Artwork bytes for `shown`'s entry, once loaded.
    let mut artwork: (Artwork, Option<Arc<Vec<u8>>>) = (Artwork::None, None);
    while let Ok(message) = receiver.recv() {
        match message {
            Message::Present(next) => {
                if shown.as_ref() == Some(&next) {
                    continue;
                }
                if *next.artwork() != artwork.0 {
                    artwork = (next.artwork().clone(), None);
                    if *next.artwork() != Artwork::None {
                        let _ = requests.send(next.artwork().clone());
                    }
                }
                sink.show(&next, artwork.1.as_deref().map(Vec::as_slice));
                shown = Some(next);
            }
            Message::Loaded(key, bytes) => {
                // Artwork for an entry that is no longer shown is dropped.
                if key != artwork.0 || bytes.is_none() {
                    continue;
                }
                artwork.1 = bytes;
                if let Some(shown) = &shown {
                    sink.show(shown, artwork.1.as_deref().map(Vec::as_slice));
                }
            }
            Message::Quit => return,
        }
    }
}

fn load(
    pending: &mpsc::Receiver<Artwork>,
    loaded: &mpsc::Sender<Message>,
    mut load_artwork: ArtworkLoader,
) {
    while let Ok(mut request) = pending.recv() {
        // Only the newest request matters.
        while let Ok(newer) = pending.try_recv() {
            request = newer;
        }
        let bytes = load_artwork(&request);
        if loaded.send(Message::Loaded(request, bytes)).is_err() {
            return;
        }
    }
}
