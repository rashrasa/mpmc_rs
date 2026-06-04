// TODO: potentially instantiate logs with an extremely high capacity

use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

#[derive(Debug, Default)]
pub struct MainBenchRunner {
    // this shouldnt slow down any benchmarks since this is only accessed when a
    // test runner completes.
    inner: Arc<Mutex<MainBenchRunnerInner>>,
}

impl MainBenchRunner {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn spawn_runner<'run>(&'run self, id: String) -> BenchRunner<'run> {
        BenchRunner {
            main: self,
            log: BenchEventLog {
                log: vec![BenchEvent {
                    instant: Instant::now(),
                    event: BenchEventData::RunnerStarted,
                }],
                runner_id: id,
            },
        }
    }

    pub fn complete(&self, log: BenchEventLog) {
        self.inner.lock().unwrap().results.push(log);
    }

    pub fn write_results_to_file(&self, path: &str) {
        todo!()
    }
}

#[derive(Debug, Default)]
struct MainBenchRunnerInner {
    results: Vec<BenchEventLog>,
}

#[derive(Debug)]
pub struct BenchRunner<'run> {
    main: &'run MainBenchRunner,
    log: BenchEventLog,
}

impl<'run> BenchRunner<'run> {
    pub fn spawn_runner(&self, id: String) -> Self {
        let id = format!("{}::{}", self.log.runner_id, id);
        Self {
            main: self.main,
            log: BenchEventLog {
                log: vec![BenchEvent {
                    instant: Instant::now(),
                    event: BenchEventData::RunnerStarted,
                }],
                runner_id: id,
            },
        }
    }
    pub fn record(&mut self, event: BenchEventData) {
        self.log.log.push({
            BenchEvent {
                instant: Instant::now(),
                event,
            }
        })
    }
}

impl<'run> Drop for BenchRunner<'run> {
    fn drop(&mut self) {
        self.record(BenchEventData::RunnerClosed);
        self.main.complete(std::mem::replace(
            &mut self.log,
            BenchEventLog {
                runner_id: String::new(),
                log: vec![],
            },
        ));
    }
}

#[derive(Debug, Default)]
pub struct BenchEventLog {
    runner_id: String,
    log: Vec<BenchEvent>,
}
#[derive(Debug)]
pub struct BenchEvent {
    instant: Instant,
    event: BenchEventData,
}

#[derive(Debug)]
pub enum BenchEventData {
    RunnerStarted,
    RunnerClosed,
}
