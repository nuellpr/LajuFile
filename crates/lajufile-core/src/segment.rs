//! Segment planning for ranged downloads.

use crate::store::Segment;

pub const SEGMENT_BYTES: u64 = 1024 * 1024;

/// Split a known file size into contiguous segments. The last segment takes
/// the remainder so the ranges always cover `0..=size - 1`.
pub fn plan_segments(size: u64, ranged: bool, max_connections: u32) -> Vec<Segment> {
    let count = if ranged {
        (size / SEGMENT_BYTES).clamp(1, max_connections.max(1) as u64)
    } else {
        1
    } as u32;

    if size == 0 {
        return vec![Segment {
            idx: 0,
            start: 0,
            end: 0,
            done: 0,
        }];
    }

    let chunk = size / count as u64;
    (0..count)
        .map(|idx| {
            let start = idx as u64 * chunk;
            let end = if idx + 1 == count {
                size - 1
            } else {
                start + chunk - 1
            };
            Segment {
                idx,
                start,
                end,
                done: 0,
            }
        })
        .collect()
}
