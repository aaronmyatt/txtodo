//! `ChannelLink`: a send on one side is a recv on the other, closing propagates, and the queue cap
//! is enforced rather than growing without bound.

use std::thread;

use crate::frame::Frame;
use crate::link::{Link, LinkError, MAX_QUEUED_FRAMES, channel_link_pair};

fn frame(n: u8) -> Frame {
    Frame {
        version: 1,
        body: vec![n],
    }
}

#[test]
fn a_send_on_one_side_is_a_recv_on_the_other() {
    let (mut a, mut b) = channel_link_pair();
    a.send(frame(1)).unwrap();
    assert_eq!(b.recv().unwrap(), frame(1));
    b.send(frame(2)).unwrap();
    assert_eq!(a.recv().unwrap(), frame(2));
}

#[test]
fn frames_are_delivered_in_order() {
    let (mut a, mut b) = channel_link_pair();
    for i in 0..10 {
        a.send(frame(i)).unwrap();
    }
    for i in 0..10 {
        assert_eq!(b.recv().unwrap(), frame(i));
    }
}

#[test]
fn dropping_the_sender_makes_the_receivers_recv_return_closed() {
    let (a, mut b) = channel_link_pair();
    drop(a);
    assert!(matches!(b.recv(), Err(LinkError::Closed)));
}

#[test]
fn a_blocking_recv_wakes_up_when_a_frame_arrives_from_another_thread() {
    let (mut a, mut b) = channel_link_pair();
    let sender = thread::spawn(move || {
        a.send(frame(42)).unwrap();
    });
    assert_eq!(b.recv().unwrap(), frame(42));
    sender.join().unwrap();
}

#[test]
fn the_queue_cap_is_enforced_rather_than_growing_without_bound() {
    let (mut a, _b) = channel_link_pair();
    for i in 0..MAX_QUEUED_FRAMES {
        a.send(frame((i % 256) as u8)).unwrap();
    }
    assert!(matches!(a.send(frame(0)), Err(LinkError::Io(_))));
}
