//! A refused edit against a real `txtodod` (task `tui-revamp/tui-shell`): the loop keeps going,
//! the edit's text is kept for the banner's Copy edit, and the next saved edit clears it.
//! Unix-only and `#[ignore]`d like this crate's other real-daemon tests.
#![cfg(unix)]

mod support;

use txtodo_proto::v1 as pb;
use txtodo_tui::action::Action;
use txtodo_tui::app::perform;
use txtodo_tui::state::AppState;

fn edit(line_number: u32, text: &str) -> Action {
    Action::Apply(pb::ApplyRequest {
        path: "todo.txt".to_owned(),
        mutations: vec![pb::Mutation {
            kind: Some(pb::mutation::Kind::Edit(pb::Edit {
                task: Some(pb::TaskRef {
                    line_number,
                    task_id: String::new(),
                }),
                new_line: text.to_owned(),
            })),
        }],
        source: "tui".to_owned(),
        ..pb::ApplyRequest::default()
    })
}

#[ignore = "spawns a real txtodod; CI-only, see ci.yml's --ignored step"]
#[tokio::test]
async fn a_refused_edit_is_kept_for_copy_until_the_next_saved_one() {
    let (_real, mut daemon) = support::RealDaemon::start("buy milk\n").await;
    let mut state = AppState::from_document("todo.txt", "buy milk");

    let keep = perform(&mut daemon, &mut state, edit(9, "call mom"))
        .await
        .unwrap_or_else(|e| panic!("perform: {e}"));
    assert!(keep, "a refusal never ends the session");
    let refused = state
        .shell
        .refused
        .clone()
        .unwrap_or_else(|| panic!("line 9 does not exist, so the edit is refused"));
    assert_eq!(refused.text, "call mom");
    assert!(!refused.error.is_empty());

    perform(&mut daemon, &mut state, edit(1, "buy oat milk"))
        .await
        .unwrap_or_else(|e| panic!("perform: {e}"));
    assert_eq!(state.shell.refused, None, "a saved edit clears it");
    assert_eq!(state.lines[0].raw, "buy oat milk");
}
