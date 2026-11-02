//! The plugin against the host's side of the protocol, and against GitLab.
//!
//! The unit tests check that the pieces are right. These run the built binary
//! exactly as kasl runs it - `--manifest`, then `run` with JSON on stdin -
//! with a stand-in GitLab on a local port, and hold the answers to what kasl
//! will accept. A plugin that passes its unit tests and fails here is one
//! kasl would list and refuse.

use std::io::Write;
use std::process::{Command, Stdio};

use assert_cmd::cargo::cargo_bin;
use mockito::{Matcher, Server, ServerGuard};
use serde_json::{Value, json};

/// The protocol version kasl speaks. Written out rather than read from
/// the plugin: a test that asks the code what it should be agrees with any
/// answer, including a wrong one.
const HOST_PROTOCOL_VERSION: u32 = 1;

/// The requests kasl knows. A manifest naming anything else is refused whole.
const HOST_REQUESTS: &[&str] = &["list_issues", "transition", "comment", "find_tasks"];

const TOKEN: &str = "glpat-test";

fn plugin() -> Command {
    Command::new(cargo_bin("kasl-plugin-gitlab"))
}

fn manifest() -> Value {
    let output = plugin().arg("--manifest").output().expect("the plugin should run");
    assert!(output.status.success(), "`--manifest` failed: {}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).expect("the manifest should be JSON")
}

/// Runs the plugin the way kasl does: the invocation in over stdin as one
/// line, the reply out over stdout.
fn invoke(call: &Value) -> Value {
    let mut child =
        plugin().arg("run").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("the plugin should run");
    let mut line = call.to_string();
    line.push('\n');
    child.stdin.take().unwrap().write_all(line.as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "`run` failed: {}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).expect("the reply should be one JSON object")
}

/// A request as kasl sends it to this server.
fn request(server: &ServerGuard, name: &str, extra: Value) -> Value {
    let mut call = json!({
        "protocol_version": HOST_PROTOCOL_VERSION,
        "request": name,
        "settings": { "url": format!("{}/", server.url()), "token": TOKEN },
    });
    if let (Value::Object(call), Value::Object(extra)) = (&mut call, extra) {
        call.extend(extra);
    }
    call
}

fn user(server: &mut ServerGuard) -> mockito::Mock {
    server
        .mock("GET", "/api/v4/user")
        .match_header("private-token", TOKEN)
        .with_body(r#"{"id": 7, "username": "ann", "name": "Ann Lee", "email": "ann@example.com"}"#)
        .create()
}

fn issue_json(full: &str, labels: &[&str]) -> Value {
    json!({
        "id": 9000, "iid": 14, "title": format!("About {full}"), "state": "opened",
        "web_url": format!("https://gitlab.example.com/{full}"), "labels": labels,
        "issue_type": "issue", "references": { "full": full }, "updated_at": "2026-11-02T08:00:00Z"
    })
}

fn merge_request_json(full: &str) -> Value {
    json!({
        "id": 9100, "iid": 31, "title": format!("About {full}"), "state": "opened", "draft": false,
        "web_url": format!("https://gitlab.example.com/{full}"), "labels": [],
        "references": { "full": full }, "updated_at": "2026-11-02T09:00:00Z"
    })
}

#[test]
fn the_manifest_is_one_kasl_reads_as_a_source() {
    let manifest = manifest();

    assert_eq!(manifest["protocol_version"], json!(HOST_PROTOCOL_VERSION));
    let requests: Vec<&str> = manifest["requests"].as_array().unwrap().iter().map(|r| r.as_str().unwrap()).collect();
    assert!(requests.contains(&"list_issues"), "transition and comment need list_issues beside them");
    for request in &requests {
        assert!(HOST_REQUESTS.contains(request), "kasl does not know the request `{request}`");
    }
    assert!(manifest.get("commands").is_none() && manifest.get("events").is_none());
}

#[test]
fn an_unknown_argument_is_refused_on_stderr() {
    let output = plugin().arg("serve").output().unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("expected --manifest or run"));
}

#[test]
fn list_issues_reads_every_page_of_issues_and_merge_requests() {
    let mut server = Server::new();
    let _user = user(&mut server);
    let _first = server
        .mock("GET", "/api/v4/issues")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("scope".into(), "assigned_to_me".into()),
            Matcher::UrlEncoded("state".into(), "opened".into()),
            Matcher::UrlEncoded("page".into(), "1".into()),
        ]))
        .with_header("x-next-page", "2")
        .with_body(json!([issue_json("group/app#14", &[])]).to_string())
        .create();
    let _second = server
        .mock("GET", "/api/v4/issues")
        .match_query(Matcher::UrlEncoded("page".into(), "2".into()))
        .with_header("x-next-page", "")
        .with_body(json!([issue_json("group/app#15", &["Doing"])]).to_string())
        .create();
    let _assigned = server
        .mock("GET", "/api/v4/merge_requests")
        .match_query(Matcher::UrlEncoded("scope".into(), "assigned_to_me".into()))
        .with_body(json!([merge_request_json("group/app!31")]).to_string())
        .create();
    let _reviews = server
        .mock("GET", "/api/v4/merge_requests")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("scope".into(), "all".into()),
            Matcher::UrlEncoded("reviewer_id".into(), "7".into()),
        ]))
        .with_body(json!([merge_request_json("group/app!31"), merge_request_json("group/lib!2")]).to_string())
        .create();

    let mut call = request(&server, "list_issues", json!({}));
    call["settings"]["start_label"] = json!("Doing");
    let reply = invoke(&call);

    assert!(reply.get("error").is_none(), "{reply}");
    let keys: Vec<&str> = reply["issues"].as_array().unwrap().iter().map(|i| i["key"].as_str().unwrap()).collect();
    assert_eq!(keys, ["group/app#14", "group/app#15", "group/app!31", "group/lib!2"]);
    assert_eq!(reply["issues"][1]["status"], "Doing");
    assert_eq!(reply["issues"][3]["type"], "Review");
    assert!(reply["list_url"].as_str().unwrap().ends_with("/dashboard/issues?assignee_username=ann"));
    for issue in reply["issues"].as_array().unwrap() {
        for field in ["key", "summary", "url"] {
            assert!(issue[field].as_str().is_some_and(|value| !value.is_empty()), "kasl requires `{field}`: {issue}");
        }
    }
}

#[test]
fn nothing_assigned_is_an_empty_list_not_silence() {
    let mut server = Server::new();
    let _user = user(&mut server);
    let _issues = server.mock("GET", "/api/v4/issues").match_query(Matcher::Any).with_body("[]").create();
    let _merge_requests = server.mock("GET", "/api/v4/merge_requests").match_query(Matcher::Any).with_body("[]").create();

    let reply = invoke(&request(&server, "list_issues", json!({})));

    assert_eq!(reply["issues"], json!([]), "kasl refuses a listing without `issues`");
}

#[test]
fn a_refused_token_is_an_error_that_says_what_to_do() {
    let mut server = Server::new();
    let _user = server.mock("GET", "/api/v4/user").with_status(401).with_body(r#"{"message":"401 Unauthorized"}"#).create();

    let reply = invoke(&request(&server, "list_issues", json!({})));

    let error = reply["error"].as_str().expect("a failure is the reply's error");
    assert!(error.contains("did not accept the token") && error.contains("kasl plugin setup gitlab"), "{error}");
    assert!(reply.get("issues").is_none(), "a failed listing must not look like an empty one");
}

#[test]
fn a_listing_cut_short_by_an_error_is_not_answered_as_a_list() {
    let mut server = Server::new();
    let _user = user(&mut server);
    let _first = server
        .mock("GET", "/api/v4/issues")
        .match_query(Matcher::UrlEncoded("page".into(), "1".into()))
        .with_header("x-next-page", "2")
        .with_body(json!([issue_json("group/app#14", &[])]).to_string())
        .create();
    let _second =
        server.mock("GET", "/api/v4/issues").match_query(Matcher::UrlEncoded("page".into(), "2".into())).with_status(502).create();

    let reply = invoke(&request(&server, "list_issues", json!({})));

    assert!(reply["error"].as_str().unwrap().contains("502"), "{reply}");
    assert!(reply.get("issues").is_none(), "half a list would mark the rest gone");
}

/// Mocks that fail the test if GitLab is asked anything at all.
fn nothing_asked(server: &mut ServerGuard) -> Vec<mockito::Mock> {
    ["GET", "PUT", "POST"]
        .into_iter()
        .map(|method| server.mock(method, Matcher::Any).match_query(Matcher::Any).expect(0).create())
        .collect()
}

fn subject(key: &str) -> Value {
    json!({ "key": key, "summary": "Something", "source": "gitlab", "url": "https://gitlab.example.com/x" })
}

#[test]
fn start_adds_the_label_once_it_is_known_to_exist() {
    let mut server = Server::new();
    let _item =
        server.mock("GET", "/api/v4/projects/group%2Fapp/issues/14").with_body(r#"{"state": "opened", "labels": ["bug"]}"#).create();
    let _label = server
        .mock("GET", "/api/v4/projects/group%2Fapp/labels/Doing")
        .match_query(Matcher::Any)
        .with_body(r#"{"name": "Doing"}"#)
        .create();
    let edit = server
        .mock("PUT", "/api/v4/projects/group%2Fapp/issues/14")
        .match_body(Matcher::Json(json!({"add_labels": "Doing"})))
        .with_body(r#"{"state": "opened", "labels": ["bug", "Doing"]}"#)
        .expect(1)
        .create();

    let call = request(&server, "transition", json!({"subject": subject("group/app#14"), "to": "start", "status": "Doing"}));
    let reply = invoke(&call);

    assert_eq!(reply, json!({"status": "Doing", "moved": true}));
    edit.assert();
}

#[test]
fn start_on_work_already_carrying_the_label_does_not_write() {
    let mut server = Server::new();
    let _item = server
        .mock("GET", "/api/v4/projects/group%2Fapp/merge_requests/31")
        .with_body(r#"{"state": "opened", "labels": ["doing"]}"#)
        .create();
    let edit = server.mock("PUT", Matcher::Any).expect(0).create();

    let mut call = request(&server, "transition", json!({"subject": subject("group/app!31"), "to": "start"}));
    call["settings"]["start_label"] = json!("Doing");
    let reply = invoke(&call);

    assert_eq!(reply, json!({"status": "Doing", "moved": false}));
    edit.assert();
}

#[test]
fn start_with_a_label_the_project_lacks_writes_nothing() {
    let mut server = Server::new();
    let _item = server.mock("GET", "/api/v4/projects/group%2Fapp/issues/14").with_body(r#"{"state": "opened", "labels": []}"#).create();
    let _label = server
        .mock("GET", "/api/v4/projects/group%2Fapp/labels/Doign")
        .match_query(Matcher::Any)
        .with_status(404)
        .with_body(r#"{"message":"404 Label Not Found"}"#)
        .create();
    let edit = server.mock("PUT", Matcher::Any).expect(0).create();

    let call = request(&server, "transition", json!({"subject": subject("group/app#14"), "to": "start", "status": "Doign"}));
    let reply = invoke(&call);

    assert!(reply["error"].as_str().unwrap().contains("no label `Doign`"), "{reply}");
    edit.assert();
}

#[test]
fn start_without_a_label_says_where_to_name_one() {
    let server = Server::new();

    let reply = invoke(&request(&server, "transition", json!({"subject": subject("group/app#14"), "to": "start"})));

    let error = reply["error"].as_str().unwrap();
    assert!(error.contains("--to") && error.contains("start_label"), "{error}");
}

#[test]
fn close_closes_the_issue_and_then_leaves_the_comment() {
    let mut server = Server::new();
    let _item = server.mock("GET", "/api/v4/projects/group%2Fapp/issues/14").with_body(r#"{"state": "opened", "labels": []}"#).create();
    let close = server
        .mock("PUT", "/api/v4/projects/group%2Fapp/issues/14")
        .match_body(Matcher::Json(json!({"state_event": "close"})))
        .with_body(r#"{"state": "closed", "labels": []}"#)
        .expect(1)
        .create();
    let note = server
        .mock("POST", "/api/v4/projects/group%2Fapp/issues/14/notes")
        .match_body(Matcher::Json(json!({"body": "Fixed in !31"})))
        .with_status(201)
        .with_body(r#"{"id": 1}"#)
        .expect(1)
        .create();

    let call = request(
        &server,
        "transition",
        json!({"subject": subject("group/app#14"), "to": "close", "status": null, "resolution": null, "comment": "Fixed in !31"}),
    );
    let reply = invoke(&call);

    assert_eq!(reply, json!({"status": "Closed", "moved": true}));
    close.assert();
    note.assert();
}

#[test]
fn close_on_a_merge_request_is_refused_before_gitlab_is_asked() {
    let mut server = Server::new();
    let untouched = nothing_asked(&mut server);

    let reply = invoke(&request(&server, "transition", json!({"subject": subject("group/app!31"), "to": "close"})));

    assert!(reply["error"].as_str().unwrap().contains("merging it"), "{reply}");
    untouched.iter().for_each(mockito::Mock::assert);
}

#[test]
fn close_with_a_resolution_is_refused_rather_than_dropped() {
    let mut server = Server::new();
    let untouched = nothing_asked(&mut server);

    let call = request(&server, "transition", json!({"subject": subject("group/app#14"), "to": "close", "resolution": "Fixed"}));
    let reply = invoke(&call);

    assert!(reply["error"].as_str().unwrap().contains("no resolution"), "{reply}");
    untouched.iter().for_each(mockito::Mock::assert);
}

#[test]
fn comment_leaves_the_text_on_a_merge_request() {
    let mut server = Server::new();
    let note = server
        .mock("POST", "/api/v4/projects/group%2Fsub%2Fapp/merge_requests/31/notes")
        .match_body(Matcher::Json(json!({"body": "Looks good"})))
        .with_status(201)
        .with_body(r#"{"id": 2}"#)
        .expect(1)
        .create();

    let reply = invoke(&request(&server, "comment", json!({"subject": subject("group/sub/app!31"), "comment": "Looks good"})));

    assert_eq!(reply["message"], "Commented on group/sub/app!31");
    note.assert();
}

#[test]
fn find_tasks_opens_each_push_and_keeps_the_persons_commits_of_the_day() {
    let mut server = Server::new();
    let _user = user(&mut server);
    let _events = server
        .mock("GET", "/api/v4/users/7/events")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("action".into(), "pushed".into()),
            Matcher::UrlEncoded("after".into(), "2026-11-01".into()),
            Matcher::UrlEncoded("before".into(), "2026-11-03".into()),
        ]))
        .with_body(
            json!([
                {"action_name": "pushed to", "project_id": 5,
                 "push_data": {"commit_count": 3, "commit_from": "f0", "commit_to": "c3"}},
                {"action_name": "pushed new", "project_id": 6,
                 "push_data": {"commit_count": 1, "commit_from": null, "commit_to": "d1"}},
                {"action_name": "pushed to", "project_id": 8,
                 "push_data": {"commit_count": 1, "commit_from": "e0", "commit_to": "e1"}}
            ])
            .to_string(),
        )
        .create();
    // Noon UTC is the same date in every zone between -11 and +11.
    let at = "2026-11-02T12:00:00Z";
    let _range = server
        .mock("GET", "/api/v4/projects/5/repository/compare")
        .match_query(Matcher::AllOf(vec![Matcher::UrlEncoded("from".into(), "f0".into()), Matcher::UrlEncoded("to".into(), "c3".into())]))
        .with_body(
            json!({"commits": [
                {"id": "c1aaaaaaaa", "message": "Fix the login\n\nbody", "author_email": "ann@example.com", "authored_date": at},
                {"id": "c2", "message": "Their change", "author_email": "bob@example.com", "author_name": "Bob", "authored_date": at},
                {"id": "c3", "message": "Last week's work", "author_email": "ann@example.com", "authored_date": "2026-10-26T12:00:00Z"}
            ]})
            .to_string(),
        )
        .create();
    let _tip = server
        .mock("GET", "/api/v4/projects/6/repository/commits/d1")
        .with_body(json!({"id": "d1bbbbbbbb", "message": "Start the branch", "author_name": "Ann Lee", "authored_date": at}).to_string())
        .create();
    let _gone = server
        .mock("GET", "/api/v4/projects/8/repository/commits/e1")
        .with_status(404)
        .with_body(r#"{"message":"404 Project Not Found"}"#)
        .create();

    let reply = invoke(&request(&server, "find_tasks", json!({"subject": {"date": "2026-11-02", "work_seconds": 3600}})));

    assert_eq!(reply["tasks"], json!([{"name": "Fix the login", "ref": "c1aaaaa"}, {"name": "Start the branch", "ref": "d1bbbbb"}]));
    let message = reply["message"].as_str().expect("an unread push is said, not skipped");
    assert!(message.starts_with("1 push could not be read"), "{message}");
}
