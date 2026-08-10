pub fn comparison_key(path: &str) -> String {
    let bytes = path.as_bytes();
    let windows_drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    let windows_unc =
        bytes.len() >= 2 && matches!(bytes[0], b'/' | b'\\') && matches!(bytes[1], b'/' | b'\\');
    let windows = windows_drive || windows_unc;

    let replaced = path.replace('\\', "/");
    let collapsed = if windows_unc {
        format!("//{}", collapse_slashes(&replaced[2..]))
    } else {
        collapse_slashes(&replaced)
    };

    let (prefix, rest, protected_components) = if windows_unc {
        ("//".to_string(), collapsed[2..].to_string(), 2)
    } else if windows_drive {
        (collapsed[..3].to_string(), collapsed[3..].to_string(), 0)
    } else if let Some(rest) = collapsed.strip_prefix('/') {
        ("/".to_string(), rest.to_string(), 0)
    } else {
        (String::new(), collapsed, 0)
    };

    let mut components: Vec<&str> = Vec::new();
    for component in rest.split('/').filter(|component| !component.is_empty()) {
        match component {
            "." => {}
            ".." if components.len() > protected_components && components.last() != Some(&"..") => {
                components.pop();
            }
            ".." if prefix.is_empty() => components.push(component),
            ".." => {}
            _ => components.push(component),
        }
    }

    let mut normalized = format!("{prefix}{}", components.join("/"));
    if normalized.is_empty() {
        normalized.push('.');
    }
    if windows {
        normalized.make_ascii_lowercase();
    }
    normalized
}

fn collapse_slashes(path: &str) -> String {
    let mut result = String::with_capacity(path.len());
    let mut previous_slash = false;
    for character in path.chars() {
        if character == '/' {
            if !previous_slash {
                result.push(character);
            }
            previous_slash = true;
        } else {
            result.push(character);
            previous_slash = false;
        }
    }
    result
}
