//! History uses unresolved templates only. The user opted to retain literal hosts.
const REDACTED: &str = "[redacted]";
pub const INVALID_URL: &str = "[URL redacted]";

fn placeholder(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

fn token(input: &str) -> Option<usize> {
    let rest = input.strip_prefix("{{")?;
    let end = rest
        .as_bytes()
        .windows(2)
        .take(201)
        .position(|pair| pair == b"}}")?;
    placeholder(&rest[..end]).then_some(end + 4)
}

/// Mask every literal run, preserving only well-formed variable placeholders.
fn component(input: &str) -> String {
    let mut remaining = input;
    let mut output = String::new();
    let mut masked = false;
    while !remaining.is_empty() {
        if let Some(end) = token(remaining) {
            output.push_str(&remaining[..end]);
            remaining = &remaining[end..];
            masked = false;
        } else {
            if !masked {
                output.push_str(REDACTED);
                masked = true;
            }
            // Consume one UTF-8 character without ever copying literal data.
            let length = remaining.chars().next().map(char::len_utf8).unwrap_or(1);
            remaining = &remaining[length..];
        }
    }
    output
}

fn host(input: &str) -> Option<String> {
    let authority = input.rsplit('@').next()?;
    let (host, port) = if authority.starts_with('[') {
        let end = authority.find(']')? + 1;
        let rest = &authority[end..];
        if !rest.is_empty() && !rest.starts_with(':') {
            return None;
        }
        (&authority[..end], rest.strip_prefix(':'))
    } else {
        authority
            .split_once(':')
            .map(|(h, p)| (h, Some(p)))
            .unwrap_or((authority, None))
    };
    if host.is_empty() {
        return None;
    }
    let mut check = String::new();
    let mut rest = host;
    let mut has_variable = false;
    while !rest.is_empty() {
        if let Some(end) = token(rest) {
            check.push_str("variable");
            rest = &rest[end..];
            has_variable = true;
        } else {
            let character = rest.chars().next()?;
            if matches!(character, '{' | '}' | '/' | '?' | '#' | '@' | '\\')
                || character.is_control()
                || character.is_whitespace()
            {
                return None;
            }
            check.push(character);
            rest = &rest[character.len_utf8()..];
        }
    }
    let parsed = reqwest::Url::parse(&format!("https://{check}/")).ok()?;
    let canonical = parsed.host_str()?;
    if !parsed.username().is_empty() || parsed.password().is_some() || parsed.port().is_some() {
        return None;
    }
    let mut output = if has_variable {
        host.to_owned()
    } else {
        canonical.to_owned()
    };
    if let Some(port) = port {
        output.push(':');
        output.push_str(&component(port));
    }
    (output.len() <= 512).then_some(output)
}

/// No resolved URL is accepted here. Credentials/fragments are discarded; literal hosts
/// are deliberately retained, with every path/query literal masked (including query names).
pub fn url(input: &str) -> String {
    if input.len() > 65_536 || input.chars().any(char::is_control) || input.contains('\\') {
        return INVALID_URL.into();
    }
    let input = input.split('#').next().unwrap_or("");
    let (origin, rest) = if let Some(end) = token(input) {
        // A complete base URL variable is a template, never a credential read.
        (&input[..end], &input[end..])
    } else {
        let Some((scheme, remainder)) = input.split_once("://") else {
            return INVALID_URL.into();
        };
        if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
            return INVALID_URL.into();
        }
        let end = remainder.find(['/', '?']).unwrap_or(remainder.len());
        let Some(host) = host(&remainder[..end]) else {
            return INVALID_URL.into();
        };
        let origin = format!("{}://{host}", scheme.to_ascii_lowercase());
        return tail(origin, &remainder[end..]);
    };
    tail(origin.into(), rest)
}

fn tail(origin: String, input: &str) -> String {
    let mut output = origin.clone();
    let mut start = 0;
    for (index, character) in input.char_indices() {
        if matches!(character, '/' | '?' | '&' | '=') {
            output.push_str(&component(&input[start..index]));
            output.push(character);
            start = index + character.len_utf8();
        }
    }
    output.push_str(&component(&input[start..]));
    if output.len() > 2048 {
        format!("{origin}/{REDACTED}")
    } else {
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retains_only_opted_in_hosts_placeholders_and_structure() {
        assert_eq!(url("https://user:private@API.example.test:8443/users/private-id?api_key=secret&x={{token}}#private"), "https://api.example.test:[redacted]/[redacted]/[redacted]?[redacted]=[redacted]&[redacted]={{token}}");
        assert_eq!(
            url("{{baseUrl}}/accounts/{{tenant}}?q={{query}}"),
            "{{baseUrl}}/[redacted]/{{tenant}}?[redacted]={{query}}"
        );
        assert_eq!(
            url("https://{{region}}.example.test:{{port}}/{{id}}"),
            "https://{{region}}.example.test:{{port}}/{{id}}"
        );
        assert_eq!(
            url("http://[::1]:1234/秘密/%7B%7Bprivate%7D%7D"),
            "http://[::1]:[redacted]/[redacted]/[redacted]"
        );
    }
    #[test]
    fn fails_closed_and_is_idempotent_for_stored_records() {
        for input in [
            "https://a.test/private?q=token",
            "{{baseUrl}}/p/{{id}}",
            "https://{{host}}:{{port}}/p",
            "not a URL",
            "https://user@host.test:8443/a",
            "https://[::1]:1234/x",
        ] {
            let safe = url(input);
            assert_eq!(url(&safe), safe);
        }
        for input in [
            "file:///private",
            "https://bad{{host}}}/private",
            "https://a.test\\@private/path",
            "https://a.test/\nsecret",
            "private",
            "https://a.test:1234@/private",
        ] {
            assert_eq!(url(input), INVALID_URL);
        }
        let raw = format!("https://api.test/{}", "segment/".repeat(1000));
        assert_eq!(url(&raw), "https://api.test/[redacted]");
    }
}
