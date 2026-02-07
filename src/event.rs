/// Async event system — decouples terminal input from the render/update loop.
///
/// Spawns background tasks for crossterm event polling and a periodic tick,
/// then exposes a unified `AppEvent` stream consumed by the main loop.
use std::time::Duration;

use color_eyre::eyre::{Result, eyre};
use crossterm::event::{Event, EventStream};
use futures::StreamExt;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::api::models::DownloadProgress;

/// Unified event type consumed by the main application loop.
#[derive(Debug)]
pub enum AppEvent {
    /// A key or mouse event from the terminal.
    Key(crossterm::event::KeyEvent),
    /// Periodic tick for UI animations (spinners, etc.).
    Tick,
    /// Progress update from a background download task.
    Download(DownloadProgress),
    /// Terminal was resized.
    #[allow(dead_code)]
    Resize(u16, u16),
}

/// Manages event sources and exposes a single receiver.
pub struct EventHandler {
    rx: UnboundedReceiver<AppEvent>,
    tx: UnboundedSender<AppEvent>,
}

impl EventHandler {
    /// Spawn the event handler tasks.
    ///
    /// `tick_rate_ms` controls how often `AppEvent::Tick` is sent.
    pub fn new(tick_rate_ms: u64) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        // Input task — reads crossterm events
        let tx_input = tx.clone();
        tokio::spawn(async move {
            let mut reader = EventStream::new();
            loop {
                let Some(event_result) = reader.next().await else {
                    break;
                };
                let Ok(event) = event_result else {
                    continue;
                };
                let msg = match event {
                    Event::Key(key) => AppEvent::Key(key),
                    Event::Resize(w, h) => AppEvent::Resize(w, h),
                    _ => continue,
                };
                if tx_input.send(msg).is_err() {
                    break;
                }
            }
        });

        // Tick task
        let tx_tick = tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(tick_rate_ms));
            loop {
                interval.tick().await;
                if tx_tick.send(AppEvent::Tick).is_err() {
                    break;
                }
            }
        });

        Self { rx, tx }
    }

    /// Get a clone of the sender — used by download tasks to send progress events.
    pub fn sender(&self) -> UnboundedSender<AppEvent> {
        self.tx.clone()
    }

    /// Wait for the next event.
    pub async fn next(&mut self) -> Result<AppEvent> {
        self.rx
            .recv()
            .await
            .ok_or_else(|| eyre!("Event channel closed"))
    }
}
