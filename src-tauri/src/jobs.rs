use std::collections::{HashSet, VecDeque};
use std::future::Future;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

#[derive(Clone)]
enum Sink {
    App(AppHandle),
    #[cfg(test)]
    Console,
    Terminal,
}

#[derive(Clone)]
pub struct Log {
    sink: Sink,
    job: String,
}

#[derive(Serialize, Clone)]
struct LogLine<'a> {
    job: &'a str,
    stream: &'a str,
    text: &'a str,
}

#[derive(Serialize, Clone)]
struct JobItem<'a> {
    job: &'a str,
    id: u64,
    state: &'a str,
}

#[derive(Serialize, Clone)]
pub struct JobState {
    pub job: String,
    pub state: &'static str,
    pub message: Option<String>,
}

impl Log {
    pub fn new(app: &AppHandle, job: &str) -> Self {
        Self { sink: Sink::App(app.clone()), job: job.to_string() }
    }

    #[cfg(test)]
    pub fn console(job: &str) -> Self {
        Self { sink: Sink::Console, job: job.to_string() }
    }

    pub fn terminal() -> Self {
        Self { sink: Sink::Terminal, job: String::new() }
    }

    pub fn info(&self, text: impl AsRef<str>) {
        self.stream("info", text.as_ref());
    }

    pub fn stream(&self, stream: &str, text: &str) {
        match &self.sink {
            Sink::App(app) => {
                let _ = app.emit("job-log", LogLine { job: &self.job, stream, text });
            }
            #[cfg(test)]
            Sink::Console => println!("[{}/{stream}] {text}", self.job),
            Sink::Terminal if stream == "err" => eprintln!("{text}"),
            Sink::Terminal => println!("{text}"),
        }
    }

    // Reports one queued item's progress, so the UI knows when it can no longer be cancelled.
    fn item(&self, id: u64, state: &str) {
        if let Sink::App(app) = &self.sink {
            let _ = app.emit("job-item", JobItem { job: &self.job, id, state });
        }
    }

    pub fn state(&self, state: &'static str, message: Option<String>) {
        if let Sink::App(app) = &self.sink {
            let _ = app.emit("job-state", JobState { job: self.job.clone(), state, message });
        }
    }
}

#[derive(Default, Clone)]
pub struct Jobs {
    running: Arc<Mutex<HashSet<String>>>,
}

pub struct JobQueue<T> {
    items: VecDeque<(u64, T)>,
    draining: bool,
    next_id: u64,
}

impl<T> Default for JobQueue<T> {
    fn default() -> Self {
        Self { items: VecDeque::new(), draining: false, next_id: 0 }
    }
}

impl<T> JobQueue<T> {
    // Only items that haven't started yet can be cancelled.
    pub fn cancel(&mut self, id: u64) -> bool {
        let Some(index) = self.items.iter().position(|(queued, _)| *queued == id) else { return false };
        self.items.remove(index);
        true
    }
}

impl Jobs {
    pub fn running(&self) -> Vec<String> {
        self.running.lock().unwrap().iter().cloned().collect()
    }

    pub fn start<F, Fut>(&self, app: &AppHandle, job: &str, work: F) -> Result<(), String>
    where
        F: FnOnce(Log) -> Fut + Send + 'static,
        Fut: Future<Output = Result<String>> + Send + 'static,
    {
        if !self.running.lock().unwrap().insert(job.to_string()) {
            return Err(format!("{job} is already running"));
        }
        let log = Log::new(app, job);
        let running = self.running.clone();
        let name = job.to_string();
        tauri::async_runtime::spawn(async move {
            log.state("running", None);
            let result = work(log.clone()).await;
            running.lock().unwrap().remove(&name);
            match result {
                Ok(message) => log.state("done", Some(message)),
                Err(error) => {
                    log.stream("err", &format!("{error:#}"));
                    log.state("failed", Some(format!("{error:#}")));
                }
            }
        });
        Ok(())
    }

    // Work you add while a queued job runs joins that run. We end the run while holding the queue's
    // lock, after its final state goes out, so an item added at that moment starts a fresh run
    // instead of waiting in a queue nothing drains. Pass the returned id to JobQueue::cancel.
    pub fn enqueue<T, F, Fut>(&self, app: &AppHandle, job: &str, queue: &Arc<Mutex<JobQueue<T>>>, item: T, work: F) -> Result<u64, String>
    where
        T: Send + 'static,
        F: Fn(T, Log) -> Fut + Send + 'static,
        Fut: Future<Output = Result<String>> + Send + 'static,
    {
        let mut pending = queue.lock().unwrap();
        let id = pending.next_id;
        if pending.draining {
            pending.next_id += 1;
            pending.items.push_back((id, item));
            return Ok(id);
        }
        if !self.running.lock().unwrap().insert(job.to_string()) {
            return Err(format!("{job} is already running"));
        }
        pending.next_id += 1;
        pending.items.push_back((id, item));
        pending.draining = true;
        drop(pending);
        let log = Log::new(app, job);
        let running = self.running.clone();
        let name = job.to_string();
        let queue = queue.clone();
        tauri::async_runtime::spawn(async move {
            log.state("running", None);
            let mut messages = Vec::new();
            let mut failures = Vec::new();
            loop {
                let (id, item) = {
                    let mut pending = queue.lock().unwrap();
                    match pending.items.pop_front() {
                        Some((id, item)) => {
                            log.item(id, "started");
                            (id, item)
                        }
                        None => {
                            pending.draining = false;
                            if failures.is_empty() {
                                log.state("done", Some(messages.join(" ")));
                            } else {
                                // The changes that worked are still on disk, so we report them too.
                                log.state("failed", Some(failures.into_iter().chain(messages).collect::<Vec<_>>().join(" ")));
                            }
                            running.lock().unwrap().remove(&name);
                            break;
                        }
                    }
                };
                match work(item, log.clone()).await {
                    Ok(message) => {
                        log.item(id, "finished");
                        if !message.is_empty() {
                            log.info(&message);
                            messages.push(message);
                        }
                    }
                    Err(error) => {
                        log.item(id, "failed");
                        let error = format!("{error:#}");
                        log.stream("err", &error);
                        failures.push(error);
                    }
                }
            }
        });
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelling_a_queued_item_drops_only_that_item() {
        let mut queue = JobQueue::default();
        queue.items.extend([(0, "a"), (1, "b")]);
        assert!(queue.cancel(0));
        assert!(!queue.cancel(5));
        assert_eq!(queue.items.iter().map(|(_, item)| *item).collect::<Vec<_>>(), ["b"]);
    }
}
