use std::time::Instant;

use mpac_rs::{BlockingReceive, BlockingSend};

/// ## Bench:
///
/// - 1 core reserved for gathering metrics
///
/// ### Metrics (Mean, P{50, 90, 99, 999}):
///
/// - Send/Receive Throughput
/// - Sender/Receiver Latency
///
/// #### Other
///
/// - Metrics' scaling with # of Sender/Receiver threads
///
/// ### Scenarios
///
/// - Pure value channel
///     - 1-1, 7-1, 1-7, 4-4, 7-7 sender-receiver threads
///     - T sizes: 4 bytes, 64 bytes, 8 kB, 64 kB
///
/// - One request and one response channel
///     - 1-1, 4-4, 6-1, 1-6 sender-receiver threads for each channel
/// - Sending sequenced data which has to be re-constructed and ordered by receivers
///     - 1 unique series per sender
///     - all receivers need to cooperate for each series and maintain a collection of sequenced values
///     - (1-1, 7-1, 1-7, 4-4, 7-7)
fn main() {
    let v1_results = run_bench(mpac_rs::v1::channel::<usize>());
}

fn run_bench<T, Sender: BlockingSend<T>, Receiver: BlockingReceive<T>>(
    (tx, rx): (Sender, Receiver),
) {
    let start = Instant::now();
}
