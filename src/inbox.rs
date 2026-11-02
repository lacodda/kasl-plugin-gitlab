//! The plugin as an inbox source: what is waiting on the person, and the two
//! things kasl may do to it - move it, and comment on it.

use crate::gitlab::{Gitlab, Issue, MergeRequest, User};
use crate::protocol::{Invocation, Settings};
use crate::reference::{Kind, Reference, encode};
use serde_json::{Value, json};
use std::collections::HashSet;

/// What `list_issues` answers: every open issue assigned to the person, every
/// open merge request assigned to them, and every one they are asked to review.
pub fn list_issues(gitlab: &Gitlab, settings: &Settings) -> Result<Value, String> {
    let me = gitlab.user()?;
    let issues = gitlab.assigned_issues()?;
    let assigned = gitlab.assigned_merge_requests()?;
    let reviewing = gitlab.review_requests(&me)?;
    Ok(listing(&settings.url, &me, &issues, &assigned, &reviewing, settings.start_label.as_deref()))
}

/// The reply to `list_issues`, from what GitLab answered.
///
/// A merge request both assigned and up for review is listed once, as
/// assigned: kasl refuses a listing that names a key twice, and being its
/// assignee is the larger part of the two.
pub fn listing(
    root: &str,
    me: &User,
    issues: &[Issue],
    assigned: &[MergeRequest],
    reviewing: &[MergeRequest],
    start_label: Option<&str>,
) -> Value {
    let mut seen = HashSet::new();
    let mut listed = Vec::new();
    for issue in issues {
        if seen.insert(issue.references.full.clone()) {
            listed.push(entry(
                &issue.references.full,
                &issue.title,
                &issue.web_url,
                issue.id,
                status(&issue.labels, false, start_label),
                &issue_type(issue.issue_type.as_deref()),
                issue.updated_at.as_deref(),
            ));
        }
    }
    for (merge_requests, kind) in [(assigned, "Merge request"), (reviewing, "Review")] {
        for merge_request in merge_requests {
            if seen.insert(merge_request.references.full.clone()) {
                listed.push(entry(
                    &merge_request.references.full,
                    &merge_request.title,
                    &merge_request.web_url,
                    merge_request.id,
                    status(&merge_request.labels, merge_request.draft, start_label),
                    kind,
                    merge_request.updated_at.as_deref(),
                ));
            }
        }
    }
    json!({ "list_url": list_url(root, me, issues, assigned, reviewing), "issues": listed })
}

fn entry(key: &str, summary: &str, url: &str, id: u64, status: String, kind: &str, updated: Option<&str>) -> Value {
    let mut entry = json!({
        "key": key,
        "summary": summary,
        "url": url,
        "id": id.to_string(),
        "status": status,
        "type": kind,
    });
    if let Some(project) = Reference::project_of(key) {
        entry["project"] = json!(project);
    }
    if let Some(updated) = updated {
        entry["updated"] = json!(updated);
    }
    entry
}

/// The status kasl shows for open work.
///
/// GitLab has no started state of its own; teams mark it with a label. When
/// the person named theirs, carrying it is the status - so a rule on `status`
/// and `kasl inbox start` speak about the same thing. Otherwise a merge
/// request in draft says so, and the rest is open.
pub fn status(labels: &[String], draft: bool, start_label: Option<&str>) -> String {
    if let Some(label) = start_label
        && carries(labels, label)
    {
        return label.to_string();
    }
    if draft { "Draft".to_string() } else { "Open".to_string() }
}

/// Label names are unique in a project regardless of case.
fn carries(labels: &[String], label: &str) -> bool {
    labels.iter().any(|carried| carried.eq_ignore_ascii_case(label))
}

fn issue_type(raw: Option<&str>) -> String {
    match raw.unwrap_or("issue") {
        "issue" => "Issue".to_string(),
        "incident" => "Incident".to_string(),
        "task" => "Task".to_string(),
        "test_case" => "Test case".to_string(),
        other => {
            let mut letters = other.replace('_', " ").chars().collect::<Vec<_>>();
            if let Some(first) = letters.first_mut() {
                *first = first.to_ascii_uppercase();
            }
            letters.into_iter().collect()
        }
    }
}

/// The page a summary toast opens: GitLab's own list of the first kind that
/// has anything in it - issues, then assigned merge requests, then reviews.
fn list_url(root: &str, me: &User, issues: &[Issue], assigned: &[MergeRequest], reviewing: &[MergeRequest]) -> String {
    let user = encode(&me.username);
    if issues.is_empty() && assigned.is_empty() && !reviewing.is_empty() {
        format!("{root}/dashboard/merge_requests?reviewer_username={user}")
    } else if issues.is_empty() && !assigned.is_empty() {
        format!("{root}/dashboard/merge_requests?assignee_username={user}")
    } else {
        format!("{root}/dashboard/issues?assignee_username={user}")
    }
}

/// `transition`: start the work, or close it.
pub fn transition(gitlab: &Gitlab, settings: &Settings, call: &Invocation) -> Result<Value, String> {
    let reference = subject_reference(call)?;
    match Invocation::text(&call.to) {
        Some("start") => start(gitlab, settings, call, &reference),
        Some("close") => close(gitlab, call, &reference),
        Some(other) => Err(format!("kasl asked to move {reference} to `{other}`; this plugin knows start and close")),
        None => Err(format!("kasl asked to move {reference} without saying where")),
    }
}

/// Starting is carrying the label that says so: the one named with `--to`, or
/// the plugin's `start_label`.
///
/// Everything that can refuse is asked before the one write: the item is
/// open, and the label exists. GitLab would create a label it does not know
/// on the spot, so a typo in `--to` would leave a new label in the project
/// for the whole team - kasl names the label and stops instead.
fn start(gitlab: &Gitlab, settings: &Settings, call: &Invocation, reference: &Reference) -> Result<Value, String> {
    let label = Invocation::text(&call.status).or(settings.start_label.as_deref()).ok_or_else(|| {
        format!(
            "GitLab has no started state of its own: name the label that marks it with `--to`, or set `start_label` with `kasl plugin setup gitlab`; {reference} stays as it is"
        )
    })?;
    if label.contains(',') {
        return Err(format!("the label `{label}` has a comma, which GitLab's API reads as two labels"));
    }
    let path = reference.path();
    let item = gitlab.item(&path, &reference.to_string())?;
    if item.state != "opened" {
        return Err(format!("{reference} is {} in GitLab; kasl starts open work only", item.state));
    }
    if carries(&item.labels, label) {
        return Ok(json!({ "status": label, "moved": false }));
    }
    if !gitlab.has_label(&reference.project_path(), label)? {
        return Err(format!(
            "{} has no label `{label}`; kasl does not create labels - make it in GitLab, or name another with `--to`",
            reference.project
        ));
    }
    gitlab.edit(&path, &json!({ "add_labels": label }), &reference.to_string())?;
    Ok(json!({ "status": label, "moved": true }))
}

/// Closing is GitLab's one way: the issue is closed, and the comment, if any,
/// follows it.
///
/// A merge request is finished by merging it, which is not kasl's to do, and
/// closing it unmerged throws the work away - so it is refused. So are a
/// resolution and a target status: GitLab has neither, and taking them
/// silently would let the person think they were kept.
fn close(gitlab: &Gitlab, call: &Invocation, reference: &Reference) -> Result<Value, String> {
    if reference.kind == Kind::MergeRequest {
        return Err(format!(
            "{reference} is a merge request: it is finished by merging it in GitLab, and kasl does not close merge requests"
        ));
    }
    if let Some(resolution) = Invocation::text(&call.resolution) {
        return Err(format!("GitLab issues have no resolution, so `{resolution}` cannot be kept; leave out --resolution"));
    }
    if let Some(status) = Invocation::text(&call.status)
        && !status.eq_ignore_ascii_case("closed")
    {
        return Err(format!("GitLab closes an issue one way, not to `{status}`; leave out --to"));
    }
    let path = reference.path();
    let item = gitlab.item(&path, &reference.to_string())?;
    let moved = item.state != "closed";
    if moved {
        gitlab.edit(&path, &json!({ "state_event": "close" }), &reference.to_string())?;
    }
    if let Some(comment) = Invocation::text(&call.comment) {
        gitlab
            .comment(&path, comment, &reference.to_string())
            .map_err(|why| format!("{reference} is closed, but the comment was not left: {why}"))?;
    }
    Ok(json!({ "status": "Closed", "moved": moved }))
}

/// `comment`: leave the text on the issue or merge request.
pub fn comment(gitlab: &Gitlab, call: &Invocation) -> Result<Value, String> {
    let reference = subject_reference(call)?;
    let text = Invocation::text(&call.comment).ok_or_else(|| format!("kasl asked to comment on {reference} with nothing to say"))?;
    gitlab.comment(&reference.path(), text, &reference.to_string())?;
    Ok(json!({ "message": format!("Commented on {reference}") }))
}

fn subject_reference(call: &Invocation) -> Result<Reference, String> {
    let key = call.subject.get("key").and_then(Value::as_str).ok_or("kasl sent no issue: the subject has no key")?;
    Reference::parse(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gitlab::References;

    fn me() -> User {
        User { id: 7, username: "me".into(), name: Some("Me".into()), email: Some("me@example.com".into()), commit_email: None }
    }

    fn issue(full: &str, labels: &[&str]) -> Issue {
        Issue {
            id: 900,
            title: format!("About {full}"),
            web_url: format!("https://gitlab.example.com/{full}"),
            labels: labels.iter().map(|label| label.to_string()).collect(),
            issue_type: Some("issue".into()),
            references: References { full: full.into() },
            updated_at: Some("2026-11-02T08:00:00Z".into()),
        }
    }

    fn merge_request(full: &str, draft: bool) -> MergeRequest {
        MergeRequest {
            id: 901,
            title: format!("About {full}"),
            web_url: format!("https://gitlab.example.com/{full}"),
            labels: Vec::new(),
            draft,
            references: References { full: full.into() },
            updated_at: None,
        }
    }

    #[test]
    fn issues_merge_requests_and_reviews_are_one_list() {
        let reply = listing(
            "https://gitlab.example.com",
            &me(),
            &[issue("group/app#14", &[])],
            &[merge_request("group/app!31", false)],
            &[merge_request("group/lib!2", true)],
            None,
        );

        let issues = reply["issues"].as_array().unwrap();
        let keys: Vec<&str> = issues.iter().map(|issue| issue["key"].as_str().unwrap()).collect();
        assert_eq!(keys, ["group/app#14", "group/app!31", "group/lib!2"]);
        let types: Vec<&str> = issues.iter().map(|issue| issue["type"].as_str().unwrap()).collect();
        assert_eq!(types, ["Issue", "Merge request", "Review"]);
        assert_eq!(issues[2]["status"], "Draft");
        assert_eq!(issues[0]["project"], "group/app");
        assert_eq!(issues[0]["id"], "900");
        assert_eq!(issues[0]["updated"], "2026-11-02T08:00:00Z");
        assert!(issues[1].get("updated").is_none(), "an absent field is left out, not null");
    }

    #[test]
    fn a_merge_request_assigned_and_up_for_review_is_listed_once_as_assigned() {
        let both = merge_request("group/app!31", false);

        let reply = listing("https://g", &me(), &[], std::slice::from_ref(&both), std::slice::from_ref(&both), None);

        let issues = reply["issues"].as_array().unwrap();
        assert_eq!(issues.len(), 1, "kasl refuses a listing with a key twice");
        assert_eq!(issues[0]["type"], "Merge request");
    }

    #[test]
    fn the_start_label_is_the_status_of_work_that_carries_it() {
        let labels = vec!["bug".to_string(), "doing".to_string()];

        assert_eq!(status(&labels, false, Some("Doing")), "Doing", "label names ignore case");
        assert_eq!(status(&labels, true, Some("Review")), "Draft");
        assert_eq!(status(&labels, false, None), "Open");
    }

    #[test]
    fn the_list_page_is_the_first_kind_with_anything_in_it() {
        let root = "https://g";
        let some_issue = [issue("g/a#1", &[])];
        let some_merge_request = [merge_request("g/a!1", false)];

        assert!(list_url(root, &me(), &some_issue, &some_merge_request, &[]).ends_with("/dashboard/issues?assignee_username=me"));
        assert!(list_url(root, &me(), &[], &some_merge_request, &[]).ends_with("/dashboard/merge_requests?assignee_username=me"));
        assert!(list_url(root, &me(), &[], &[], &some_merge_request).ends_with("/dashboard/merge_requests?reviewer_username=me"));
        assert!(list_url(root, &me(), &[], &[], &[]).ends_with("/dashboard/issues?assignee_username=me"));
    }

    #[test]
    fn an_issue_type_reads_as_gitlab_names_it() {
        assert_eq!(issue_type(Some("incident")), "Incident");
        assert_eq!(issue_type(Some("test_case")), "Test case");
        assert_eq!(issue_type(Some("objective_key")), "Objective key");
        assert_eq!(issue_type(None), "Issue");
    }
}
