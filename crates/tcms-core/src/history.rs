//! FIFO operations and bounded, atomically persisted transaction history.
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
pub type Transcript = Arc<Mutex<String>>;
pub fn append(log: &Transcript, text: &str) {
    let mut log = log.lock().unwrap_or_else(|e| e.into_inner());
    log.push_str(text);
    if log.len() > 65536 {
        let mut cut = log.len() - 65536;
        while !log.is_char_boundary(cut) {
            cut += 1;
        }
        log.drain(..cut);
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}
impl Status {
    pub fn active(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Queued => "history.queued",
            Self::Running => "history.running",
            Self::Succeeded => "history.succeeded",
            Self::Failed => "history.failed",
            Self::Cancelled => "history.cancelled",
            Self::Interrupted => "history.interrupted",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub id: u64,
    pub title: String,
    pub created: u64,
    pub finished: Option<u64>,
    pub status: Status,
    pub output: String,
}
struct State {
    next: u64,
    records: Vec<Record>,
    pending: VecDeque<u64>,
    logs: HashMap<u64, Transcript>,
    warning: Option<String>,
}
pub struct TransactionQueue {
    state: Mutex<State>,
    changed: Condvar,
    path: Option<PathBuf>,
}
pub struct Running {
    queue: Arc<TransactionQueue>,
    pub id: u64,
    pub log: Transcript,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
impl TransactionQueue {
    pub fn application() -> Arc<Self> {
        Self::load(
            dirs::data_local_dir().map(|d| d.join("the-cursed-moon-store/transactions.json")),
        )
    }
    pub fn load(path: Option<PathBuf>) -> Arc<Self> {
        let mut warning = None;
        let mut records: Vec<Record> = match path.as_ref().map(std::fs::read) {
            Some(Ok(b)) => serde_json::from_slice(&b).unwrap_or_else(|e| {
                warning = Some(e.to_string());
                vec![]
            }),
            Some(Err(e)) if e.kind() != std::io::ErrorKind::NotFound => {
                warning = Some(e.to_string());
                vec![]
            }
            _ => vec![],
        };
        for r in &mut records {
            if r.status.active() {
                r.status = Status::Interrupted;
                r.finished = Some(now());
            }
        }
        let next = records.iter().map(|r| r.id).max().unwrap_or(0) + 1;
        Arc::new(Self {
            state: Mutex::new(State {
                next,
                records,
                pending: VecDeque::new(),
                logs: HashMap::new(),
                warning,
            }),
            changed: Condvar::new(),
            path,
        })
    }
    fn persist(&self, s: &mut State) {
        while s.records.len() > 100 {
            let Some(i) = s.records.iter().position(|r| !r.status.active()) else {
                break;
            };
            s.records.remove(i);
        }
        if let Some(path) = &self.path {
            let r = serde_json::to_vec(&s.records)
                .map_err(std::io::Error::other)
                .and_then(|b| crate::atomic_file::write(path, &b));
            if let Err(e) = r {
                s.warning = Some(e.to_string());
            }
        }
    }
    pub fn enqueue(&self, title: String) -> u64 {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let id = s.next;
        s.next += 1;
        s.records.push(Record {
            id,
            title,
            created: now(),
            finished: None,
            status: Status::Queued,
            output: String::new(),
        });
        s.pending.push_back(id);
        self.persist(&mut s);
        self.changed.notify_all();
        id
    }
    pub fn cancel(&self, id: u64) -> bool {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let Some(r) = s
            .records
            .iter_mut()
            .find(|r| r.id == id && r.status == Status::Queued)
        else {
            return false;
        };
        r.status = Status::Cancelled;
        r.finished = Some(now());
        s.pending.retain(|&p| p != id);
        self.persist(&mut s);
        self.changed.notify_all();
        true
    }
    pub fn acquire(self: &Arc<Self>, id: u64) -> Option<Running> {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if !s
                .records
                .iter()
                .any(|r| r.id == id && r.status == Status::Queued)
            {
                return None;
            }
            if s.pending.front() == Some(&id)
                && !s.records.iter().any(|r| r.status == Status::Running)
            {
                break;
            }
            s = self.changed.wait(s).unwrap_or_else(|e| e.into_inner());
        }
        s.records.iter_mut().find(|r| r.id == id).unwrap().status = Status::Running;
        let log = Arc::new(Mutex::new(String::new()));
        s.logs.insert(id, log.clone());
        self.persist(&mut s);
        Some(Running {
            queue: self.clone(),
            id,
            log,
        })
    }
    fn complete(&self, id: u64, status: Status, error: Option<&str>) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let output = s
            .logs
            .remove(&id)
            .map(|l| l.lock().unwrap_or_else(|e| e.into_inner()).clone());
        if let Some(r) = s
            .records
            .iter_mut()
            .find(|r| r.id == id && r.status.active())
        {
            r.status = status;
            r.finished = Some(now());
            if let Some(output) = output {
                r.output = output;
            }
            if let Some(error) = error {
                let log = Arc::new(Mutex::new(std::mem::take(&mut r.output)));
                append(&log, &format!("\n{error}\n"));
                r.output = log.lock().unwrap().clone();
            }
        }
        s.pending.retain(|&p| p != id);
        self.persist(&mut s);
        self.changed.notify_all();
    }
    pub fn finish(&self, id: u64, error: Option<&str>) {
        self.complete(
            id,
            if error.is_some() {
                Status::Failed
            } else {
                Status::Succeeded
            },
            error,
        );
    }
    pub fn active(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .records
            .iter()
            .any(|r| r.status.active())
    }
    pub fn warning(&self) -> Option<String> {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .warning
            .clone()
    }
    pub fn snapshot(&self) -> Vec<Record> {
        let s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        s.records
            .iter()
            .rev()
            .cloned()
            .map(|mut r| {
                if let Some(l) = s.logs.get(&r.id) {
                    r.output = l.lock().unwrap_or_else(|e| e.into_inner()).clone();
                }
                r
            })
            .collect()
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.queue.complete(self.id, Status::Interrupted, None);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interrupted_history_is_not_replayed() {
        let path = std::env::temp_dir().join(format!("tcms-history-{}.json", std::process::id()));
        let q = TransactionQueue::load(Some(path.clone()));
        q.enqueue("unfinished".into());
        drop(q);
        let q = TransactionQueue::load(Some(path.clone()));
        assert!(!q.active());
        assert_eq!(q.snapshot()[0].status, Status::Interrupted);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn queue_cancels_only_waiting_jobs_and_recovers_from_panic() {
        let q = TransactionQueue::load(None);
        let a = q.enqueue("a".into());
        let b = q.enqueue("b".into());
        let run = q.acquire(a).unwrap();
        append(&run.log, "hello");
        assert!(!q.cancel(a));
        assert!(q.cancel(b));
        assert!(q.acquire(b).is_none());
        q.finish(a, None);
        drop(run);
        assert_eq!(q.snapshot()[1].output, "hello");
        let c = q.enqueue("c".into());
        drop(q.acquire(c));
        assert!(!q.active());
        assert_eq!(q.snapshot()[0].status, Status::Interrupted);
    }
}
