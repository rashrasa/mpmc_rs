use std::{
    collections::HashMap,
    fs::{File, create_dir_all},
    io::Write,
    path::Path,
    time::Instant,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_repr::{Deserialize_repr, Serialize_repr};

#[derive(Debug)]
pub struct MainBenchRunner {
    inner: BenchRunner,
}

impl MainBenchRunner {
    pub fn new() -> Self {
        Self {
            inner: BenchRunner {
                start: Instant::now(),
                log: BenchEventLog {
                    runner_id: String::from("main_runner"),
                    log: vec![BenchEvent {
                        event: BenchEventData::RunnerStarted,
                        runner_elapsed_secs: 0.0,
                        additional: HashMap::new(),
                    }],
                },
            },
        }
    }

    pub fn spawn_runner(&self, id: String) -> BenchRunner {
        self.inner.spawn_runner(id)
    }
}

#[derive(Debug)]
pub struct BenchRunner {
    start: Instant,
    log: BenchEventLog,
}

impl BenchRunner {
    pub fn spawn_runner(&self, id: String) -> Self {
        let id = format!("{}::{}", self.log.runner_id, id);
        Self {
            log: BenchEventLog {
                log: vec![BenchEvent {
                    runner_elapsed_secs: 0.0,
                    event: BenchEventData::RunnerStarted,
                    additional: HashMap::new(),
                }],
                runner_id: id,
            },
            start: Instant::now(),
        }
    }
    pub fn record(&mut self, event: BenchEventData, additional: HashMap<String, Value>) {
        self.log.log.push({
            BenchEvent {
                runner_elapsed_secs: self.start.elapsed().as_secs_f64(),
                event,
                additional,
            }
        })
    }
}

impl Drop for BenchRunner {
    fn drop(&mut self) {
        self.record(BenchEventData::RunnerClosed, HashMap::new());
        let mut dst = Path::new("results").to_path_buf();
        let splits = self.log.runner_id.split("::");
        let mut last = "";
        for split in splits {
            dst = dst.join(split);
            last = split;
        }
        let last = last.to_owned() + ".json";
        if let Ok(_) = create_dir_all(&dst) {
            if let Ok(mut file) = File::create(&dst.join(last)) {
                if let Ok(bytes) = serde_json::to_vec(&self.log.log) {
                    let _ = file.write_all(&bytes);
                }
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct BenchEventLog {
    runner_id: String,
    log: Vec<BenchEvent>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BenchEvent {
    #[serde(rename = "_t")]
    runner_elapsed_secs: f64,

    #[serde(rename = "_e")]
    event: BenchEventData,

    #[serde(flatten)]
    additional: HashMap<String, Value>,
}

#[derive(Debug, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum BenchEventData {
    RunnerStarted,
    RunnerClosed,
    ValueSent,
    ValueReceived,
}
