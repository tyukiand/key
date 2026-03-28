use anyhow::{anyhow, Result};

/// Check if an XML document contains an element at the given slash-separated path.
/// Path example: "settings/servers/server/token"
pub fn xml_has_path(content: &str, path: &str) -> Result<bool> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return Ok(true);
    }

    let mut reader = Reader::from_str(content);
    let mut depth_stack: Vec<String> = Vec::new();
    let mut max_matched = 0;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                depth_stack.push(name.clone());

                // Check if current stack matches the path prefix
                if depth_stack.len() <= segments.len() {
                    let matches = depth_stack.iter().zip(segments.iter()).all(|(a, b)| a == b);
                    if matches {
                        max_matched = max_matched.max(depth_stack.len());
                        if depth_stack.len() == segments.len() {
                            return Ok(true);
                        }
                    }
                }
            }
            Ok(Event::End(_)) => {
                depth_stack.pop();
            }
            Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                depth_stack.push(name);
                if depth_stack.len() <= segments.len() {
                    let matches = depth_stack.iter().zip(segments.iter()).all(|(a, b)| a == b);
                    if matches && depth_stack.len() == segments.len() {
                        return Ok(true);
                    }
                }
                depth_stack.pop();
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(anyhow!("XML parse error: {}", e)),
            _ => {}
        }
    }

    Ok(false)
}

/// Navigate a serde_json::Value by a jq-style dot-path.
/// Supports: `.key`, `.key.sub`, `.arr[0]`, `.arr[0].name`
pub fn json_has_query(content: &str, query: &str) -> Result<bool> {
    let root: serde_json::Value =
        serde_json::from_str(content).map_err(|e| anyhow!("JSON parse error: {}", e))?;
    Ok(navigate_json(&root, query))
}

/// Navigate a serde_yaml::Value by the same dot-path syntax as JSON.
pub fn yaml_has_query(content: &str, query: &str) -> Result<bool> {
    let root: serde_yaml::Value =
        serde_yaml::from_str(content).map_err(|e| anyhow!("YAML parse error: {}", e))?;
    Ok(navigate_yaml(&root, query))
}

fn navigate_json(value: &serde_json::Value, query: &str) -> bool {
    let segments = parse_query_segments(query);
    let mut current = value;
    for seg in segments {
        match seg {
            Segment::Key(k) => match current.get(k) {
                Some(v) => current = v,
                None => return false,
            },
            Segment::Index(i) => match current.get(i) {
                Some(v) => current = v,
                None => return false,
            },
        }
    }
    true
}

fn navigate_yaml(value: &serde_yaml::Value, query: &str) -> bool {
    let segments = parse_query_segments(query);
    let mut current = value;
    for seg in segments {
        match seg {
            Segment::Key(k) => {
                if let Some(m) = current.as_mapping() {
                    match m.get(serde_yaml::Value::String(k.to_string())) {
                        Some(v) => current = v,
                        None => return false,
                    }
                } else {
                    return false;
                }
            }
            Segment::Index(i) => {
                if let Some(seq) = current.as_sequence() {
                    match seq.get(i) {
                        Some(v) => current = v,
                        None => return false,
                    }
                } else {
                    return false;
                }
            }
        }
    }
    true
}

enum Segment<'a> {
    Key(&'a str),
    Index(usize),
}

/// Parse a query like `.key.sub[0].name` into segments.
fn parse_query_segments(query: &str) -> Vec<Segment<'_>> {
    let q = query.strip_prefix('.').unwrap_or(query);
    if q.is_empty() {
        return vec![];
    }
    let mut segments = Vec::new();
    for part in q.split('.') {
        if part.is_empty() {
            continue;
        }
        if let Some(bracket_pos) = part.find('[') {
            let key = &part[..bracket_pos];
            if !key.is_empty() {
                segments.push(Segment::Key(key));
            }
            // Parse all [N] suffixes
            let rest = &part[bracket_pos..];
            let mut pos = 0;
            while pos < rest.len() {
                if rest[pos..].starts_with('[') {
                    if let Some(end) = rest[pos..].find(']') {
                        if let Ok(idx) = rest[pos + 1..pos + end].parse::<usize>() {
                            segments.push(Segment::Index(idx));
                        }
                        pos += end + 1;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
        } else {
            segments.push(Segment::Key(part));
        }
    }
    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_simple_path() {
        let xml = r#"<settings><servers><server><token>abc</token></server></servers></settings>"#;
        assert!(xml_has_path(xml, "settings/servers/server/token").unwrap());
        assert!(!xml_has_path(xml, "settings/servers/server/missing").unwrap());
        assert!(xml_has_path(xml, "settings").unwrap());
    }

    #[test]
    fn xml_empty_element() {
        let xml = r#"<root><empty/></root>"#;
        assert!(xml_has_path(xml, "root/empty").unwrap());
    }

    #[test]
    fn json_dot_path() {
        let json = r#"{"user": {"name": "alice", "tags": ["a", "b"]}}"#;
        assert!(json_has_query(json, ".user.name").unwrap());
        assert!(json_has_query(json, ".user.tags[0]").unwrap());
        assert!(json_has_query(json, ".user.tags[1]").unwrap());
        assert!(!json_has_query(json, ".user.tags[2]").unwrap());
        assert!(!json_has_query(json, ".user.missing").unwrap());
    }

    #[test]
    fn yaml_dot_path() {
        let yaml = "models:\n  - name: gpt4\n    version: 1\n  - name: claude\n";
        assert!(yaml_has_query(yaml, "models[0].name").unwrap());
        assert!(yaml_has_query(yaml, "models[1].name").unwrap());
        assert!(!yaml_has_query(yaml, "models[2].name").unwrap());
    }
}
