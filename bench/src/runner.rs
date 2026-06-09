use std::{
    fs::{File, create_dir_all},
    io::{BufWriter, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::completion::CompletionGuard;
use anyhow::Context;
use fast_time::{Clock, Instant};

use crate::RUNNER_WRITER_BUFFER_SIZE;

#[derive(Debug)]
pub struct MainBenchRunner {
    inner: BenchRunner,
}

impl MainBenchRunner {
    pub fn new() -> Self {
        let mut clock = Clock::new();

        let start = clock.now();
        Self {
            inner: BenchRunner {
                global_start: Arc::new(start),
                id_bank: Arc::new(AtomicU64::new(0)),
                clock: clock,

                id: String::from("main_runner"),
                runner_start: start,
                log: vec![],

                completed: CompletionGuard::new("main_runner".into()),
            },
        }
    }

    pub fn spawn_runner(&self, id: String) -> BenchRunner {
        self.inner.spawn_runner(id)
    }

    pub fn complete_runner(self) -> anyhow::Result<()> {
        self.inner
            .complete_runner()
            .context("failed to complete main runner")
    }
}

#[derive(Debug)]
pub struct BenchRunner {
    id_bank: Arc<AtomicU64>,
    global_start: Arc<Instant>,
    clock: Clock,

    id: String,
    runner_start: Instant,
    log: Vec<BenchEvent>,

    completed: CompletionGuard,
}

impl BenchRunner {
    pub fn spawn_runner(&self, id: String) -> Self {
        let id = format!("{}::{}", self.id, id);
        let mut clock = self.clock.clone();
        let start = clock.now();
        Self {
            global_start: Arc::new(clock.now()),
            id_bank: self.id_bank.clone(),
            clock: clock,

            id: id.clone(),
            runner_start: start,
            log: vec![],

            completed: CompletionGuard::new(id),
        }
    }

    pub fn next_id(&self) -> u64 {
        self.id_bank.fetch_add(1, Ordering::Relaxed)
    }

    pub fn clock(&self) -> Clock {
        self.clock.clone()
    }

    pub fn override_start(&mut self, start: Instant) {
        self.runner_start = start;
    }

    pub fn start_event<'a>(&'a mut self) -> EventGuard<'a> {
        EventGuard {
            start: self.clock.now(),
            runner: self,
        }
    }

    pub fn complete_runner(mut self) -> anyhow::Result<()> {
        let end = self.clock.now();

        let mut dst = Path::new("results").to_path_buf();
        let splits = self.id.split("::");
        let mut last = "";
        for split in splits {
            dst = dst.join(split);
            last = split;
        }
        dst.pop();

        let last = last.to_owned() + ".bin";

        create_dir_all(&dst)?;
        let mut file = File::create(&dst.join(last))?;

        write_all_bench_log(
            self.log,
            *self.global_start,
            self.runner_start,
            end,
            &mut file,
        )
        .context(format!("failed to write benchmark to {dst:?}"))?;

        self.completed.complete();

        Ok(())
    }
}

// Not using Drop, results in unnecessary copying (can't move out of self).
pub struct EventGuard<'a> {
    start: Instant,
    runner: &'a mut BenchRunner,
}

impl<'a> EventGuard<'a> {
    pub fn finish(self, id: u64) {
        self.runner.log.push(BenchEvent {
            start: self.start,
            end: self.runner.clock.now(),
            id: id,
        })
    }
}

#[derive(Debug)]
pub struct BenchEvent {
    pub start: Instant,
    pub end: Instant,
    pub id: u64,
}

fn write_all_bench_log(
    log: Vec<BenchEvent>,
    global_start: Instant,
    runner_start: Instant,
    runner_end: Instant,
    writer: impl std::io::Write,
) -> anyhow::Result<()> {
    let mut writer = BufWriter::with_capacity(RUNNER_WRITER_BUFFER_SIZE, writer);
    // file layout (little-endian):
    // 8 bytes start_t_secs f64
    // 8 bytes end_t_ms secs f64
    // 8 bytes padding (for hex viewers)
    //
    // for each row:
    //  8 bytes start_t_secs f64
    //  8 bytes end_t_secs f64
    //  8 bytes id u64

    writer
        .write(
            &runner_start
                .duration_since(global_start)
                .as_secs_f64()
                .to_le_bytes(),
        )
        .context("failed to write runner_start")?;

    writer
        .write(
            &runner_end
                .duration_since(global_start)
                .as_secs_f64()
                .to_le_bytes(),
        )
        .context("failed to write runner_end")?;

    writer
        .write(&0u64.to_le_bytes())
        .context("failed to write padding bytes")?;

    for row in log {
        writer
            .write(
                &row.start
                    .duration_since(global_start)
                    .as_secs_f64()
                    .to_le_bytes(),
            )
            .context("failed to write row_start")?;

        writer
            .write(
                &row.end
                    .duration_since(global_start)
                    .as_secs_f64()
                    .to_le_bytes(),
            )
            .context("failed to write row_end")?;

        writer
            .write(&row.id.to_le_bytes())
            .context("failed to write row_id")?;
    }

    Ok(())
}
