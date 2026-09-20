//! `testing::pin_global_trace_floor` in its own test binary: it installs a process-global
//! subscriber, which must not meet the lib tests that expect to install their own.

use tracing::level_filters::LevelFilter;
use txtodo_telemetry::testing::{LogSink, capturing_dispatch, pin_global_trace_floor};

fn emit() {
    tracing::debug!(n = 1, "pin_floor_probe");
}

#[test]
fn the_pin_keeps_the_max_level_open_and_capture_still_works() {
    // First hit of the callsite on a thread with no subscriber: the state a sibling test leaves.
    std::thread::spawn(emit).join().unwrap();

    pin_global_trace_floor();
    pin_global_trace_floor(); // a second call is a no-op, never a panic

    let sink = LogSink::new();
    let dispatch = capturing_dispatch(sink.clone(), "pin-floor-test");
    let guard = tracing::dispatcher::set_default(&dispatch);
    emit();
    drop(guard);

    let text = sink.captured_text();
    assert!(text.contains("pin_floor_probe"), "captured: {text}");
    // With no scoped dispatch left, only the pinned global subscriber holds the max level up.
    // Without the pin this is OFF, and a sibling thread's callsite is cached as "nobody listens".
    // Ref: https://docs.rs/tracing/latest/tracing/level_filters/struct.LevelFilter.html#method.current
    assert_eq!(LevelFilter::current(), LevelFilter::TRACE);
}
