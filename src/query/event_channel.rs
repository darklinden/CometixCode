//! Query async-generator transport (PORTING.md A2).
//! A pull consumer must resume the producer only after inspecting the last yield.
//! Ordinary REPL consumers retain their existing unbounded event transport.

use super::QueryEvent;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct QueryEventSender {
    sender: async_channel::Sender<QueryEvent>,
    resume: Option<Arc<QueryPullControl>>,
}

impl From<async_channel::Sender<QueryEvent>> for QueryEventSender {
    fn from(sender: async_channel::Sender<QueryEvent>) -> Self {
        Self {
            sender,
            resume: None,
        }
    }
}

impl QueryEventSender {
    pub(super) fn pull(
        sender: async_channel::Sender<QueryEvent>,
        resume: Arc<QueryPullControl>,
    ) -> Self {
        Self {
            sender,
            resume: Some(resume),
        }
    }

    pub async fn send(&self, event: QueryEvent) -> Result<(), async_channel::SendError<()>> {
        let Some(resume) = &self.resume else {
            return self
                .sender
                .send(event)
                .await
                .map_err(|_| async_channel::SendError(()));
        };
        let _turn = resume.send_lock.lock().await;
        {
            // Queue order and acknowledgement order must match even when a
            // synchronous UI callback publishes between generator yields.
            let mut pending = resume.pending.lock().unwrap();
            pending.push_back(true);
            if self.sender.try_send(event).is_err() {
                pending.pop_back();
                return Err(async_channel::SendError(()));
            }
        }
        match resume.permits.acquire().await {
            Ok(permit) => permit.forget(),
            Err(_) => return Err(async_channel::SendError(())),
        }
        Ok(())
    }

    pub(super) fn is_pull(&self) -> bool {
        self.resume.is_some()
    }

    // `TrySendError<QueryEvent>` is the channel's own error type.
    #[allow(clippy::result_large_err)]
    pub fn try_send(
        &self,
        event: QueryEvent,
    ) -> Result<(), async_channel::TrySendError<QueryEvent>> {
        let Some(resume) = &self.resume else {
            return self.sender.try_send(event);
        };
        let mut pending = resume.pending.lock().unwrap();
        pending.push_back(false);
        let result = self.sender.try_send(event);
        if result.is_err() {
            pending.pop_back();
        }
        result
    }
}

/// The resume/return operations of a pull-based async generator.
#[derive(Debug)]
pub struct QueryPullControl {
    permits: tokio::sync::Semaphore,
    pending: std::sync::Mutex<std::collections::VecDeque<bool>>,
    send_lock: tokio::sync::Mutex<()>,
    stopped: std::sync::atomic::AtomicBool,
    stop: tokio::sync::Notify,
}
impl QueryPullControl {
    pub fn new() -> Self {
        Self {
            permits: tokio::sync::Semaphore::new(0),
            pending: Default::default(),
            send_lock: tokio::sync::Mutex::new(()),
            stopped: false.into(),
            stop: tokio::sync::Notify::new(),
        }
    }
    pub fn advance(&self) {
        if self.pending.lock().unwrap().pop_front() == Some(true) {
            self.permits.add_permits(1);
        }
    }
    pub fn close(&self) {
        self.stopped
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.permits.close();
        self.stop.notify_one();
    }
    pub async fn closed(&self) {
        if !self.stopped.load(std::sync::atomic::Ordering::SeqCst) {
            self.stop.notified().await;
        }
    }
}

/// A6: dropping a Rust consumer must release a producer suspended at yield.
pub(crate) struct QueryGeneratorGuard(pub Arc<QueryPullControl>);
impl Drop for QueryGeneratorGuard {
    fn drop(&mut self) {
        self.0.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn pull_resume_matches_official_yield_not_ui_callbacks() {
        // CC execAgentHook.ts:177-209: next() only resumes the inspected yield.
        let control = Arc::new(QueryPullControl::new());
        let (tx, rx) = async_channel::unbounded();
        let sender = QueryEventSender::pull(tx, control.clone());
        sender.try_send(QueryEvent::ClearStreamingPreview).unwrap();
        let producer =
            tokio::spawn(async move { sender.send(QueryEvent::StreamRequestStart).await });
        rx.recv().await.unwrap();
        control.advance(); // A synchronous callback cannot resume the generator.
        rx.recv().await.unwrap();
        tokio::task::yield_now().await;
        assert!(!producer.is_finished());
        control.advance();
        assert!(producer.await.unwrap().is_ok());
    }
    #[tokio::test]
    async fn generator_drop_matches_official_return_releases_yield() {
        let control = Arc::new(QueryPullControl::new());
        let guard = QueryGeneratorGuard(control.clone());
        let (tx, rx) = async_channel::unbounded();
        let sender = QueryEventSender::pull(tx, control.clone());
        let producer =
            tokio::spawn(async move { sender.send(QueryEvent::StreamRequestStart).await });
        rx.recv().await.unwrap();
        drop(guard);
        assert!(producer.await.unwrap().is_err());
        control.closed().await;
    }
}
