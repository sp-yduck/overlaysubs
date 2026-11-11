//! Measurements for the real-model benchmark (`bench_report` in src-tauri, run by `scripts/bench.sh`).
use std::collections::VecDeque;

/// Character error rate: edit distance over the reference length, counting letters and digits only
/// and ignoring case, so punctuation and spacing do not count and unspaced scripts (ja, zh) work.
/// 0 is identical; it can exceed 1 when the hypothesis is much longer than the reference.
pub fn cer(reference: &str, hypothesis: &str) -> f64 {
    let norm = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<Vec<_>>();
    let (r, h) = (norm(reference), norm(hypothesis));
    let mut row: Vec<usize> = (0..=h.len()).collect();
    for (i, rc) in r.iter().enumerate() {
        let mut diag = row[0];
        row[0] = i + 1;
        for (j, hc) in h.iter().enumerate() {
            let sub = diag + usize::from(rc != hc);
            diag = row[j + 1];
            row[j + 1] = sub.min(diag + 1).min(row[j] + 1);
        }
    }
    row[h.len()] as f64 / r.len().max(1) as f64
}

#[derive(Debug, Default, PartialEq)]
pub struct Replayed {
    /// Per final: seconds from the endpointer emitting it to its last caption being done.
    pub final_delays: Vec<f64>,
    pub partials_run: usize,
    pub partials_skipped: usize,
    /// Seconds the engines were working.
    pub busy: f64,
}

/// Replays the app's session loop (`start` in src-tauri) on a virtual clock, so a recorded clip
/// shows the delays live audio would: the loop waits for an event, takes everything queued by
/// then, drops every partial that is not the newest, and runs the rest in order.
/// `events` are `(emitted at, in seconds of audio; is final; payload)` in order; `run` handles one
/// and returns the seconds it took.
pub fn replay<E>(events: Vec<(f64, bool, E)>, mut run: impl FnMut(E) -> f64) -> Replayed {
    let mut r = Replayed::default();
    let mut queue: VecDeque<_> = events.into();
    let mut now = 0.0f64;
    while let Some(first) = queue.pop_front() {
        now = now.max(first.0);
        let mut batch = vec![first];
        while queue.front().is_some_and(|e| e.0 <= now) {
            batch.extend(queue.pop_front());
        }
        let n = batch.len();
        for (i, (at, fin, ev)) in batch.into_iter().enumerate() {
            if !fin && i != n - 1 {
                r.partials_skipped += 1;
                continue;
            }
            let took = run(ev);
            now += took;
            r.busy += took;
            if fin {
                r.final_delays.push(now - at);
            } else {
                r.partials_run += 1;
            }
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cer_ignores_case_punctuation_and_spacing() {
        assert_eq!(cer("Hello, world.", "hello world"), 0.0);
        assert_eq!(cer("今日は、いい天気。", "今日はいい天気"), 0.0);
        assert_eq!(cer("abcd", "abxd"), 0.25, "one substitution in four");
        assert_eq!(cer("abcd", "abd"), 0.25, "one deletion");
        assert_eq!(cer("ab", "abcd"), 1.0, "two insertions over a reference of two");
        assert_eq!(cer("", ""), 0.0);
    }

    #[test]
    fn replay_queues_finals_and_drops_stale_partials() {
        // Partials at 1 s and 2 s, a final at 3 s; each event takes 1.5 s.
        // 1.0–2.5: first partial. At 2.5 only the 2 s partial is queued, so it runs until 4.0.
        // The final emitted at 3.0 waited for it and is done at 5.5.
        let r = replay(vec![(1.0, false, ()), (2.0, false, ()), (3.0, true, ())], |_| 1.5);
        assert_eq!(r, Replayed { final_delays: vec![2.5], partials_run: 2, partials_skipped: 0, busy: 4.5 });

        // Slower engines: by 4.0 both the second partial and the final are queued, so the partial is dropped.
        let r = replay(vec![(1.0, false, ()), (2.0, false, ()), (3.0, true, ())], |_| 3.0);
        assert_eq!(r, Replayed { final_delays: vec![4.0], partials_run: 1, partials_skipped: 1, busy: 6.0 });

        // Fast engines are idle between events: the delay is just the work.
        let r = replay(vec![(1.0, false, ()), (3.0, true, ())], |_| 0.25);
        assert_eq!((r.final_delays, r.partials_run), (vec![0.25], 1));
    }
}
