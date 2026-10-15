//! Tiny percent-encoding helpers for the links Hey builds.

pub fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    while i < bytes.len() {
        let escaped = match bytes.get(i..i + 3) {
            Some([b'%', hi, lo]) => hex(*hi).zip(hex(*lo)).map(|(h, l)| h * 16 + l),
            _ => None,
        };
        match (escaped, bytes[i]) {
            (Some(b), _) => {
                out.push(b);
                i += 3;
                continue;
            }
            (None, b'+') => out.push(b' '),
            (None, b) => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The value of `name` in `scheme:action?name=value&...`.
pub fn query_param(link: &str, name: &str) -> Option<String> {
    let query = link.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == name).then(|| decode(v))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for s in [
            "C:\\Users\\Jane Doe\\Downloads\\Hey\\a b.jpg",
            "123 456",
            "ünïcode & stuff=1%",
        ] {
            assert_eq!(decode(&encode(s)), s);
        }
        assert_eq!(
            query_param("hey-action:copy?text=12%2034", "text").as_deref(),
            Some("12 34")
        );
        assert_eq!(decode("100%"), "100%");
    }
}
