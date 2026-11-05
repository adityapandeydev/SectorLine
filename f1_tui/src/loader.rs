//! One worker, one replaceable pending request, and two replaceable output slots.
pub mod session;
use crate::data::limits::Cancellation;
use std::{
    sync::{Arc, Condvar, Mutex},
    thread,
    time::Duration,
};

pub enum LoaderEvent<T> {
    Progress { id: u64, message: String },
    Finished { id: u64, result: Result<T, String> },
}
struct Job<R> {
    id: u64,
    request: R,
    cancel: Cancellation,
}
struct State<R, T> {
    next_id: u64,
    current: Option<u64>,
    pending: Option<Job<R>>,
    active: Option<Cancellation>,
    progress: Option<LoaderEvent<T>>,
    finished: Option<LoaderEvent<T>>,
    stopped: bool,
}
pub struct Loader<R, T> {
    shared: Arc<(Mutex<State<R, T>>, Condvar)>,
    timeout: Duration,
}
impl<R: Send + 'static, T: Send + 'static> Loader<R, T> {
    pub fn new(
        timeout: Duration,
        mut work: impl FnMut(R, Cancellation, &dyn Fn(String)) -> Result<T, String> + Send + 'static,
    ) -> Self {
        let shared = Arc::new((
            Mutex::new(State {
                next_id: 0,
                current: None,
                pending: None,
                active: None,
                progress: None,
                finished: None,
                stopped: false,
            }),
            Condvar::new(),
        ));
        let worker = shared.clone();
        thread::spawn(move || {
            loop {
                let job = {
                    let (lock, ready) = &*worker;
                    let mut state = lock.lock().expect("loader state");
                    while state.pending.is_none() && !state.stopped {
                        state = ready.wait(state).expect("loader state");
                    }
                    if state.stopped {
                        return;
                    }
                    let job = state.pending.take().expect("pending job");
                    state.active = Some(job.cancel.clone());
                    job
                };
                let id = job.id;
                let progress = |message: String| {
                    let mut state = worker.0.lock().expect("loader state");
                    if state.current == Some(id) && !state.stopped {
                        state.progress = Some(LoaderEvent::Progress {
                            id,
                            message: display_message(&message),
                        });
                    }
                };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                    || -> Result<T, String> {
                        job.cancel.check()?;
                        let value = work(job.request, job.cancel.clone(), &progress)?;
                        job.cancel.check()?;
                        Ok(value)
                    },
                ))
                .unwrap_or_else(|_| {
                    Err("Session worker failed; retry or choose another session".into())
                });
                let mut state = worker.0.lock().expect("loader state");
                state.active = None;
                if state.current == Some(id) && !state.stopped {
                    state.progress = None;
                    state.finished = Some(LoaderEvent::Finished {
                        id,
                        result: result.map_err(|e| display_message(&e)),
                    });
                }
            }
        });
        Self { shared, timeout }
    }
    pub fn start(&self, request: R) -> u64 {
        let mut state = self.shared.0.lock().expect("loader state");
        if let Some(active) = &state.active {
            active.cancel();
        }
        state.pending.take();
        state.progress = None;
        state.finished = None;
        state.next_id = state.next_id.checked_add(1).expect("request ID exhausted");
        let id = state.next_id;
        state.current = Some(id);
        state.pending = Some(Job {
            id,
            request,
            cancel: Cancellation::with_timeout(self.timeout),
        });
        self.shared.1.notify_one();
        id
    }
    pub fn cancel(&self) {
        let mut state = self.shared.0.lock().expect("loader state");
        state.current = None;
        state.pending = None;
        state.progress = None;
        state.finished = None;
        if let Some(active) = &state.active {
            active.cancel();
        }
    }
    pub fn poll(&self) -> Option<LoaderEvent<T>> {
        let mut state = self.shared.0.lock().expect("loader state");
        state.finished.take().or_else(|| state.progress.take())
    }
}
impl<R, T> Drop for Loader<R, T> {
    fn drop(&mut self) {
        let mut state = self.shared.0.lock().expect("loader state");
        state.stopped = true;
        state.pending = None;
        if let Some(active) = &state.active {
            active.cancel();
        }
        self.shared.1.notify_one();
        // No join on the UI thread. An in-flight blocking call returns at its I/O
        // timeout, and the sole worker exits without publishing a late result.
    }
}
pub fn display_message(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(400).collect()
}
