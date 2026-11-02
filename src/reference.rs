//! An issue's or merge request's key: GitLab's own full reference.
//!
//! `group/app#14` is an issue and `group/app!31` a merge request - the form
//! GitLab prints under every title and links when it is pasted into a
//! comment. Using it as kasl's key means the key alone says where the thing
//! lives, so a `transition` or a `comment` needs nothing kasl did not keep.

use std::fmt;

/// Which of the two kinds a reference names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Issue,
    MergeRequest,
}

impl Kind {
    fn sign(self) -> char {
        match self {
            Kind::Issue => '#',
            Kind::MergeRequest => '!',
        }
    }

    /// The segment of the API path the kind lives under.
    pub fn collection(self) -> &'static str {
        match self {
            Kind::Issue => "issues",
            Kind::MergeRequest => "merge_requests",
        }
    }
}

/// A parsed key: the project's path, the kind, and the number within the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub project: String,
    pub kind: Kind,
    pub iid: u64,
}

impl Reference {
    /// Reads a key back. A project path cannot hold `#` or `!`, so the last of
    /// either is where the path ends.
    pub fn parse(key: &str) -> Result<Self, String> {
        let unusable = || format!("`{key}` is not a GitLab reference such as group/app#14 or group/app!31");
        let at = key.rfind(['#', '!']).ok_or_else(unusable)?;
        let (project, rest) = key.split_at(at);
        let kind = if rest.starts_with('#') { Kind::Issue } else { Kind::MergeRequest };
        let iid = rest[1..].parse::<u64>().map_err(|_| unusable())?;
        if project.is_empty() || project.starts_with('/') || project.ends_with('/') {
            return Err(unusable());
        }
        Ok(Self { project: project.to_string(), kind, iid })
    }

    /// The project part of a full reference, for the inbox's `project` field.
    pub fn project_of(key: &str) -> Option<String> {
        Self::parse(key).ok().map(|reference| reference.project)
    }

    /// The API path of the thing itself: `/projects/group%2Fapp/issues/14`.
    pub fn path(&self) -> String {
        format!("/projects/{}/{}/{}", encode(&self.project), self.kind.collection(), self.iid)
    }

    /// The API path of the project it lives in.
    pub fn project_path(&self) -> String {
        format!("/projects/{}", encode(&self.project))
    }
}

impl fmt::Display for Reference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}{}", self.project, self.kind.sign(), self.iid)
    }
}

/// Percent-encodes one path segment: a project's path with its slashes, a
/// label's name with its spaces and `::`.
pub fn encode(segment: &str) -> String {
    let mut encoded = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => encoded.push(byte as char),
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_issue_and_a_merge_request_are_told_apart_by_their_sign() {
        let issue = Reference::parse("group/app#14").unwrap();
        let merge_request = Reference::parse("group/sub/app!31").unwrap();

        assert_eq!((issue.project.as_str(), issue.kind, issue.iid), ("group/app", Kind::Issue, 14));
        assert_eq!((merge_request.project.as_str(), merge_request.kind, merge_request.iid), ("group/sub/app", Kind::MergeRequest, 31));
    }

    #[test]
    fn a_reference_reads_back_as_it_was_written() {
        for key in ["group/app#14", "group/sub/app!31", "me/my.project_x#1"] {
            assert_eq!(Reference::parse(key).unwrap().to_string(), key);
        }
    }

    #[test]
    fn what_is_not_a_reference_is_refused() {
        for key in ["PROJ-12", "group/app#", "#14", "group/app#x", "group/app/#3", "group/app#-1"] {
            assert!(Reference::parse(key).is_err(), "{key}");
        }
    }

    #[test]
    fn the_api_path_encodes_the_project_whole() {
        let reference = Reference::parse("group/sub/app!31").unwrap();

        assert_eq!(reference.path(), "/projects/group%2Fsub%2Fapp/merge_requests/31");
        assert_eq!(reference.project_path(), "/projects/group%2Fsub%2Fapp");
    }

    #[test]
    fn a_label_is_encoded_as_one_segment() {
        assert_eq!(encode("workflow::in progress"), "workflow%3A%3Ain%20progress");
        assert_eq!(encode("Doing"), "Doing");
    }
}
