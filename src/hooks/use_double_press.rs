//! Maps to: CC hooks/useDoublePress.ts

use iocraft::prelude::*;
use std::time::{Duration, Instant};

pub const DOUBLE_PRESS_TIMEOUT_MS: u128 = 800;

/// State returned by use_double_press hook.
/// All fields are Copy (State<T> is Copy) — pass by value freely.
#[derive(Clone, Copy)]
pub struct DoublePressState {
    pub pending: State<bool>,
    pub last_press: State<Option<Instant>>,
    pub triggered: State<bool>,
    timeout_tx: Ref<async_channel::Sender<()>>,
}

impl DoublePressState {
    pub fn is_pending(self) -> bool {
        self.pending.get()
    }

    pub fn take_triggered(mut self) -> bool {
        if self.triggered.get() {
            self.triggered.set(false);
            true
        } else {
            false
        }
    }

    pub fn press(mut self) {
        let now = Instant::now();
        if is_within_double_press_window(self.last_press.get(), now) {
            self.pending.set(false);
            self.last_press.set(None);
            self.triggered.set(true);
            return;
        }
        self.last_press.set(Some(now));
        self.pending.set(true);
        let timeout_tx = self.timeout_tx.read().clone();
        let _ = timeout_tx.try_send(());
    }

    pub fn clear(mut self) {
        self.pending.set(false);
        self.last_press.set(None);
    }
}

fn is_within_double_press_window(prev: Option<Instant>, now: Instant) -> bool {
    // The occupied slot represents CC's still-active timeoutRef. A timeout
    // or a successful pair clears it; elapsed time alone is not sufficient.
    prev.is_some_and(|prev| now.duration_since(prev).as_millis() <= DOUBLE_PRESS_TIMEOUT_MS)
}

pub fn use_double_press(hooks: &mut Hooks) -> DoublePressState {
    let timeout_channel = hooks.use_const(|| std::sync::Arc::new(async_channel::unbounded::<()>()));
    let state = DoublePressState {
        pending: hooks.use_state(|| false),
        last_press: hooks.use_state(|| None),
        triggered: hooks.use_state(|| false),
        timeout_tx: hooks.use_ref(|| timeout_channel.0.clone()),
    };

    // CC `useDoublePress.ts` clears the pending UI with a setTimeout after
    // DOUBLE_PRESS_TIMEOUT_MS. Keep this event-driven: an always-on polling
    // task writes synchronized-update escape sequences in iocraft's inline
    // renderer, which pins terminal scrollback to the bottom.
    hooks.use_future({
        let mut pending = state.pending;
        let mut last_press = state.last_press;
        let timeout_rx = timeout_channel.1.clone();
        async move {
            while timeout_rx.recv().await.is_ok() {
                while let Some(prev) = last_press.get() {
                    let elapsed = prev.elapsed().as_millis();
                    if elapsed >= DOUBLE_PRESS_TIMEOUT_MS {
                        pending.set(false);
                        last_press.set(None);
                        break;
                    }
                    futures_timer::Delay::new(Duration::from_millis(
                        (DOUBLE_PRESS_TIMEOUT_MS - elapsed) as u64,
                    ))
                    .await;
                }
            }
        }
    });

    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_press_window_matches_official_800ms_cutoff() {
        let start = Instant::now();

        assert!(is_within_double_press_window(
            Some(start),
            start + Duration::from_millis(DOUBLE_PRESS_TIMEOUT_MS as u64 - 1),
        ));
        assert!(is_within_double_press_window(
            Some(start),
            start + Duration::from_millis(DOUBLE_PRESS_TIMEOUT_MS as u64),
        ));
        assert!(!is_within_double_press_window(
            Some(start),
            start + Duration::from_millis(DOUBLE_PRESS_TIMEOUT_MS as u64 + 1),
        ));
        assert!(!is_within_double_press_window(None, start));
    }
}
