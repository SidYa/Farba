//! Data parallelism for the heavy pixel loops: all cores in the native build (the Tauri
//! backend), one thread in WASM (`wasm32-unknown-unknown` has no threads). Work items are
//! independent, so the results are the same either way.

/// How many threads the parallel helpers use.
pub fn threads() -> usize {
    if let Some(n) = OVERRIDE.get() {
        return n;
    }
    #[cfg(target_arch = "wasm32")]
    {
        1
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // FARBA_THREADS overrides it (benchmarks).
        static N: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
        *N.get_or_init(|| {
            std::env::var("FARBA_THREADS")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|&n| n > 0)
                .unwrap_or_else(|| std::thread::available_parallelism().map_or(4, |n| n.get()))
        })
    }
}

thread_local! {
    static OVERRIDE: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// Runs `f` with the parallel helpers limited to `n` threads on this thread (tests compare
/// one thread with all cores).
pub fn with_threads<R>(n: usize, f: impl FnOnce() -> R) -> R {
    let old = OVERRIDE.replace(Some(n.max(1)));
    let r = f();
    OVERRIDE.set(old);
    r
}

/// Calls `f(index, chunk)` for every `chunk_len`-sized chunk of `data` (the last may be
/// shorter), spread over all cores.
pub fn chunks_mut<T: Send>(data: &mut [T], chunk_len: usize, f: impl Fn(usize, &mut [T]) + Sync) {
    run(data, chunk_len.max(1), threads(), f);
}

/// `(0..n).map(f).collect()`, spread over at most `max_threads` threads (memory-hungry
/// work items limit how many run at once).
pub fn map<R: Send>(n: usize, max_threads: usize, f: impl Fn(usize) -> R + Sync) -> Vec<R> {
    let mut out: Vec<Option<R>> = (0..n).map(|_| None).collect();
    run(&mut out, 1, max_threads.min(threads()), |i, slot| {
        slot[0] = Some(f(i))
    });
    out.into_iter()
        .map(|r| r.expect("every item is computed"))
        .collect()
}

/// Hands out the chunks to `threads` threads, one chunk at a time.
fn run<T: Send>(
    data: &mut [T],
    chunk_len: usize,
    threads: usize,
    f: impl Fn(usize, &mut [T]) + Sync,
) {
    let threads = threads.min(data.len().div_ceil(chunk_len));
    #[cfg(not(target_arch = "wasm32"))]
    if threads > 1 {
        let queue = std::sync::Mutex::new(data.chunks_mut(chunk_len).enumerate());
        std::thread::scope(|s| {
            for _ in 0..threads {
                s.spawn(|| {
                    loop {
                        let next = queue.lock().unwrap().next();
                        let Some((i, chunk)) = next else { break };
                        f(i, chunk);
                    }
                });
            }
        });
        return;
    }
    let _ = threads;
    data.chunks_mut(chunk_len)
        .enumerate()
        .for_each(|(i, c)| f(i, c));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_and_map_cover_everything_in_order() {
        let mut v = vec![0usize; 1000];
        chunks_mut(&mut v, 7, |i, c| {
            for (j, x) in c.iter_mut().enumerate() {
                *x = i * 7 + j;
            }
        });
        assert!(v.iter().enumerate().all(|(i, &x)| i == x));
        assert_eq!(
            map(100, 3, |i| i * 2),
            (0..100).map(|i| i * 2).collect::<Vec<_>>()
        );
    }
}
