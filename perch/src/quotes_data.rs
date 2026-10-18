//! perch::quotes_data: 动态桥接 perch_data::theme_pack 台词语录

use std::collections::HashMap;
use std::sync::Mutex;

pub fn get_theme_quotes(theme: &str) -> (&'static str, &'static [&'static str]) {
    static CACHE: Mutex<Option<HashMap<String, (&'static str, &'static [&'static str])>>> = Mutex::new(None);
    let mut guard = CACHE.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    if !map.contains_key(theme) {
        let pack = perch_data::theme_pack::load(theme);
        let speaker: &'static str = Box::leak(pack.quotes.speaker.into_boxed_str());
        let lines: Vec<&'static str> = pack
            .quotes
            .lines
            .into_iter()
            .map(|l| {
                let normalized = l.replace("#CRLF#", "\n");
                let leaked: &'static str = Box::leak(normalized.into_boxed_str());
                leaked
            })
            .collect();
        let lines_slice: &'static [&'static str] = Box::leak(lines.into_boxed_slice());
        map.insert(theme.to_string(), (speaker, lines_slice));
    }
    let (spk, lines) = map.get(theme).unwrap();
    (*spk, *lines)
}
