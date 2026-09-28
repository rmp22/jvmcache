use crate::domain::JvmCacheError;
use std::fs;

pub fn expand_argfiles(args: &[String], depth: usize) -> Result<Vec<String>, JvmCacheError> {
    if depth > 10 {
        return Err(JvmCacheError::InvalidInvocation("Exceeded max recursive @argfile depth".into()));
    }
    let mut out = Vec::new();
    for arg in args {
        if let Some(file_path) = arg.strip_prefix('@') {
            let content = fs::read_to_string(file_path)?;
            let parsed_lines = parse_argfile_content(&content);
            let nested = expand_argfiles(&parsed_lines, depth + 1)?;
            out.extend(nested);
        } else {
            out.push(arg.clone());
        }
    }
    Ok(out)
}

fn parse_argfile_content(content: &str) -> Vec<String> {
    let (mut args, mut current, mut in_quote) = (Vec::new(), String::new(), None);
    let mut chars = content.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '#' && in_quote.is_none() && current.is_empty() {
            for next_c in chars.by_ref() {
                if next_c == '\n' {
                    break;
                }
            }
            continue;
        }

        match in_quote {
            Some(q) => {
                if c == q {
                    in_quote = None;
                } else if c == '\\' {
                    if matches!(chars.peek(), Some(&next_c) if next_c == q || next_c == '\\') {
                        current.push(chars.next().unwrap());
                        continue;
                    }
                    current.push(c);
                } else {
                    current.push(c);
                }
            }
            None => {
                if c == '"' || c == '\'' {
                    in_quote = Some(c);
                } else if c.is_whitespace() {
                    if !current.is_empty() {
                        args.push(current.clone());
                        current.clear();
                    }
                } else if c == '\\' {
                    if chars.peek().is_some() {
                        current.push(chars.next().unwrap());
                    } else {
                        current.push(c);
                    }
                } else {
                    current.push(c);
                }
            }
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}
