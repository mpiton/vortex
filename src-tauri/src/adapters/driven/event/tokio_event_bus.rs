use tokio::sync::broadcast;

use crate::domain::event::DomainEvent;
use crate::domain::ports::driven::EventBus;

pub struct TokioEventBus {
    sender: broadcast::Sender<DomainEvent>,
}

impl TokioEventBus {
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }
}

impl EventBus for TokioEventBus {
    fn publish(&self, event: DomainEvent) {
        let _ = self.sender.send(event);
    }

    fn subscribe(&self, handler: Box<dyn Fn(&DomainEvent) + Send + Sync + 'static>) {
        let mut rx = self.sender.subscribe();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => handler(&event),
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "event bus subscriber lagged, events dropped");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::download::DownloadId;
    use std::sync::{Arc, Mutex};
    use tokio::sync::Notify;

    #[tokio::test]
    async fn test_publish_and_receive_event() {
        let bus = TokioEventBus::new(16);
        let received: Arc<Mutex<Vec<DomainEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let notify = Arc::new(Notify::new());
        let received_clone = received.clone();
        let notify_clone = notify.clone();

        bus.subscribe(Box::new(move |event: &DomainEvent| {
            received_clone.lock().unwrap().push(event.clone());
            notify_clone.notify_one();
        }));

        bus.publish(DomainEvent::DownloadStarted { id: DownloadId(1) });
        notify.notified().await;

        let events = received.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            DomainEvent::DownloadStarted { id: DownloadId(1) }
        );
    }

    #[tokio::test]
    async fn test_multiple_subscribers_receive_same_event() {
        let bus = TokioEventBus::new(16);
        let received1: Arc<Mutex<Vec<DomainEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let received2: Arc<Mutex<Vec<DomainEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let notify1 = Arc::new(Notify::new());
        let notify2 = Arc::new(Notify::new());
        let clone1 = received1.clone();
        let clone2 = received2.clone();
        let n1 = notify1.clone();
        let n2 = notify2.clone();

        bus.subscribe(Box::new(move |event: &DomainEvent| {
            clone1.lock().unwrap().push(event.clone());
            n1.notify_one();
        }));
        bus.subscribe(Box::new(move |event: &DomainEvent| {
            clone2.lock().unwrap().push(event.clone());
            n2.notify_one();
        }));

        bus.publish(DomainEvent::DownloadCompleted { id: DownloadId(42) });
        notify1.notified().await;
        notify2.notified().await;

        assert_eq!(received1.lock().unwrap().len(), 1);
        assert_eq!(received2.lock().unwrap().len(), 1);
        assert_eq!(
            received1.lock().unwrap()[0],
            DomainEvent::DownloadCompleted { id: DownloadId(42) }
        );
        assert_eq!(
            received2.lock().unwrap()[0],
            DomainEvent::DownloadCompleted { id: DownloadId(42) }
        );
    }

    #[test]
    fn test_publish_no_subscriber_doesnt_block() {
        let bus = TokioEventBus::new(16);
        bus.publish(DomainEvent::DownloadStarted { id: DownloadId(99) });
        bus.publish(DomainEvent::DownloadCompleted { id: DownloadId(99) });
    }

    #[test]
    fn test_new_with_zero_capacity_uses_minimum() {
        let bus = TokioEventBus::new(0);
        bus.publish(DomainEvent::DownloadStarted { id: DownloadId(1) });
    }
}
