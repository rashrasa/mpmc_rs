use std::{collections::HashMap, thread, time::Instant};

use mpac_rs::{BlockingReceive, BlockingSend, ChannelMaker};

use crate::runner::{BenchEventData, BenchRunner};

#[derive(Clone)]
pub struct Config {
    pub n_senders: usize,
    pub n_receivers: usize,
    pub sender_config: SenderConfig,
}

#[derive(Clone)]
pub enum SenderConfig {
    TimeToLiveSeconds(f64),
    NumberOfRequests(u64),
}

pub fn run_bench_1<Maker>(runner: &BenchRunner, maker: &Maker, config: Config)
where
    Maker: ChannelMaker,
{
    let mut handles = vec![];

    // Scope to ensure values get dropped appropriately
    {
        let (tx, rx) = maker.channel();
        for i in 0..config.n_senders {
            let mut tx_runner = runner.spawn_runner(format!("tx_runner_{}", i));
            let tx_thread = tx.clone();
            let config_thread = config.clone();
            let s_h: thread::JoinHandle<()> = thread::spawn(move || {
                let config = config_thread;
                let start = Instant::now();
                let mut n_sent = 0u64;

                let mut counter = 0u64;
                let tx = tx_thread;
                loop {
                    if let Ok(_) = tx.send(counter) {
                        tx_runner.record(
                            BenchEventData::ValueSent,
                            HashMap::from([("value".into(), counter.into())]),
                        );
                        counter += 1;
                        n_sent += 1;
                    } else {
                        break;
                    }
                    match config.sender_config {
                        SenderConfig::TimeToLiveSeconds(ttl_s) => {
                            if start.elapsed().as_secs_f64() > ttl_s {
                                break;
                            }
                        }
                        SenderConfig::NumberOfRequests(n) => {
                            if n_sent >= n {
                                break;
                            }
                        }
                    }
                }
            });
            handles.push(s_h);
        }

        for i in 0..config.n_receivers {
            let mut rx_runner = runner.spawn_runner(format!("rx_runner_{}", i));
            let rx_thread = rx.clone();
            let r_h = thread::spawn(move || {
                let rx = rx_thread;
                while let Ok(r) = rx.recv() {
                    rx_runner.record(
                        BenchEventData::ValueReceived,
                        HashMap::from([("value".into(), r.into())]),
                    );
                }
            });
            handles.push(r_h);
        }
    }
    for handle in handles {
        handle.join().unwrap();
    }
}
