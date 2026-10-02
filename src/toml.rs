//! 极简 TOML 子集解析器。
//!
//! 只支持本工具配置需要的语法，避免引入任何第三方依赖：
//!   - 段头      [general] / [profiles.default] / [vaults]
//!   - 字符串    key = "value"（支持 \" \\ \n \t 转义）
//!   - 布尔      key = true / false
//!   - 字符串数组 key = ["a", "b"]（可跨行）
//!   - 内联表    key = { path = "x", profile = "y" }
//!   - # 行注释
//!
//! `#` 在带引号的字符串内部不会被当作注释。

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum Value {
    Str(String),
    Bool(bool),
    Array(Vec<String>),
    Table(HashMap<String, Value>),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[String]> {
        match self {
            Value::Array(a) => Some(a.as_slice()),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn as_table(&self) -> Option<&HashMap<String, Value>> {
        match self {
            Value::Table(t) => Some(t),
            _ => None,
        }
    }
}

/// 解析结果：段路径 -> (键 -> 值)。段路径以 "." 连接，例如 "profiles.default"。
#[derive(Debug, Default)]
pub struct Doc {
    pub sections: HashMap<String, HashMap<String, Value>>,
}

impl Doc {
    pub fn section(&self, path: &str) -> Option<&HashMap<String, Value>> {
        self.sections.get(path)
    }

    #[cfg(test)]
    pub fn get(&self, section: &str, key: &str) -> Option<&Value> {
        self.sections.get(section).and_then(|s| s.get(key))
    }
}

pub fn parse(input: &str) -> Result<Doc, String> {
    let mut doc = Doc::default();
    let mut current = String::from("");

    // 先把「跨行的数组值」合并成逻辑行：数组允许写成
    //   dirs = [
    //     "a",
    //     "b",
    //   ]
    let mut logical: Vec<(usize, String)> = Vec::new();
    let mut pending: Option<(usize, String)> = None;
    for (idx, raw_line) in input.lines().enumerate() {
        let line_no = idx + 1;
        let stripped = strip_comment(raw_line);
        let trimmed = stripped.trim();

        match pending.take() {
            Some((start, mut acc)) => {
                acc.push(' ');
                acc.push_str(trimmed);
                if brackets_balanced(&acc) {
                    logical.push((start, acc));
                } else {
                    pending = Some((start, acc));
                }
            }
            None => {
                if trimmed.is_empty() {
                    continue;
                }
                // 段头一定自成一行，不能是跨行数组的续行
                if trimmed.starts_with('[') && trimmed.ends_with(']') && !trimmed.contains('=') {
                    logical.push((line_no, trimmed.to_string()));
                    continue;
                }
                if !brackets_balanced(trimmed) {
                    pending = Some((line_no, trimmed.to_string()));
                } else {
                    logical.push((line_no, trimmed.to_string()));
                }
            }
        }
    }
    if let Some((start, _)) = pending {
        return Err(format!("第 {start} 行：数组或内联表缺少结尾的 ']' / '}}'"));
    }

    for (line_no, line) in logical {
        if line.starts_with('[') {
            if !line.ends_with(']') {
                return Err(format!("第 {line_no} 行：段头缺少结尾的 ']'"));
            }
            let name = line[1..line.len() - 1].trim().to_string();
            if name.is_empty() {
                return Err(format!("第 {line_no} 行：段名为空"));
            }
            current = name;
            doc.sections.entry(current.clone()).or_default();
            continue;
        }

        let eq = find_top_level_eq(&line)
            .ok_or_else(|| format!("第 {line_no} 行：缺少 '=' 分隔符 -> {line}"))?;
        let key = unquote_key(line[..eq].trim(), line_no)?;
        let rest = line[eq + 1..].trim().to_string();
        if key.is_empty() {
            return Err(format!("第 {line_no} 行：键名为空"));
        }

        let value = parse_value(&rest, line_no)?;
        doc.sections
            .entry(current.clone())
            .or_default()
            .insert(key, value);
    }

    Ok(doc)
}

/// 引号外的 `[` 与 `]`、`{` 与 `}` 是否配平（用于识别跨行值）。
fn brackets_balanced(text: &str) -> bool {
    let mut square = 0i32;
    let mut curly = 0i32;
    let mut in_str = false;
    let mut escaped = false;
    for ch in text.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_str => escaped = true,
            '"' => in_str = !in_str,
            '[' if !in_str => square += 1,
            ']' if !in_str => square -= 1,
            '{' if !in_str => curly += 1,
            '}' if !in_str => curly -= 1,
            _ => {}
        }
    }
    square == 0 && curly == 0
}

/// 键名允许写成 "带 空格的名字" 或裸名字。
fn unquote_key(text: &str, line_no: usize) -> Result<String, String> {
    let t = text.trim();
    if t.starts_with('"') {
        return parse_quoted(t, line_no);
    }
    Ok(t.to_string())
}

/// 去掉行内注释，但保留字符串内部的 '#'。
fn strip_comment(line: &str) -> String {
    let mut out = String::new();
    let mut in_str = false;
    let mut escaped = false;
    for ch in line.chars() {
        if escaped {
            out.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_str => {
                out.push(ch);
                escaped = true;
            }
            '"' => {
                in_str = !in_str;
                out.push(ch);
            }
            '#' if !in_str => break,
            _ => out.push(ch),
        }
    }
    out
}

/// 找到不在引号/括号内的第一个 '='。
fn find_top_level_eq(line: &str) -> Option<usize> {
    let mut in_str = false;
    for (i, ch) in line.char_indices() {
        match ch {
            '"' => in_str = !in_str,
            '=' if !in_str => return Some(i),
            _ => {}
        }
    }
    None
}

fn parse_value(text: &str, line_no: usize) -> Result<Value, String> {
    let t = text.trim();
    if t.starts_with('"') {
        return Ok(Value::Str(parse_quoted(t, line_no)?));
    }
    if t == "true" {
        return Ok(Value::Bool(true));
    }
    if t == "false" {
        return Ok(Value::Bool(false));
    }
    if t.starts_with('[') {
        let inner = t
            .strip_prefix('[')
            .and_then(|s| s.strip_suffix(']'))
            .ok_or_else(|| format!("第 {line_no} 行：数组缺少结尾的 ']'"))?;
        let mut items = Vec::new();
        for part in split_quoted(inner) {
            let p = part.trim();
            if p.is_empty() {
                continue;
            }
            items.push(parse_quoted(p, line_no)?);
        }
        return Ok(Value::Array(items));
    }
    if t.starts_with('{') {
        let inner = t
            .strip_prefix('{')
            .and_then(|s| s.strip_suffix('}'))
            .ok_or_else(|| format!("第 {line_no} 行：内联表缺少结尾的 '}}'"))?;
        let mut table = HashMap::new();
        for part in split_quoted(inner) {
            let p = part.trim();
            if p.is_empty() {
                continue;
            }
            let eq = find_top_level_eq(p)
                .ok_or_else(|| format!("第 {line_no} 行：内联表项缺少 '=' -> {p}"))?;
            let k = p[..eq].trim().to_string();
            let v = parse_value(p[eq + 1..].trim(), line_no)?;
            table.insert(k, v);
        }
        return Ok(Value::Table(table));
    }
    Err(format!("第 {line_no} 行：无法识别的值 -> {t}"))
}

fn parse_quoted(text: &str, line_no: usize) -> Result<String, String> {
    let t = text.trim();
    let inner = if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        &t[1..t.len() - 1]
    } else {
        return Err(format!("第 {line_no} 行：期望用双引号包裹的字符串 -> {t}"));
    };
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => return Err(format!("第 {line_no} 行：字符串以未完成的转义结尾")),
        }
    }
    Ok(out)
}

/// 按逗号切分，但跳过引号内的逗号。
fn split_quoted(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut in_str = false;
    let mut escaped = false;
    for ch in text.chars() {
        if escaped {
            cur.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_str => {
                cur.push(ch);
                escaped = true;
            }
            '"' => {
                in_str = !in_str;
                cur.push(ch);
            }
            ',' if !in_str => {
                parts.push(std::mem::take(&mut cur));
            }
            _ => cur.push(ch),
        }
    }
    parts.push(cur);
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sections_strings_bools_arrays() {
        let src = r#"
# 注释
[general]
shared_dir = "C:\\x\\Obsidian-Config"   # 行尾注释
verify = true

[links]
dirs = ["plugins", "themes", "snippets"]

[vaults]
"AI" = { path = "Obsidian-AI" }
"#;
        let doc = parse(src).expect("should parse");
        assert_eq!(
            doc.get("general", "shared_dir").unwrap().as_str().unwrap(),
            r"C:\x\Obsidian-Config"
        );
        assert!(doc.get("general", "verify").unwrap().as_bool().unwrap());
        assert_eq!(
            doc.get("links", "dirs").unwrap().as_array().unwrap(),
            &["plugins".to_string(), "themes".to_string(), "snippets".to_string()]
        );
        let v = doc.get("vaults", "AI").unwrap().as_table().unwrap();
        assert_eq!(v.get("path").unwrap().as_str().unwrap(), "Obsidian-AI");
    }

    #[test]
    fn hash_inside_string_is_not_a_comment() {
        let src = "[general]\nshared_dir = \"C:\\a#b\"\n";
        let doc = parse(src).unwrap();
        assert_eq!(doc.get("general", "shared_dir").unwrap().as_str().unwrap(), r"C:\a#b");
    }

    #[test]
    fn rejects_unknown_value() {
        assert!(parse("[general]\nroot = nope\n").is_err());
    }

    #[test]
    fn parses_multiline_arrays_with_inner_comments() {
        let src = r#"
[links]
dirs = [
  ".obsidian/plugins",   # 插件
  ".obsidian/themes",
  "zip",
]

[general]
root = ".."
"#;
        let doc = parse(src).expect("should parse");
        assert_eq!(
            doc.get("links", "dirs").unwrap().as_array().unwrap(),
            &[
                ".obsidian/plugins".to_string(),
                ".obsidian/themes".to_string(),
                "zip".to_string()
            ]
        );
        assert_eq!(doc.get("general", "root").unwrap().as_str().unwrap(), "..");
    }

    #[test]
    fn errors_on_unterminated_array() {
        let err = parse("[links]\ndirs = [\n \"a\",\n").unwrap_err();
        assert!(err.contains("缺少结尾"), "unexpected error: {err}");
    }
}
