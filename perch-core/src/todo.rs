//! perch-core::todo: 纯 Rust 原生 Todo CRUD 数据管理引擎

use std::fs;
use std::path::PathBuf;
use perch_data::TodoItem;

pub fn get_todos_file_path() -> PathBuf {
    perch_data::paths::todos_json()
}

pub fn load_todos() -> Vec<TodoItem> {
    let path = get_todos_file_path();
    if let Ok(content) = fs::read_to_string(&path) {
        if let Ok(items) = serde_json::from_str::<Vec<TodoItem>>(&content) {
            return items;
        }
    }
    vec![
        TodoItem { id: 1, text: perch_data::t("core2.todo_welcome"), done: false },
    ]
}

pub fn save_todos(items: &[TodoItem]) {
    let path1 = get_todos_file_path();
    if let Some(parent) = path1.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(items) {
        let _ = fs::write(&path1, &json);
    }
}

pub fn handle_todo_command(subargs: &[String]) {
    if subargs.is_empty() {
        return;
    }

    let mut items = load_todos();

    match subargs[0].as_str() {
        "add" => {
            if subargs.len() > 1 {
                let text = subargs[1..].join(" ").trim().to_string();
                if !text.is_empty() {
                    let next_id = items.iter().map(|i| i.id).max().unwrap_or(0) + 1;
                    items.insert(0, TodoItem { id: next_id, text, done: false });
                    save_todos(&items);
                }
            }
        }
        "edit" => {
            if subargs.len() > 2 {
                if let Ok(id) = subargs[1].parse::<usize>() {
                    let text = subargs[2..].join(" ").trim().to_string();
                    if !text.is_empty() {
                        if let Some(item) = items.iter_mut().find(|i| i.id == id) {
                            item.text = text;
                            save_todos(&items);
                        }
                    }
                }
            }
        }
        "toggle" => {
            if subargs.len() > 1 {
                if let Ok(id) = subargs[1].parse::<usize>() {
                    if let Some(item) = items.iter_mut().find(|i| i.id == id) {
                        item.done = !item.done;
                        save_todos(&items);
                    }
                }
            }
        }
        "delete" => {
            if subargs.len() > 1 {
                if let Ok(id) = subargs[1].parse::<usize>() {
                    items.retain(|i| i.id != id);
                    save_todos(&items);
                }
            }
        }
        "clear" => {
            items.clear();
            save_todos(&items);
        }
        "clear-done" => {
            items.retain(|i| !i.done);
            save_todos(&items);
        }
        "list" => {}
        _ => {}
    }
}
