//! Inert Stage 1 must not expose a partial UI path, even to conforming peers.
use super::*;
use crate::worker::{Event, Worker};
use cedar_protocol::{CaseSearchDescriptor, CaseSearchRequest};

#[test]
fn case_search_stage1_validates_hello_but_never_admits_the_operation() {
    for descriptor in [
        None,
        Some(CaseSearchDescriptor::V1),
        Some(CaseSearchDescriptor { version: 2 }),
        Some(CaseSearchDescriptor { version: u32::MAX }),
        Some(CaseSearchDescriptor { version: 0 }),
    ] {
        for trust in [false, true] {
            let mut app = CedarApp::empty();
            let (worker, commands) = Worker::recording();
            app.worker = Some(worker);
            app.state = ConnectionState::Connecting;
            app.connecting_form = Some(ConnectForm {
                local_root: "/project".into(),
                allow_run: trust,
                ..Default::default()
            });
            app.apply_event(Event {
                generation: app.generation,
                id: 0,
                connected: true,
                result: Ok(Payload::Hello {
                    protocol: cedar_protocol::PROTOCOL_VERSION,
                    root: "/project".into(),
                    agent: Some(agent_support::full_test_agent()),
                    file_discovery: None,
                    scoped_search: Some(cedar_protocol::ScopedSearchDescriptor::V1),
                    case_search: descriptor,
                }),
            });
            if descriptor.is_some_and(|value| value.version == 0) {
                assert!(!app.ready());
                assert!(commands.try_recv().is_err());
                continue;
            }
            assert!(app.ready());
            assert!(app.backend_scoped_search_supported());
            assert!(matches!(
                commands.try_recv().unwrap().op,
                Operation::List { .. }
            ));
            for case_sensitive in [false, true] {
                let operation = Operation::SearchCase {
                    request: CaseSearchRequest {
                        query: "needle".into(),
                        scope: String::new(),
                        masks: vec![],
                        limit: 1000,
                        case_sensitive,
                    },
                };
                assert!(app
                    .operation_problem(&operation)
                    .unwrap()
                    .contains("not enabled"));
                assert_eq!(app.request(operation, Job::Git), 0);
                assert!(commands.try_recv().is_err());
            }
            assert!(app
                .operation_problem(&Operation::Search {
                    query: "needle".into(),
                    limit: 1000
                })
                .is_none());
            assert!(app
                .operation_problem(&Operation::SearchIn {
                    request: cedar_protocol::ScopedSearchRequest {
                        query: "needle".into(),
                        scope: "src".into(),
                        masks: vec!["*.rs".into()],
                        limit: 1000,
                    }
                })
                .is_none());
        }
    }
}
