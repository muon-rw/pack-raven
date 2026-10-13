use std::collections::HashSet;
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
}
