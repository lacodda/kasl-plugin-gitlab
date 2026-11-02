//! The wire between kasl and this plugin.
//!
//! One invocation read from stdin, one reply written to stdout. Kept apart
//! from the plugin's own work so that the work is a function of a value
//! rather than of a process.

use serde::Deserialize;
use serde_json::{Map, Value};

/// The protocol version kasl speaks. A plugin declaring anything else is
/// listed by the host and refused, with the mismatch shown - so the number is
/// a promise, not a decoration.
pub const PROTOCOL_VERSION: u32 = 1;

/// What kasl sends for one call.
///
/// Exactly one of `request`, `command` and `event` says what kind of call it
/// is. This plugin declares requests only, so the other two are read just to
/// be refused by name rather than misread as a request.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Invocation {
    #[serde(default)]
    pub protocol_version: Option<u32>,
    #[serde(default)]
    pub request: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub event: Option<String>,
    /// The issue a `transition` or `comment` is about, or the day `find_tasks` looks at.
    #[serde(default)]
    pub subject: Value,
    /// Every declared setting that has a value; a setting never set is absent.
    #[serde(default)]
    pub settings: Map<String, Value>,
    /// `transition`: `start` or `close`.
    #[serde(default)]
    pub to: Option<String>,
    /// `transition`: what the person named with `--to`.
    #[serde(default)]
    pub status: Option<String>,
    /// `transition`: what the person named with `--resolution`.
    #[serde(default)]
    pub resolution: Option<String>,
    /// `transition` (the text of `close -m`) and `comment`.
    #[serde(default)]
    pub comment: Option<String>,
}

impl Invocation {
    /// Read one invocation from a reader, usually stdin.
    pub fn read(mut source: impl std::io::Read) -> Result<Self, String> {
        let mut input = String::new();
        source.read_to_string(&mut input).map_err(|error| format!("could not read the request: {error}"))?;
        serde_json::from_str(&input).map_err(|error| format!("could not read the request: {error}"))
    }

    /// The request this call makes, or why it is not one this plugin answers.
    pub fn request(&self) -> Result<&str, String> {
        if let Some(version) = self.protocol_version
            && version != PROTOCOL_VERSION
        {
            return Err(format!("kasl spoke protocol {version} and this plugin speaks {PROTOCOL_VERSION}"));
        }
        match (&self.request, &self.command, &self.event) {
            (Some(request), None, None) => Ok(request),
            (None, Some(command), None) => Err(format!("this plugin has no command `{command}`")),
            (None, None, Some(event)) => Err(format!("this plugin does not listen to `{event}`")),
            (None, None, None) => Err("the call names no request".to_string()),
            _ => Err("the call names more than one of request, command and event".to_string()),
        }
    }

    /// A text field of the invocation, trimmed, with blank read as absent.
    pub fn text(value: &Option<String>) -> Option<&str> {
        value.as_deref().map(str::trim).filter(|text| !text.is_empty())
    }
}

/// What the plugin needs to reach GitLab, read from the invocation's settings.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// The instance's root, without a trailing slash or `/api/v4`.
    pub url: String,
    pub token: String,
    /// The label that marks work started, when the person has one.
    pub start_label: Option<String>,
}

impl Settings {
    pub fn read(settings: &Map<String, Value>) -> Result<Self, String> {
        let text = |key: &str| settings.get(key).and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty());
        let url = text("url").ok_or("no GitLab address is set; run `kasl plugin setup gitlab`")?;
        let token = text("token").ok_or("no access token is set; run `kasl plugin setup gitlab`")?;
        Ok(Self { url: instance_root(url)?, token: token.to_string(), start_label: text("start_label").map(str::to_string) })
    }
}

/// The address as given, reduced to the instance's root.
///
/// People paste the address they see - with a trailing slash, or the API's own
/// `/api/v4` from a token page - and both would double up in every request
/// path. Anything that is not http(s) is refused rather than guessed at.
fn instance_root(url: &str) -> Result<String, String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(format!("the GitLab address `{url}` is not an http(s) address"));
    }
    let root = url.trim_end_matches('/');
    let root = root.strip_suffix("/api/v4").unwrap_or(root).trim_end_matches('/');
    Ok(root.to_string())
}

/// Write the reply to stdout.
pub fn emit(reply: &Value) {
    print!("{reply}");
}

/// Stop with a reason on stderr. The host shows it to the user.
pub fn fail(message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn invocation(raw: Value) -> Invocation {
        serde_json::from_value(raw).unwrap()
    }

    #[test]
    fn a_request_is_read_by_name() {
        let call = invocation(json!({"protocol_version": 1, "request": "list_issues", "settings": {}}));

        assert_eq!(call.request(), Ok("list_issues"));
    }

    #[test]
    fn a_command_or_an_event_is_refused_by_name() {
        let command = invocation(json!({"command": "mail", "subject": {}}));
        let event = invocation(json!({"event": "workday_end", "subject": {}}));

        assert!(command.request().unwrap_err().contains("no command `mail`"));
        assert!(event.request().unwrap_err().contains("does not listen to `workday_end`"));
    }

    #[test]
    fn another_protocol_is_refused_before_anything_is_read() {
        let call = invocation(json!({"protocol_version": 2, "request": "list_issues"}));

        assert!(call.request().unwrap_err().contains("protocol 2"));
    }

    #[test]
    fn a_call_without_a_kind_is_refused() {
        assert!(invocation(json!({})).request().unwrap_err().contains("no request"));
        assert!(invocation(json!({"request": "a", "event": "b"})).request().unwrap_err().contains("more than one"));
    }

    #[test]
    fn the_address_is_reduced_to_the_instance_root() {
        assert_eq!(instance_root("https://gitlab.example.com/").unwrap(), "https://gitlab.example.com");
        assert_eq!(instance_root("https://gitlab.example.com/api/v4/").unwrap(), "https://gitlab.example.com");
        assert_eq!(instance_root("http://git.local:8080/gitlab").unwrap(), "http://git.local:8080/gitlab");
        assert!(instance_root("gitlab.example.com").unwrap_err().contains("not an http(s) address"));
    }

    #[test]
    fn settings_need_an_address_and_a_token() {
        let mut settings = Map::new();
        assert!(Settings::read(&settings).unwrap_err().contains("no GitLab address"));
        settings.insert("url".into(), json!("https://gitlab.example.com"));
        assert!(Settings::read(&settings).unwrap_err().contains("no access token"));
        settings.insert("token".into(), json!("glpat-1"));
        settings.insert("start_label".into(), json!("  "));

        let read = Settings::read(&settings).unwrap();

        assert_eq!(read.token, "glpat-1");
        assert_eq!(read.start_label, None, "a blank label is no label");
    }

    #[test]
    fn nonsense_on_stdin_is_refused_rather_than_guessed() {
        assert!(Invocation::read(b"not json at all".as_slice()).is_err());
    }
}
