use lajufile_core::segment::{SEGMENT_BYTES, plan_segments};

#[test]
fn plans_one_segment_for_small_or_non_ranged() {
    let small = plan_segments(512 * 1024, true, 8);
    assert_eq!(small.len(), 1);
    assert_eq!((small[0].start, small[0].end), (0, 512 * 1024 - 1));

    let plain = plan_segments(10 * SEGMENT_BYTES, false, 8);
    assert_eq!(plain.len(), 1);
    assert_eq!((plain[0].start, plain[0].end), (0, 10 * SEGMENT_BYTES - 1));
}

#[test]
fn splits_evenly_and_gives_remainder_to_last() {
    let segments = plan_segments(4 * SEGMENT_BYTES + SEGMENT_BYTES / 2, true, 8);
    assert_eq!(segments.len(), 4);
    assert_eq!((segments[0].start, segments[0].end), (0, 1_179_647));
    assert_eq!((segments[3].start, segments[3].end), (3_538_944, 4_718_591));
    assert!(segments.iter().all(|segment| segment.done == 0));
}

#[test]
fn clamps_connection_count_to_one_and_limit() {
    assert_eq!(plan_segments(8 * SEGMENT_BYTES, true, 0).len(), 1);
    assert_eq!(plan_segments(8 * SEGMENT_BYTES, true, 3).len(), 3);
    assert_eq!(plan_segments(8 * SEGMENT_BYTES, true, 64).len(), 8);
}

#[test]
fn zero_size_produces_one_empty_range() {
    let segments = plan_segments(0, true, 8);
    assert_eq!(segments.len(), 1);
    assert_eq!((segments[0].start, segments[0].end), (0, 0));
}
