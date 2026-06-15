#![cfg_attr(feature = "unstable", feature(test))]

// TODO: potentially instantiate logs with an extremely high capacity
pub mod aggregate;
pub mod completion;
pub mod runner;
pub mod test;

const RUNNER_WRITER_BUFFER_SIZE: usize = 64 * 1024;

#[cfg(test)]
mod tests {
    use fast_time::Clock;
    use std::time::Duration;

    #[cfg(feature = "unstable")]
    use std::time::{Instant, SystemTime};

    #[test]
    fn test_fast_time_cloned_delayed() {
        let mut clock = Clock::new();

        let start = clock.now();
        std::thread::sleep(Duration::from_millis(2500));
        let end = clock.clone().now();
        let elapsed = end.duration_since(start).as_secs_f64();
        println!("elapsed: {:.17}", elapsed);
        assert!(elapsed >= 2.4_999_999_999_999);
    }

    #[cfg(feature = "unstable")]
    extern crate test;
    #[cfg(feature = "unstable")]
    use test::Bencher;

    #[cfg(feature = "unstable")]
    #[bench]
    fn bench_instant_now(bencher: &mut Bencher) {
        bencher.iter(|| Instant::now());
    }

    #[cfg(feature = "unstable")]
    #[bench]
    fn bench_instant_elapsed_f64(bencher: &mut Bencher) {
        let now = Instant::now();

        bencher.iter(|| now.elapsed().as_secs_f64());
    }

    #[cfg(feature = "unstable")]
    #[bench]
    fn bench_system_time_now(bencher: &mut Bencher) {
        bencher.iter(|| SystemTime::now());
    }

    #[cfg(feature = "unstable")]
    #[bench]
    fn bench_system_time_elapsed_f64(bencher: &mut Bencher) {
        let now = SystemTime::now();

        bencher.iter(|| now.elapsed().unwrap().as_secs_f64());
    }

    #[cfg(feature = "unstable")]
    #[bench]
    fn bench_fast_time_now(bencher: &mut Bencher) {
        let mut clock = Clock::new();

        bencher.iter(|| clock.now());
    }

    #[cfg(feature = "unstable")]
    #[bench]
    fn bench_fast_time_elapsed_f64(bencher: &mut Bencher) {
        let mut clock = Clock::new();
        let now = clock.now();

        bencher.iter(|| now.elapsed(&mut clock).as_secs_f64());
    }
}
