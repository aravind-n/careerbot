//! Structured logging setup. We mount two layers on a single
//! tracing-subscriber registry: a JSON formatter that writes to stderr
//! (captured by the supervisor) and an in-memory ring buffer + broadcast
//! channel that backs `careerbot logs [--follow]`.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;

const RING_CAPACITY: usize = 1000;
const BROADCAST_CAPACITY: usize = 256;

/// Handle to the in-memory log capture installed by [`init_tracing`].
/// The daemon hands this to its HTTP routes so `careerbot logs` can
/// fetch a snapshot and `careerbot logs --follow` can stream.
#[derive(Clone)]
pub struct LogBuffer {
    buffer: Arc<Mutex<VecDeque<String>>>,
    tx: broadcast::Sender<String>,
}

impl LogBuffer {
    /// Empty placeholder. Used by tests that exercise the daemon
    /// without installing a global subscriber.
    pub fn empty() -> Self {
        let (tx, _) = broadcast::channel(1);
        Self {
            buffer: Arc::new(Mutex::new(VecDeque::new())),
            tx,
        }
    }

    /// Snapshot of the last lines captured. Returns oldest-first.
    pub fn snapshot(&self) -> Vec<String> {
        self.buffer
            .lock()
            .map(|b| b.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Subscribe to new lines as they arrive.
    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.tx.subscribe()
    }
}

/// Initialise the global tracing subscriber and return the handle to
/// the in-memory capture. Must be called exactly once per process,
/// before any tracing calls fire.
pub fn init_tracing() -> LogBuffer {
    let buffer: Arc<Mutex<VecDeque<String>>> =
        Arc::new(Mutex::new(VecDeque::with_capacity(RING_CAPACITY)));
    let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);

    let buf_layer = BufferLayer {
        buffer: buffer.clone(),
        tx: tx.clone(),
    };

    let fmt_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_writer(std::io::stderr)
        .with_current_span(true)
        .with_span_list(true)
        .with_thread_ids(true)
        .with_thread_names(true);

    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(fmt_layer)
        .with(buf_layer)
        .init();

    LogBuffer { buffer, tx }
}

struct BufferLayer {
    buffer: Arc<Mutex<VecDeque<String>>>,
    tx: broadcast::Sender<String>,
}

impl<S> Layer<S> for BufferLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let mut fields = FieldString::default();
        event.record(&mut fields);

        let line = format!(
            "{} {} {} {}",
            chrono::Utc::now().to_rfc3339(),
            metadata.level(),
            metadata.target(),
            fields.0,
        );

        if let Ok(mut buf) = self.buffer.lock() {
            if buf.len() >= RING_CAPACITY {
                buf.pop_front();
            }
            buf.push_back(line.clone());
        }
        // Best-effort fan-out; ignored when there are no subscribers.
        let _ = self.tx.send(line);
    }
}

#[derive(Default)]
struct FieldString(String);

impl Visit for FieldString {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if !self.0.is_empty() {
            self.0.push(' ');
        }
        let _ = write!(self.0, "{}={:?}", field.name(), value);
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if !self.0.is_empty() {
            self.0.push(' ');
        }
        let _ = write!(self.0, "{}={}", field.name(), value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn push_directly(buf: &LogBuffer, n: usize) {
        // Bypass the tracing wiring; the LogBuffer is just a ring + a
        // broadcaster. Pushing through `tracing::info!()` would require
        // installing the global subscriber, which we don't want inside
        // unit tests.
        if let Ok(mut guard) = buf.buffer.lock() {
            for i in 0..n {
                if guard.len() >= RING_CAPACITY {
                    guard.pop_front();
                }
                guard.push_back(format!("line {}", i));
            }
        }
    }

    #[test]
    fn empty_buffer_returns_no_lines() {
        let buf = LogBuffer::empty();
        assert!(buf.snapshot().is_empty());
    }

    #[test]
    fn snapshot_returns_lines_oldest_first() {
        let buf = LogBuffer::empty();
        push_directly(&buf, 5);
        let snap = buf.snapshot();
        assert_eq!(snap, vec!["line 0", "line 1", "line 2", "line 3", "line 4"]);
    }

    #[test]
    fn ring_buffer_drops_oldest_when_full() {
        let buf = LogBuffer::empty();
        push_directly(&buf, RING_CAPACITY + 10);
        let snap = buf.snapshot();
        assert_eq!(snap.len(), RING_CAPACITY);
        assert_eq!(snap[0], format!("line {}", 10));
        assert_eq!(snap[RING_CAPACITY - 1], format!("line {}", RING_CAPACITY + 9));
    }
}
