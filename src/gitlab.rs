//! GitLab's REST API, as much of it as the plugin asks: who the token
//! belongs to, what is assigned to them, one issue or merge request, its
//! labels, and the pushes of a day.

use reqwest::Method;
use reqwest::blocking::{Client, RequestBuilder, Response};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::time::Duration;

/// The most pages one listing may take. A listing is read whole or not at
/// all, because kasl marks gone whatever it does not list; so running out of
/// pages is a failure, not a shorter list.
const MOST_PAGES: u32 = 50;

/// How many items GitLab hands over per page; 100 is its ceiling.
const PER_PAGE: &str = "100";

/// A client for one instance and one token.
pub struct Gitlab {
    api: String,
    token: String,
    http: Client,
}

/// The account the token belongs to.
#[derive(Debug, Clone, Deserialize)]
pub struct User {
    pub id: u64,
    pub username: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    /// The address the account commits with, when it chose one.
    #[serde(default)]
    pub commit_email: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct References {
    pub full: String,
}

/// An issue as `/issues` lists it.
#[derive(Debug, Clone, Deserialize)]
pub struct Issue {
    pub id: u64,
    pub title: String,
    pub web_url: String,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub issue_type: Option<String>,
    pub references: References,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// A merge request as `/merge_requests` lists it.
#[derive(Debug, Clone, Deserialize)]
pub struct MergeRequest {
    pub id: u64,
    pub title: String,
    pub web_url: String,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub draft: bool,
    pub references: References,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// What a transition reads before it writes: an issue's or a merge request's
/// state and labels. Both answer with these fields.
#[derive(Debug, Clone, Deserialize)]
pub struct Item {
    pub state: String,
    #[serde(default)]
    pub labels: Vec<String>,
}

/// One entry of a user's activity.
#[derive(Debug, Clone, Deserialize)]
pub struct Event {
    #[serde(default)]
    pub action_name: String,
    #[serde(default)]
    pub project_id: Option<u64>,
    #[serde(default)]
    pub push_data: Option<PushData>,
}

/// What a push carried: its range and its size.
#[derive(Debug, Clone, Deserialize)]
pub struct PushData {
    #[serde(default)]
    pub commit_from: Option<String>,
    #[serde(default)]
    pub commit_to: Option<String>,
    #[serde(default)]
    pub commit_count: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Commit {
    pub id: String,
    pub message: String,
    #[serde(default)]
    pub author_email: Option<String>,
    #[serde(default)]
    pub author_name: Option<String>,
    #[serde(default)]
    pub authored_date: Option<String>,
    #[serde(default)]
    pub committed_date: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Comparison {
    commits: Vec<Commit>,
}

impl Gitlab {
    pub fn new(root: &str, token: &str) -> Result<Self, String> {
        let http = Client::builder()
            .user_agent(concat!("kasl-plugin-gitlab/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            // kasl stops a plugin at two minutes; one slow request must not
            // spend all of them.
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| format!("could not start an HTTP client: {}", chain(&error)))?;
        Ok(Self { api: format!("{root}/api/v4"), token: token.to_string(), http })
    }

    /// The account the token belongs to.
    pub fn user(&self) -> Result<User, String> {
        self.get("/user", &[], "the token's account")
    }

    /// Open issues assigned to the token's account, every page.
    pub fn assigned_issues(&self) -> Result<Vec<Issue>, String> {
        self.get_all("/issues", &[("scope", "assigned_to_me".into()), ("state", "opened".into())], "assigned issues")
    }

    /// Open merge requests assigned to the token's account, every page.
    pub fn assigned_merge_requests(&self) -> Result<Vec<MergeRequest>, String> {
        self.get_all("/merge_requests", &[("scope", "assigned_to_me".into()), ("state", "opened".into())], "assigned merge requests")
    }

    /// Open merge requests the account is asked to review, every page.
    pub fn review_requests(&self, user: &User) -> Result<Vec<MergeRequest>, String> {
        self.get_all(
            "/merge_requests",
            &[("scope", "all".into()), ("state", "opened".into()), ("reviewer_id", user.id.to_string())],
            "review requests",
        )
    }

    /// An issue's or a merge request's state and labels, read by its API path.
    pub fn item(&self, path: &str, what: &str) -> Result<Item, String> {
        self.get(path, &[], what)
    }

    /// Whether the project has a label by this name, its groups' labels included.
    pub fn has_label(&self, project_path: &str, label: &str) -> Result<bool, String> {
        let path = format!("{project_path}/labels/{}", crate::reference::encode(label));
        let response = self.send(self.request(Method::GET, &path).query(&[("include_ancestor_groups", "true")]), "the label")?;
        match response.status().as_u16() {
            404 => Ok(false),
            _ => checked(response, &format!("the label `{label}`")).map(|_| true),
        }
    }

    /// Changes an issue or a merge request: `{"add_labels": ...}`, `{"state_event": "close"}`.
    pub fn edit(&self, path: &str, change: &Value, what: &str) -> Result<Item, String> {
        let response = self.send(self.request(Method::PUT, path).json(change), what)?;
        read(checked(response, what)?, what)
    }

    /// Leaves a comment on an issue or a merge request.
    pub fn comment(&self, path: &str, body: &str, what: &str) -> Result<(), String> {
        let notes = format!("{path}/notes");
        let response = self.send(self.request(Method::POST, &notes).json(&serde_json::json!({ "body": body })), what)?;
        checked(response, what).map(|_| ())
    }

    /// The account's pushes between two days, exclusive at both ends, every page.
    pub fn pushes(&self, user: &User, after: &str, before: &str) -> Result<Vec<Event>, String> {
        self.get_all(
            &format!("/users/{}/events", user.id),
            &[("action", "pushed".into()), ("after", after.into()), ("before", before.into())],
            "the day's pushes",
        )
    }

    /// The commits in `(from, to]`.
    pub fn compare(&self, project_id: u64, from: &str, to: &str) -> Result<Vec<Commit>, String> {
        let comparison: Comparison = self.get(
            &format!("/projects/{project_id}/repository/compare"),
            &[("from", from.into()), ("to", to.into())],
            "the pushed commits",
        )?;
        Ok(comparison.commits)
    }

    /// One commit, whole.
    pub fn commit(&self, project_id: u64, sha: &str) -> Result<Commit, String> {
        self.get(&format!("/projects/{project_id}/repository/commits/{sha}"), &[], "the pushed commit")
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        self.http.request(method, format!("{}{path}", self.api)).header("PRIVATE-TOKEN", &self.token)
    }

    fn send(&self, request: RequestBuilder, what: &str) -> Result<Response, String> {
        request.send().map_err(|error| format!("could not reach GitLab at {} for {what}: {}", self.root(), chain(&error)))
    }

    fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, String)], what: &str) -> Result<T, String> {
        let response = self.send(self.request(Method::GET, path).query(query), what)?;
        read(checked(response, what)?, what)
    }

    /// Every page of a listing, following GitLab's `x-next-page`.
    fn get_all<T: DeserializeOwned>(&self, path: &str, query: &[(&str, String)], what: &str) -> Result<Vec<T>, String> {
        let mut all = Vec::new();
        let mut page = "1".to_string();
        for _ in 0..MOST_PAGES {
            let request = self.request(Method::GET, path).query(query).query(&[("per_page", PER_PAGE), ("page", page.as_str())]);
            let response = checked(self.send(request, what)?, what)?;
            let next = response
                .headers()
                .get("x-next-page")
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            let batch: Vec<T> = read(response, what)?;
            all.extend(batch);
            match next {
                Some(next) => page = next,
                None => return Ok(all),
            }
        }
        Err(format!("GitLab had more than {MOST_PAGES} pages of {what}; kasl reads a list whole or not at all"))
    }

    fn root(&self) -> &str {
        self.api.strip_suffix("/api/v4").unwrap_or(&self.api)
    }
}

/// The response when it is a success, or what went wrong in the reader's words.
fn checked(response: Response, what: &str) -> Result<Response, String> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let said = response.text().ok().and_then(|body| said_by_gitlab(&body)).map(|message| format!(": {message}")).unwrap_or_default();
    Err(match status.as_u16() {
        401 => format!("GitLab did not accept the token (401){said}; give a new one with `kasl plugin setup gitlab`"),
        403 => format!("GitLab refused {what} (403){said}; the token needs the `api` scope to change issues, or `read_api` to read them"),
        404 => format!("GitLab has no {what} (404){said}"),
        code => format!("GitLab answered {code} for {what}{said}"),
    })
}

/// GitLab's own words from an error body: `{"message": ...}` or `{"error": ...}`.
fn said_by_gitlab(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    let said = value.get("message").or_else(|| value.get("error"))?;
    let text = match said {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    let text = text.trim();
    (!text.is_empty()).then(|| text.chars().take(200).collect())
}

fn read<T: DeserializeOwned>(response: Response, what: &str) -> Result<T, String> {
    response.json::<T>().map_err(|error| format!("GitLab's answer for {what} could not be read: {}", chain(&error)))
}

/// An error with its causes: reqwest's own message stops at "error sending
/// request", and the reason - a certificate, a refused connection - is below it.
fn chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let cause_text = cause.to_string();
        if !text.contains(&cause_text) {
            text.push_str(": ");
            text.push_str(&cause_text);
        }
        source = cause.source();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gitlabs_own_words_are_kept_and_cut() {
        assert_eq!(said_by_gitlab(r#"{"message":"401 Unauthorized"}"#).as_deref(), Some("401 Unauthorized"));
        assert_eq!(said_by_gitlab(r#"{"error":"insufficient_scope"}"#).as_deref(), Some("insufficient_scope"));
        assert_eq!(said_by_gitlab(r#"{"message":{"labels":["is invalid"]}}"#).as_deref(), Some(r#"{"labels":["is invalid"]}"#));
        assert_eq!(said_by_gitlab("<html>"), None);
        assert_eq!(said_by_gitlab(&format!(r#"{{"message":"{}"}}"#, "x".repeat(500))).unwrap().len(), 200);
    }
}
