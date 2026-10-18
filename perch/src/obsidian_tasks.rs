//! Obsidian 今日日记里的 `- [ ]` 任务 ↔ 待办卡片(双向):
//!   读:把今天日记里的任务并进待办列表(id = ID_BASE + 行号,文字前带 📓);
//!   写:在卡片上点一下,把那一行的 `[ ]` / `[x]` 改写回日记文件。
//! 只读取今天的日记,日记不存在时不创建;只改任务行的复选框,不动其他内容。

use std::path::PathBuf;
use std::time::SystemTime;

/// 待办 id ≥ 这个值就是 Obsidian 任务(id - ID_BASE = 日记文件里的行号)
pub const ID_BASE: u32 = 1_000_000;

#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub line: usize,
    pub text: String,
    pub done: bool,
}

pub fn is_obsidian_id(id: u32) -> bool {
    id >= ID_BASE
}

/// 今天日记的路径(库里没有这个文件也返回路径,由调用方判断是否存在)
pub fn note_path() -> Option<PathBuf> {
    let vault = crate::tools::vault()?;
    Some(crate::tools::daily_note_path(&vault).0)
}

/// 解析一行是不是任务:返回 (复选框内的字符位置, 是否已完成, 任务文字)
fn parse_line(line: &str) -> Option<(bool, String)> {
    let t = line.trim_start();
    let rest = t.strip_prefix("- [").or_else(|| t.strip_prefix("* [")).or_else(|| t.strip_prefix("+ ["))?;
    let mut ch = rest.chars();
    let mark = ch.next()?;
    if !matches!(mark, ' ' | 'x' | 'X') || ch.next()? != ']' {
        return None;
    }
    let text = ch.as_str().trim();
    // 模板里的 "1. " 之类编号占位:去掉编号,剩下是空的就不算任务
    let text = text.trim_start_matches(|c: char| c.is_ascii_digit()).trim_start_matches(['.', ')']).trim();
    if text.is_empty() {
        return None;
    }
    Some((mark != ' ', text.to_string()))
}

pub fn parse(content: &str) -> Vec<Task> {
    content.lines().enumerate().filter_map(|(i, l)| parse_line(l).map(|(done, text)| Task { line: i, text, done })).take(40).collect()
}

/// 按文件修改时间缓存:没变化就不重新读。返回 true 表示任务列表有变化
pub fn refresh(key: &mut Option<(PathBuf, SystemTime)>, cache: &mut Vec<Task>) -> bool {
    if !crate::config::get().notes.sync_tasks {
        let changed = !cache.is_empty();
        cache.clear();
        *key = None;
        return changed;
    }
    let Some(path) = note_path() else {
        let changed = !cache.is_empty();
        cache.clear();
        return changed;
    };
    let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    match mtime {
        None => {
            let changed = !cache.is_empty();
            cache.clear();
            *key = None;
            changed
        }
        Some(m) => {
            if key.as_ref().map_or(false, |(p, t)| *p == path && *t == m) {
                return false;
            }
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            let new = parse(&content);
            let changed = new != *cache;
            *cache = new;
            *key = Some((path, m));
            changed
        }
    }
}

/// 把第 line 行任务的复选框翻转并写回;那一行已经不是任务了就什么也不做
pub fn toggle(line: usize) -> bool {
    let Some(path) = note_path() else { return false };
    let Ok(content) = std::fs::read_to_string(&path) else { return false };
    let crlf = content.contains("\r\n");
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let Some(l) = lines.get_mut(line) else { return false };
    if parse_line(l).is_none() {
        return false;
    }
    // 只替换第一个复选框
    let flipped = if let Some(pos) = l.find("[ ]") {
        format!("{}[x]{}", &l[..pos], &l[pos + 3..])
    } else if let Some(pos) = l.find("[x]").or_else(|| l.find("[X]")) {
        format!("{}[ ]{}", &l[..pos], &l[pos + 3..])
    } else {
        return false;
    };
    *l = flipped;
    let sep = if crlf { "\r\n" } else { "\n" };
    let had_trailing = content.ends_with('\n');
    std::fs::write(&path, lines.join(sep) + if had_trailing { sep } else { "" }).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tasks_and_skips_placeholders() {
        let note = "# 日记\n\n## 三件事\n- [ ] 1. \n- [ ] 2. \n- [x] 3. 写完周报\n- [ ] 发邮件\n  - [X] 缩进的子任务\n- 普通列表\n* [ ] 星号任务\n";
        let t = parse(note);
        let texts: Vec<&str> = t.iter().map(|x| x.text.as_str()).collect();
        assert_eq!(texts, vec!["写完周报", "发邮件", "缩进的子任务", "星号任务"]);
        assert!(t[0].done && !t[1].done && t[2].done && !t[3].done);
        assert_eq!(t[1].line, 6); // 行号从 0 开始:"发邮件" 是第 7 行
    }

    #[test]
    fn line_numbers_are_zero_based() {
        let t = parse("a\n- [ ] x\n");
        assert_eq!(t[0].line, 1);
    }

    /// 用真实的 Obsidian 库走一遍:创建今天的日记(按模板)→ 写闪念 → 读任务 → 翻转复选框 → 删掉自己创建的文件。
    /// 今天的日记已存在时拒绝运行(不碰你的内容)。cargo test real_vault_flow -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_vault_flow() {
        let vault = crate::tools::vault().expect("没找到 Obsidian 库");
        let (path, _) = crate::tools::daily_note_path(&vault);
        println!("日记路径: {}", path.display());
        assert!(!path.exists(), "今天的日记已存在,拒绝运行");
        let created = crate::tools::ensure_daily(&vault);
        assert_eq!(created, path);
        let body = std::fs::read_to_string(&path).unwrap();
        println!("--- 新建的日记 ---
{}", body);
        assert!(!body.contains("{{date}}"), "模板变量没替换");
        assert!(crate::tools::append_diary("12:34", "HUD 测试闪念"));
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("- 12:34 HUD 测试闪念"));
        // 追加一个任务再读、再翻转
        let mut b = body.clone();
        b.push_str("
- [ ] HUD 测试任务
");
        std::fs::write(&path, b).unwrap();
        let tasks = parse(&std::fs::read_to_string(&path).unwrap());
        println!("任务: {:?}", tasks);
        let t = tasks.iter().find(|t| t.text == "HUD 测试任务").expect("没读到任务");
        assert!(!t.done);
        assert!(toggle(t.line));
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.contains("- [x] HUD 测试任务"));
        assert!(toggle(t.line));
        assert!(std::fs::read_to_string(&path).unwrap().contains("- [ ] HUD 测试任务"));
        // 清理:这个文件是本测试创建的
        std::fs::remove_file(&path).unwrap();
        assert!(!path.exists());
        println!("已清理,库没有净变化");
    }
}
