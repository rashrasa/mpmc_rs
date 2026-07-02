# Multi-Producer, Multi-Consumer Channel

An MPMC unbounded concurrent queue, a custom benchmark engine, and an optimized event aggregator.

Used `Miri` to flag unsoundness and undefined behaviour when using `unsafe`.

Used industry-standard tools like `cargo-flamegraph` to discover optimization opportunities concluding with a 44x speedup for the aggregator.

Benchmarked multiple implementations against the unbounded MPMC channel from `crossbeam` (a popular crate).

- Version 1: `Vec<T>` with a `Mutex`
- Version 2: `VecDeque<T>` with a `Mutex`
- Version 3: Linked List with head/tail spinlocks
- Version 4: Resizable Atomic Queue using `Vec<Location<T>>` internally
    - `Location<T>` are reusable slots
    - State management for Readers and Writers (mechanism for gaining exclusivity)
    - Access through atomic indices
- Version 5: Same as Version 4 but:
    - using `parking_lot::RwLock` instead of stateful access
        - A "read" is a shared access (push and pop through atomic indices)
        - A "write" is an exclusive access (resize)

## Aggregator Optimizations

Collects raw data from the Benchmark and produces useful aggregations such as throughput, send/recv delay values, data latency values, etc. for plotting.

- Can be found primarily at `bench/src/aggregate.rs` and `bench/src/aggregate/metric.rs`

### Optimization Round 1

*Note: these times were measured once and there may be some hidden variance unaccounted for. Round 2 uses a more appropriate approach.*

Given `13.4 GB` of raw benchmark data,

Went from `400.32s` with the initial implementation,

down to `8.98s` (`44.6x` speedup) with all optimizations.

#### Speedup 1: Lazy Errors in Hot Loops

Time down to `305.34s`

- Lazily evaluating error string in `LazyWindowedMetric::add` on Option value (using `ok_or_else` instead of `ok_or`) in hot loop

#### Speedup 2: Sorting at the End Instead of Inserting in Sorted Order

Time down to `94.60s`

- Sorting aggregation bucket values lazily in `LazyWindowedMetric::generate` 
- Meaning there are no more inserts inside each bucket, only pushes at their ends
- No longer using sorted values in `LazyWindowedMetric::generate_gauged` (was only necessary to find percentiles)

#### Speedup 3: Lists instead of Hashmaps

Time down to `29.70s`

- `Vec` instead of `HashMap` for `u64` keys
    - Had to update benchmark runner to reset event ids to 0 for each run
- Estimating number of events by summing file sizes then pre-allocating entire `Vec` at the start

#### Speedup 4: Multithreading

Time down to `8.98s`

- Spawned a thread for each run (version/config pair)

### (WIP) Optimization Round 2

- `101 GB` of raw benchmark data

`hyperfine` initial result (3 warmups, 10 runs): 

- Mean: `304s`
- Standard deviation: `160s` (very high!)
- Range: `176s --- 583s`
- User Time: `142s`
- System Time: `125s`

#### Speedup 1: Memory-Mapped Raw Benchmark Data

- Every raw benchmark binary data file was memory-mapped (using crate `memmap2`)

`hyperfine` result (3 warmups, 10 runs): 

- Mean: `140s` (**2.2x improvement**)
- Standard deviation: `70s`
- Range: `71s --- 277s`
- User Time: `199s`
- System Time: `154s`

## Benchmark

A thread is spawned for each receiver and sender. Senders call `send` as many times as possible and Receivers attempt to `recv` all events in the channel for some number of seconds.

Each event contains:

- Start time
- End time
- ID
- Backpressure

After the benchmark is run, events recorded by senders are matched with ones from receivers and an aggregation is run across all complete and partial (sender/receiver half only) event data.

- Can be found primarily at `bench/src/runner.rs` and `bench/src/test/test_1.rs`

### Optimizations

- Used a raw binary encoding for event files instead of UTF-8
- Delegated full-event re-construction to the aggregator, to avoid synchronizing senders and receivers
- Tree-like structure for benchmark runners
    - they write to file as soon as a thread is done its work, avoids writing gigabytes of data all at once
    - each thread can record its own events without contending with others

## Overall Results

### Configuration

- 3 Senders
- 3 Receivers
- 5s TTL
- 4 byte payload

### Version 1

Implemented as a `Vec`, safe access through a single `Mutex` and shared with `Arc`. Receivers pop using `remove(0)` and busy-wait (with `sleep`) until there is an item in the queue, or the queue is empty and the sender count is 0. Senders add to the queue if the receiver count is >0.

### Version 2

Uses a VecDeque instead of a Vec and is guarded by a Mutex. Basic optimization but results in a significantly better channel.

#### Optimization

`remove(0)` for `Vec` shuffles every element back and returns the first element. This operation was O(n). 

`VecDeque` maintains a ring buffer which, instead of one length field, stores a start and end. This way, removing an element using `pop_front` from the front involves an addition and moduluo operation and is O(1).

### Version 3

Implemented as a concurrent linked-list using atomics. Functionally works as a list with a spin-lock at each end.

### Version 4

Implemented as a concurrent array using atomics. To meet the requirement of being unbounded, a mechanism for re-allocating the array was created.

### Crossbeam

They implemented it as a linked-list of blocks, each with a constant number of slots that can each hold an item.

## Run

`./run.sh bench aggregate plot` or `./run.sh` to run all stages,

exclude any arg to only run specified stages,

open `output/plots/*.html` to view plots.
