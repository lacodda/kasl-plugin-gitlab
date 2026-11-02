//! `find_tasks`: the person's commits of a day, offered to `kasl task find`.
//!
//! GitLab's activity feed records pushes, not commits, and a push of five
//! commits names only its range. Each push is opened into its commits, and
//! of those only the ones the person wrote on that day are kept - a push
//! today often carries yesterday's work, or a colleague's merged in.

use crate::gitlab::{Commit, Event, Gitlab, PushData, User};
use chrono::{DateTime, Days, Local, NaiveDate};
use serde_json::{Value, json};
use std::collections::HashSet;

/// What `find_tasks` answers for the day in `subject`.
pub fn find_tasks(gitlab: &Gitlab, subject: &Value) -> Result<Value, String> {
    let date = day_of(subject)?;
    let me = gitlab.user()?;
    // The feed's window is in GitLab's days, exclusive at both ends; a day
    // either side covers any time zone, and each commit's own moment decides.
    let after = date.checked_sub_days(Days::new(1)).unwrap_or(date).format("%Y-%m-%d").to_string();
    let before = date.checked_add_days(Days::new(1)).unwrap_or(date).format("%Y-%m-%d").to_string();
    let pushes = gitlab.pushes(&me, &after, &before)?;

    let mut commits = Vec::new();
    let mut unread = Vec::new();
    for push in pushes.iter().filter(|event| is_push(event)) {
        let (Some(project_id), Some(data)) = (push.project_id, &push.push_data) else {
            continue;
        };
        match commits_of(gitlab, project_id, data) {
            Ok(found) => commits.extend(found),
            Err(why) => unread.push(why),
        }
    }

    let mut reply = json!({ "tasks": tasks(&commits, &me, date) });
    // A push that could not be opened is said, not skipped in silence: a
    // missing suggestion would look like a day without that work.
    if let Some(first) = unread.first() {
        let count = unread.len();
        let pushes = if count == 1 { "push" } else { "pushes" };
        reply["message"] = json!(format!("{count} {pushes} could not be read, so their commits are not offered: {first}"));
    }
    Ok(reply)
}

/// The day a `find_tasks` subject names: `kasl report --json`'s `date`.
fn day_of(subject: &Value) -> Result<NaiveDate, String> {
    let raw = subject.get("date").and_then(Value::as_str).ok_or("kasl sent no day: the subject has no date")?;
    NaiveDate::parse_from_str(raw, "%Y-%m-%d").map_err(|_| format!("kasl sent a day this plugin cannot read: {raw}"))
}

fn is_push(event: &Event) -> bool {
    matches!(event.action_name.as_str(), "pushed to" | "pushed new")
}

/// Every commit a push carried.
///
/// One commit is its tip. More are the range GitLab compares; when the range
/// cannot be compared - a new branch has no `from` - the tip still is one.
fn commits_of(gitlab: &Gitlab, project_id: u64, push: &PushData) -> Result<Vec<Commit>, String> {
    let Some(to) = push.commit_to.as_deref() else {
        return Ok(Vec::new());
    };
    if push.commit_count.unwrap_or(1) > 1
        && let Some(from) = push.commit_from.as_deref()
        && !is_null_sha(from)
        && from != to
    {
        let range = gitlab.compare(project_id, from, to)?;
        if !range.is_empty() {
            return Ok(range);
        }
    }
    Ok(vec![gitlab.commit(project_id, to)?])
}

/// GitLab's all-zero SHA: the side of a push that had no commit before it.
fn is_null_sha(sha: &str) -> bool {
    !sha.is_empty() && sha.bytes().all(|byte| byte == b'0')
}

/// The offered tasks: one per commit by the person on the day, its first
/// line as the name and its short SHA as the reference.
pub fn tasks(commits: &[Commit], me: &User, date: NaiveDate) -> Vec<Value> {
    let mut seen = HashSet::new();
    commits
        .iter()
        .filter(|commit| written_by(commit, me) && written_on(commit, date))
        .filter(|commit| seen.insert(commit.id.clone()))
        .filter_map(|commit| {
            let name = commit.message.lines().next().unwrap_or_default().trim();
            (!name.is_empty()).then(|| json!({ "name": name, "ref": commit.id.chars().take(7).collect::<String>() }))
        })
        .collect()
}

/// By the person: their address first - stable however the name is spelt -
/// then their name.
fn written_by(commit: &Commit, me: &User) -> bool {
    let same = |theirs: &Option<String>, mine: &Option<String>| match (theirs, mine) {
        (Some(theirs), Some(mine)) => !mine.trim().is_empty() && theirs.trim().eq_ignore_ascii_case(mine.trim()),
        _ => false,
    };
    same(&commit.author_email, &me.email) || same(&commit.author_email, &me.commit_email) || same(&commit.author_name, &me.name)
}

/// On the day, in this machine's time zone - the one kasl's day is kept in.
/// The moment it was written, or committed when GitLab gave no other.
fn written_on(commit: &Commit, date: NaiveDate) -> bool {
    commit
        .authored_date
        .as_deref()
        .or(commit.committed_date.as_deref())
        .and_then(|raw| DateTime::parse_from_rfc3339(raw).ok())
        .is_some_and(|moment| moment.with_timezone(&Local).date_naive() == date)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn me() -> User {
        User {
            id: 7,
            username: "me".into(),
            name: Some("Ann Lee".into()),
            email: Some("ann@example.com".into()),
            commit_email: Some("ann@users.noreply.example.com".into()),
        }
    }

    /// A commit written at noon of `date` in this machine's zone.
    fn commit(sha: &str, message: &str, email: Option<&str>, name: Option<&str>, date: NaiveDate) -> Commit {
        let noon = date.and_hms_opt(12, 0, 0).unwrap().and_local_timezone(Local).unwrap();
        Commit {
            id: sha.into(),
            message: message.into(),
            author_email: email.map(str::to_string),
            author_name: name.map(str::to_string),
            authored_date: Some(noon.to_rfc3339()),
            committed_date: None,
        }
    }

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 2).unwrap()
    }

    #[test]
    fn the_persons_commits_of_the_day_are_offered_by_their_first_line() {
        let commits = [
            commit("a1b2c3d4e5", "Fix the login\n\nLonger body", Some("ANN@example.com"), None, day()),
            commit("b1", "Signed with the commit address", Some("ann@users.noreply.example.com"), None, day()),
            commit("c1", "By name only", None, Some("ann lee"), day()),
        ];

        let offered = tasks(&commits, &me(), day());

        assert_eq!(
            offered,
            vec![
                json!({"name": "Fix the login", "ref": "a1b2c3d"}),
                json!({"name": "Signed with the commit address", "ref": "b1"}),
                json!({"name": "By name only", "ref": "c1"}),
            ]
        );
    }

    #[test]
    fn a_colleagues_commit_and_yesterdays_are_left_out() {
        let yesterday = day().pred_opt().unwrap();
        let commits = [
            commit("a1", "Theirs", Some("bob@example.com"), Some("Bob"), day()),
            commit("b1", "Mine, but yesterday", Some("ann@example.com"), None, yesterday),
        ];

        assert!(tasks(&commits, &me(), day()).is_empty());
    }

    #[test]
    fn a_commit_pushed_twice_is_offered_once() {
        let twice = commit("a1", "Same", Some("ann@example.com"), None, day());

        assert_eq!(tasks(&[twice.clone(), twice], &me(), day()).len(), 1);
    }

    #[test]
    fn a_blank_identity_matches_nobody() {
        let nobody = User { email: Some(" ".into()), commit_email: None, name: None, ..me() };

        assert!(tasks(&[commit("a1", "x", Some(" "), None, day())], &nobody, day()).is_empty());
    }

    #[test]
    fn the_day_is_read_from_the_subject() {
        assert_eq!(day_of(&json!({"date": "2026-11-02", "work_seconds": 1})).unwrap(), day());
        assert!(day_of(&json!({})).unwrap_err().contains("no date"));
        assert!(day_of(&json!({"date": "02.11.2026"})).unwrap_err().contains("cannot read"));
    }

    #[test]
    fn only_a_null_sha_is_null() {
        assert!(is_null_sha("0000000000000000000000000000000000000000"));
        assert!(!is_null_sha("0000a"));
        assert!(!is_null_sha(""));
    }
}
