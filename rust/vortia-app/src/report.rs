//! A draft of a GitHub issue, written for the user to review before anything is sent.
//!
//! Opening the address this builds creates nothing: GitHub shows its new-issue form with
//! the title and the text filled in, and the user decides. A text too long for an address
//! stays on the clipboard instead of being cut.

/// Where new issues are written.
const NEW_ISSUE: &str = "https://github.com/getnodalia/vortia/issues/new";

/// The longest address handed to the system, as the Qt application kept it.
const MAX_URL: usize = 7000;

/// A draft ready to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    /// The address that opens GitHub's form.
    pub url: String,
    /// The text did not fit in the address, so the user has to paste it.
    pub body_on_clipboard: bool,
}

/// `text` for a query string: letters, digits and `-_.~` stay, everything else is `%XX`.
pub fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The address of the new-issue form with `title` and `body` filled in, or with the title
/// alone when both together would not fit.
pub fn draft(title: &str, body: &str) -> Draft {
    let full = format!(
        "{NEW_ISSUE}?title={}&body={}",
        percent_encode(title),
        percent_encode(body)
    );
    if full.len() <= MAX_URL {
        return Draft {
            url: full,
            body_on_clipboard: false,
        };
    }
    let title_only = format!("{NEW_ISSUE}?title={}", percent_encode(title));
    // A title that is itself enormous is cut, on a character boundary.
    let url = if title_only.len() <= MAX_URL {
        title_only
    } else {
        let short: String = title.chars().take(200).collect();
        format!("{NEW_ISSUE}?title={}", percent_encode(&short))
    };
    Draft {
        url,
        body_on_clipboard: !body.is_empty(),
    }
}

/// What the report says about the machine and the case, and nothing that names the user:
/// no path, no case name, no address.
pub fn environment(application: Option<&str>) -> String {
    let mut text = format!(
        "Vortia {}\nSystem: {} {}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    if let Some(application) = application {
        text.push_str(&format!("Solver of the open case: {application}\n"));
    }
    text
}

/// The text of the report: what the user wrote, then the facts about the machine.
pub fn body(description: &str, application: Option<&str>) -> String {
    let description = description.trim();
    let mut text = String::new();
    if !description.is_empty() {
        text.push_str(description);
        text.push_str("\n\n");
    }
    text.push_str("---\n");
    text.push_str(&environment(application));
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreserved_characters_stay_and_the_rest_is_escaped() {
        assert_eq!(percent_encode("a-Z_0.9~"), "a-Z_0.9~");
        assert_eq!(percent_encode("a b&c=d"), "a%20b%26c%3Dd");
        // Two bytes in UTF-8, each escaped.
        assert_eq!(percent_encode("é"), "%C3%A9");
        assert_eq!(percent_encode("\n"), "%0A");
    }

    #[test]
    fn a_short_report_travels_in_the_address() {
        let d = draft("Crash on open", "It closes.");
        assert_eq!(
            d.url,
            "https://github.com/getnodalia/vortia/issues/new?title=Crash%20on%20open&body=It%20closes."
        );
        assert!(!d.body_on_clipboard);
    }

    #[test]
    fn a_long_report_keeps_the_title_and_leaves_the_text_to_the_clipboard() {
        let d = draft("Slow", &"x".repeat(9000));
        assert_eq!(
            d.url,
            "https://github.com/getnodalia/vortia/issues/new?title=Slow"
        );
        assert!(d.body_on_clipboard);
    }

    #[test]
    fn an_enormous_title_is_cut_and_never_overflows_the_address() {
        let d = draft(&"é".repeat(5000), "");
        assert!(d.url.len() <= MAX_URL);
        assert!(!d.body_on_clipboard);
    }

    #[test]
    fn the_body_names_the_machine_and_nothing_private() {
        let text = body("  It stops.  ", Some("rhoSimpleFoam"));
        assert!(text.starts_with("It stops.\n\n---\nVortia "));
        assert!(text.contains(std::env::consts::OS));
        assert!(text.contains("rhoSimpleFoam"));
        let empty = body("", None);
        assert!(empty.starts_with("---\nVortia "));
        assert!(!empty.contains("Solver"));
    }
}
