use std::{
    sync::{Arc, Barrier},
    thread,
    time::Instant,
};

use anyhow::Context;
use log::{debug, error};
use mpac_rs::{BBlockingReceive, BBlockingSend, BChannelMaker};

use crate::runner::BenchRunner;

#[derive(Clone)]
pub struct Config<T> {
    pub n_senders: usize,
    pub n_receivers: usize,
    pub sender_ttl_s: Option<f64>,
    pub receiver_ttl_s: Option<f64>,
    pub make_payload: fn() -> T,
}

struct Message<T> {
    id: u64,
    _payload: T,
}

pub fn run_bench_1<T, Maker>(
    runner: &BenchRunner,
    maker: Maker,
    config: Config<T>,
) -> anyhow::Result<()>
where
    T: Clone + Send + 'static,
    Maker: BChannelMaker,
{
    let mut handles = vec![];

    if config.receiver_ttl_s.is_none() && config.sender_ttl_s.is_none() {
        return Err(anyhow::Error::msg(
            "Time-To-Live must be set for either senders, receivers, or both.",
        ));
    }

    let start_flag: Arc<Barrier> =
        Arc::new(Barrier::new(config.n_receivers + config.n_senders + 1));

    let (tx, rx) = maker.channel();

    for i in 0..config.n_senders {
        let runner = runner.spawn_runner(format!("tx_runner_{}", i));
        let tx = tx.clone();
        let config = config.clone();
        let start_flag = Arc::clone(&start_flag);
        let s_h: thread::JoinHandle<()> = thread::spawn(move || {
            sender_thread(start_flag, config.clone(), tx, runner, config.make_payload);
        });
        handles.push(s_h);
    }

    for i in 0..config.n_receivers {
        let rx = rx.clone();
        let runner = runner.spawn_runner(format!("rx_runner_{}", i));
        let config = config.clone();
        let start_flag = Arc::clone(&start_flag);
        let r_h = thread::spawn(move || {
            receiver_thread(start_flag, config.clone(), rx, runner);
        });
        handles.push(r_h);
    }
    drop(tx);
    drop(rx);

    start_flag.wait();

    for handle in handles {
        handle.join().unwrap();
    }

    Ok(())
}

fn sender_thread<T: Send>(
    start_flag: Arc<Barrier>,
    config: Config<T>,
    mut tx: impl BBlockingSend<Message<T>>,
    mut runner: BenchRunner,
    make_payload: fn() -> T,
) {
    start_flag.wait();

    debug!("(Sender) Received start signal");

    let start = Instant::now();
    runner.override_start(start);
    loop {
        if sender_work(&mut tx, &mut runner, make_payload).is_err() {
            break;
        }
        if !keep_sender_alive(&config.sender_ttl_s, &start) {
            break;
        }
    }

    // Do not remove, otherwise channel has to wait extra to close.
    drop(tx);

    if let Err(err) = runner
        .complete_runner()
        .context("failed to complete runner")
    {
        error!("{err}");
    }
}

fn sender_work<T: Send>(
    tx: &mut impl BBlockingSend<Message<T>>,
    runner: &mut BenchRunner,
    make_playload: fn() -> T,
) -> anyhow::Result<()> {
    let id = runner.next_id();
    let message = Message {
        id,
        _payload: make_playload(),
    };

    let event = runner.start_event();
    if let Ok(len) = tx.b_send(message) {
        event.finish(id, len as u64);
    } else {
        return Err(anyhow::Error::msg("channel closed"));
    };
    Ok(())
}

fn keep_sender_alive(ttl: &Option<f64>, start: &Instant) -> bool {
    if let Some(ttl) = ttl {
        if start.elapsed().as_secs_f64() > *ttl {
            return false;
        }
        return true;
    }
    true
}

fn receiver_thread<T: Send>(
    start_flag: Arc<Barrier>,
    config: Config<T>,
    mut rx: impl BBlockingReceive<Message<T>>,
    mut runner: BenchRunner,
) {
    start_flag.wait();

    debug!("(Receiver) Received start signal");

    let start = Instant::now();
    runner.override_start(start);
    loop {
        if receiver_work(&mut rx, &mut runner).is_err() {
            break;
        }
        if !keep_receiver_alive(&config.receiver_ttl_s, &start) {
            break;
        }
    }

    // Do not remove, otherwise channel has to wait extra to close.
    drop(rx);

    if let Err(err) = runner
        .complete_runner()
        .context("failed to complete runner")
    {
        error!("{err}");
    }
}

fn receiver_work<T: Send>(
    rx: &mut impl BBlockingReceive<Message<T>>,
    runner: &mut BenchRunner,
) -> anyhow::Result<()> {
    let event_guard = runner.start_event();
    match rx.b_recv() {
        Ok((r, len)) => {
            event_guard.finish(r.id, len as u64);
            Ok(())
        }
        Err(_) => Err(anyhow::Error::msg("channel closed")),
    }
}

fn keep_receiver_alive(ttl: &Option<f64>, start: &Instant) -> bool {
    if let Some(ttl) = ttl {
        if start.elapsed().as_secs_f64() > *ttl {
            return false;
        }
        return true;
    }
    true
}
