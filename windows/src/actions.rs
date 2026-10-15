//! Smart actions: spot what a piece of text is (a sign-in code, a link, an
//! address, an email address, a phone number) so the notification can offer
//! the right buttons and the clipboard gets the useful part.

#[derive(Debug, Default, PartialEq)]
pub struct Found {
    pub link: Option<String>,
    /// A one-time sign-in code, digits only.
    pub code: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
}

pub fn find(text: &str) -> Found {
    let link = crate::winutil::find_link(text).map(str::to_string);
    // Look for everything else outside the link, so digits and @ signs in a
    // URL don't count.
    let rest = match &link {
        Some(l) => text.replacen(l.as_str(), " ", 1),
        None => text.to_string(),
    };
    Found {
        code: find_code(&rest),
        email: find_email(&rest),
        phone: find_phone(&rest),
        address: find_address(&rest),
        link,
    }
}

const CODE_WORDS: &[&str] = &[
    "code",
    "otp",
    "verif",
    "passcode",
    "pin",
    "2fa",
    "one-time",
    "one time",
    "login",
    "log in",
    "sign in",
    "sign-in",
    "security",
    "authenticat",
    "confirm",
];

fn find_code(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    let trimmed = text.trim();
    let whole_is_code =
        (4..=8).contains(&trimmed.len()) && trimmed.chars().all(|c| c.is_ascii_digit());
    if whole_is_code {
        return Some(trimmed.to_string());
    }
    if !CODE_WORDS.iter().any(|w| lower.contains(w)) {
        return None;
    }
    let words: Vec<&str> = text
        .split(|c: char| {
            c.is_whitespace() || matches!(c, ',' | ':' | ';' | '(' | ')' | '"' | '!' | '?')
        })
        .filter(|w| !w.is_empty())
        .collect();
    for (i, raw) in words.iter().enumerate() {
        let word = raw.trim_end_matches('.');
        // "123 456" written as two halves
        if word.len() == 3
            && word.chars().all(|c| c.is_ascii_digit())
            && let Some(next) = words.get(i + 1).map(|w| w.trim_end_matches('.'))
            && next.len() == 3
            && next.chars().all(|c| c.is_ascii_digit())
        {
            return Some(format!("{word}{next}"));
        }
        // "G-482913", "482-913", "482913"
        let without_prefix = match word.split_once('-') {
            Some((p, d)) if p.len() <= 2 && p.chars().all(|c| c.is_ascii_alphabetic()) => d,
            _ => word,
        };
        let digits: String = without_prefix.chars().filter(|c| *c != '-').collect();
        let dashes = without_prefix.matches('-').count();
        if (4..=8).contains(&digits.len())
            && digits.chars().all(|c| c.is_ascii_digit())
            && dashes <= 1
        {
            return Some(digits);
        }
    }
    None
}

fn find_email(text: &str) -> Option<String> {
    text.split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '<' | '>' | '(' | ')' | '"'))
        .map(|w| w.trim_matches(|c: char| matches!(c, '.' | ':' | '\'')))
        .find(|w| {
            let Some((user, domain)) = w.split_once('@') else {
                return false;
            };
            !user.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && !domain.contains('@')
        })
        .map(str::to_string)
}

fn find_phone(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let allowed = |c: char| c.is_ascii_digit() || matches!(c, ' ' | '-' | '.' | '(' | ')' | '+');
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() || chars[i] == '+' || chars[i] == '(' {
            let start = i;
            while i < chars.len() && allowed(chars[i]) {
                i += 1;
            }
            let candidate: String = chars[start..i].iter().collect();
            let candidate = candidate
                .trim()
                .trim_end_matches(['.', '-', '('])
                .to_string();
            let digits = candidate.chars().filter(char::is_ascii_digit).count();
            let has_separators = candidate.contains([' ', '-', '(', '.', '+']);
            if (10..=15).contains(&digits) && (has_separators || digits >= 10) {
                return Some(candidate);
            }
        }
        i += 1;
    }
    None
}

const STREET_WORDS: &[&str] = &[
    "st",
    "street",
    "ave",
    "avenue",
    "rd",
    "road",
    "blvd",
    "boulevard",
    "dr",
    "drive",
    "ln",
    "lane",
    "way",
    "ct",
    "court",
    "pl",
    "place",
    "pkwy",
    "parkway",
    "hwy",
    "highway",
    "cir",
    "circle",
    "ter",
    "terrace",
    "sq",
    "square",
];

fn find_address(text: &str) -> Option<String> {
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        for (i, word) in words.iter().enumerate() {
            let number = word.trim_end_matches(',');
            if !(1..=6).contains(&number.len()) || !number.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let street_end = words
                .iter()
                .enumerate()
                .skip(i + 2)
                .take(5)
                .find(|(_, w)| {
                    let w = w.trim_end_matches([',', '.']).to_lowercase();
                    STREET_WORDS.contains(&w.as_str())
                })
                .map(|(j, _)| j);
            if street_end.is_some() {
                let address = words[i..].join(" ");
                let address = address.trim_end_matches(['.', ',', '!', '?']);
                return Some(address.chars().take(120).collect());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_sign_in_codes() {
        assert_eq!(
            find("Your verification code is 482913").code.as_deref(),
            Some("482913")
        );
        assert_eq!(
            find("G-482913 is your Google verification code.")
                .code
                .as_deref(),
            Some("482913")
        );
        assert_eq!(
            find("Use 482 913 to sign in").code.as_deref(),
            Some("482913")
        );
        assert_eq!(
            find("Your Discord login code: 4829").code.as_deref(),
            Some("4829")
        );
        assert_eq!(find("482913").code.as_deref(), Some("482913"));
        // Not codes
        assert_eq!(find("See you at 2030 on Friday").code, None);
        assert_eq!(find("Meeting ID 123 4567 8901").code, None);
    }

    #[test]
    fn finds_links_emails_phones_and_addresses() {
        let f = find(
            "Join Zoom Meeting https://us02web.zoom.us/j/123456789?pwd=abc\nMeeting ID: 123 4567 8901",
        );
        assert_eq!(
            f.link.as_deref(),
            Some("https://us02web.zoom.us/j/123456789?pwd=abc")
        );
        assert_eq!(f.code, None);

        assert_eq!(
            find("email me at jane.doe@example.com.").email.as_deref(),
            Some("jane.doe@example.com")
        );
        assert_eq!(find("not an email: a@b").email, None);

        assert_eq!(
            find("call (555) 123-4567 tonight").phone.as_deref(),
            Some("(555) 123-4567")
        );
        assert_eq!(
            find("+1 555 123 4567").phone.as_deref(),
            Some("+1 555 123 4567")
        );
        assert_eq!(find("I owe you 25 bucks").phone, None);

        assert_eq!(
            find("Party at 1600 Pennsylvania Ave NW, Washington, DC 20500!")
                .address
                .as_deref(),
            Some("1600 Pennsylvania Ave NW, Washington, DC 20500")
        );
        assert_eq!(
            find("meet at 42 Main St.").address.as_deref(),
            Some("42 Main St")
        );
        assert_eq!(find("I have 3 dogs and they're the best").address, None);
    }
}
