//! kasl-plugin-gitlab: GitLab for kasl, a work activity tracker for the
//! command line.
//!
//! Two calls, following the line's plugin convention:
//!
//!   kasl-plugin-gitlab --manifest   -> what this plugin offers, as JSON on stdout
//!   kasl-plugin-gitlab run          -> invocation as JSON on stdin, reply on stdout
//!
//! It answers four requests: `list_issues`, `transition` and `comment` make it
//! an inbox source, and `find_tasks` offers the day's commits to
//! `kasl task find`. It lives for the duration of one call and holds no state.

mod commits;
mod gitlab;
mod inbox;
mod protocol;
mod reference;

use protocol::{Invocation, PROTOCOL_VERSION, Settings};
use serde_json::{Value, json};

fn main() {
    let argument = std::env::args().nth(1).unwrap_or_default();

    match argument.as_str() {
        "--manifest" => print!("{}", manifest()),
        "run" => run(),
        other => {
            // A plugin that cannot be understood says so on stderr and exits
            // non-zero: the host shows the reason rather than hiding the
            // plugin, because a silently missing integration is
            // indistinguishable from one that was never installed.
            eprintln!("unknown argument `{other}`; expected --manifest or run");
            std::process::exit(2);
        }
    }
}

/// What this plugin tells kasl about itself.
fn manifest() -> Value {
    json!({
        "protocol_version": PROTOCOL_VERSION,
        "name": "gitlab",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "GitLab issues and merge requests waiting on you, and your commits of the day",
        "requests": ["list_issues", "transition", "comment", "find_tasks"],
        "settings": [
            {
                "key": "url",
                "label": "GitLab address",
                "description": "The instance's root, such as https://gitlab.com",
                "required": true
            },
            {
                "key": "token",
                "label": "Access token",
                "description": "A personal access token: the api scope lets kasl start, close and comment; read_api only reads",
                "secret": true,
                "required": true
            },
            {
                "key": "start_label",
                "label": "Label that marks work started",
                "description": "Added by `kasl inbox start`, and shown as the status of work that carries it; leave empty if your team has none"
            }
        ]
    })
}

fn run() {
    let invocation = match Invocation::read(std::io::stdin()) {
        Ok(invocation) => invocation,
        Err(message) => protocol::fail(&message),
    };
    protocol::emit(&answer(&invocation));
}

/// The reply to one invocation. A failure is the reply's `error`, which kasl
/// shows as the plugin's own words; the exit code stays for a plugin that
/// could not understand the call at all.
fn answer(invocation: &Invocation) -> Value {
    match handle(invocation) {
        Ok(reply) => reply,
        Err(error) => json!({ "error": error }),
    }
}

fn handle(invocation: &Invocation) -> Result<Value, String> {
    let request = invocation.request()?;
    let settings = Settings::read(&invocation.settings)?;
    let gitlab = gitlab::Gitlab::new(&settings.url, &settings.token)?;
    match request {
        "list_issues" => inbox::list_issues(&gitlab, &settings),
        "transition" => inbox::transition(&gitlab, &settings, invocation),
        "comment" => inbox::comment(&gitlab, invocation),
        "find_tasks" => commits::find_tasks(&gitlab, &invocation.subject),
        other => Err(format!("this plugin does not answer `{other}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_declares_the_protocol_and_what_it_answers() {
        let manifest = manifest();

        assert_eq!(manifest["protocol_version"], json!(PROTOCOL_VERSION));
        assert_eq!(manifest["name"], json!("gitlab"));
        assert_eq!(manifest["requests"], json!(["list_issues", "transition", "comment", "find_tasks"]));
    }

    #[test]
    fn the_manifest_reports_the_version_the_package_was_built_at() {
        // A hard-coded version would drift from Cargo.toml at the first
        // release and the host would show the wrong number for ever after.
        assert_eq!(manifest()["version"], json!(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn the_token_is_a_secret_and_the_label_is_optional() {
        let manifest = manifest();
        let settings = manifest["settings"].as_array().unwrap();
        let setting = |key: &str| settings.iter().find(|setting| setting["key"] == key).unwrap().clone();

        assert_eq!(setting("token")["secret"], json!(true), "a token never goes to kasl's config file");
        assert_eq!(setting("url")["required"], json!(true));
        assert!(setting("start_label").get("required").is_none());
    }

    #[test]
    fn a_request_without_settings_says_what_to_run() {
        let invocation: Invocation = serde_json::from_value(json!({"protocol_version": 1, "request": "list_issues"})).unwrap();

        let reply = answer(&invocation);

        assert!(reply["error"].as_str().unwrap().contains("kasl plugin setup gitlab"), "{reply}");
    }

    #[test]
    fn a_request_it_does_not_answer_is_named() {
        let invocation: Invocation = serde_json::from_value(json!({
            "request": "list_projects",
            "settings": {"url": "https://gitlab.example.com", "token": "t"}
        }))
        .unwrap();

        assert!(answer(&invocation)["error"].as_str().unwrap().contains("`list_projects`"));
    }
}
