//! Tells the devices listening to a library that its log grew.
//!
//! In one process. Running several instances needs the news passed between
//! them too (PostgreSQL LISTEN/NOTIFY); until then devices also poll.

use std::collections::HashMap;
use std::sync::Mutex;

use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Default)]
pub struct Events {
    channels: Mutex<HashMap<Uuid, broadcast::Sender<u64>>>,
}

impl Events {
    pub fn subscribe(&self, library: Uuid) -> broadcast::Receiver<u64> {
        let mut channels = self.channels.lock().unwrap_or_else(|e| e.into_inner());
        channels.entry(library).or_insert_with(|| broadcast::channel(16).0).subscribe()
    }

    /// The library's log now ends at `head`.
    pub fn publish(&self, library: Uuid, head: u64) {
        let mut channels = self.channels.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(sender) = channels.get(&library)
            && sender.send(head).is_err()
        {
            // Nobody listens any more.
            channels.remove(&library);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn listeners_hear_the_new_head() {
        let events = Events::default();
        let library = Uuid::now_v7();
        events.publish(library, 1);
        let mut listener = events.subscribe(library);
        events.publish(library, 2);
        assert_eq!(listener.recv().await.unwrap(), 2);
        drop(listener);
        events.publish(library, 3);
        assert!(events.channels.lock().unwrap().is_empty(), "the channel goes with its last listener");
    }
}
