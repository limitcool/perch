//! perch-data::i18n: 国际化本地化字符串表引擎
//! 支持内置 zh-CN / en，支持 <root>/lang/<code>.toml 外部覆盖，支持变量模板替换

use std::collections::HashMap;
use std::fs;
use std::sync::RwLock;

const BUILTIN_ZH_CN: &str = include_str!("../lang/zh-CN.toml");
const BUILTIN_EN: &str = include_str!("../lang/en.toml");

static CURRENT_LANG: RwLock<Option<String>> = RwLock::new(None);
static LANG_TABLES: RwLock<Option<HashMap<String, HashMap<String, String>>>> = RwLock::new(None);

/// 递归将 toml::Value 展平成以 '.' 分隔的点分键字典
fn flatten_toml(prefix: &str, value: &toml::Value, out: &mut HashMap<String, String>) {
    match value {
        toml::Value::Table(table) => {
            for (k, v) in table {
                let next_prefix = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{}.{}", prefix, k)
                };
                flatten_toml(&next_prefix, v, out);
            }
        }
        toml::Value::String(s) => {
            out.insert(prefix.to_string(), s.clone());
        }
        _ => {}
    }
}

/// 解析单个语言包文本为扁平键值对
fn parse_lang_content(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Ok(val) = toml::from_str::<toml::Value>(content) {
        flatten_toml("", &val, &mut map);
    }
    map
}

/// 读取指定语言代码的字符串表 (优先外部 <root>/lang/<code>.toml，其次回退内置)
fn load_table(lang: &str) -> HashMap<String, String> {
    let external_path = crate::paths::root().join("lang").join(format!("{}.toml", lang));
    if external_path.is_file() {
        if let Ok(content) = fs::read_to_string(&external_path) {
            let map = parse_lang_content(&content);
            if !map.is_empty() {
                return map;
            }
        }
    }

    match lang {
        "en" => parse_lang_content(BUILTIN_EN),
        _ => parse_lang_content(BUILTIN_ZH_CN),
    }
}

/// 获取系统 UI 语言代码 (Windows 优先)
fn detect_system_language() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        extern "system" {
            fn GetUserDefaultUILanguage() -> u16;
        }
        let lcid = unsafe { GetUserDefaultUILanguage() };
        // 0x0804 为 zh-CN, 0x0404/0x0c04 为繁中
        if (lcid & 0x00FF) == 0x04 {
            return "zh-CN";
        }
    }
    if let Ok(lang) = std::env::var("LANG") {
        if lang.to_lowercase().starts_with("zh") {
            return "zh-CN";
        }
    }
    "en"
}

/// 初始化当前语言 (根据 config.toml [ui] language)
pub fn init_from_config(config_lang: &str) {
    let lang = match config_lang.trim() {
        "auto" | "" => detect_system_language().to_string(),
        "en" => "en".to_string(),
        "zh" | "zh-CN" | "zh_CN" => "zh-CN".to_string(),
        other => other.to_string(),
    };
    set_language(&lang);
}

/// 设置当前生效语言
pub fn set_language(lang: &str) {
    if let Ok(mut cur) = CURRENT_LANG.write() {
        *cur = Some(lang.to_string());
    }
}

/// 获取当前生效语言代码
pub fn current_language() -> String {
    if let Ok(cur) = CURRENT_LANG.read() {
        if let Some(ref l) = *cur {
            return l.clone();
        }
    }
    detect_system_language().to_string()
}

/// 获取对应 key 的本地化字符串
pub fn t(key: &str) -> String {
    let lang = current_language();

    // 检查缓存
    let mut need_load_lang = false;
    let mut need_load_zh = false;
    {
        if let Ok(cache) = LANG_TABLES.read() {
            if let Some(ref map) = *cache {
                if !map.contains_key(&lang) {
                    need_load_lang = true;
                }
                if !map.contains_key("zh-CN") {
                    need_load_zh = true;
                }
            } else {
                need_load_lang = true;
                need_load_zh = true;
            }
        }
    }

    if need_load_lang || need_load_zh {
        if let Ok(mut cache) = LANG_TABLES.write() {
            let map = cache.get_or_insert_with(HashMap::new);
            if !map.contains_key(&lang) {
                map.insert(lang.clone(), load_table(&lang));
            }
            if !map.contains_key("zh-CN") {
                map.insert("zh-CN".to_string(), load_table("zh-CN"));
            }
        }
    }

    // 1. 查找当前语言
    if let Ok(cache) = LANG_TABLES.read() {
        if let Some(ref map) = *cache {
            if let Some(table) = map.get(&lang) {
                if let Some(val) = table.get(key) {
                    return val.clone();
                }
            }
            // 2. 回退到 zh-CN
            if let Some(table) = map.get("zh-CN") {
                if let Some(val) = table.get(key) {
                    return val.clone();
                }
            }
        }
    }

    // 3. 回退到 key 本身
    key.to_string()
}

/// 支持参数模板替换的本地化取值: 如 "确定要{what}吗?"
pub fn t_fmt(key: &str, vars: &[(&str, &str)]) -> String {
    let mut template = t(key);
    for (name, val) in vars {
        let needle = format!("{{{}}}", name);
        template = template.replace(&needle, val);
    }
    template
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zh_and_en_have_same_keys_and_placeholders() {
        let zh = parse_lang_content(BUILTIN_ZH_CN);
        let en = parse_lang_content(BUILTIN_EN);
        let placeholders = |s: &str| -> Vec<String> {
            let mut v: Vec<String> = s.split('{').skip(1).filter_map(|p| p.split('}').next()).map(|x| x.to_string()).collect();
            v.sort();
            v
        };
        for (k, zv) in &zh {
            let ev = en.get(k).unwrap_or_else(|| panic!("key {} missing in en.toml", k));
            assert_eq!(placeholders(zv), placeholders(ev), "placeholder mismatch for key {}", k);
        }
        for k in en.keys() {
            assert!(zh.contains_key(k), "key {} missing in zh-CN.toml", k);
        }
    }

    #[test]
    fn test_all_keys_used_in_sources_exist() {
        let zh = parse_lang_content(BUILTIN_ZH_CN);
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut missing: Vec<String> = Vec::new();
        for crate_dir in ["perch/src", "perch-core/src"] {
            let dir = base.join(crate_dir);
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().map_or(true, |x| x != "rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&p).unwrap_or_default();
                for pat in ["t(\"", "t_fmt(\""] {
                    let mut rest = text.as_str();
                    while let Some(i) = rest.find(pat) {
                        let before = rest[..i].chars().last().unwrap_or(' ');
                        let after = &rest[i + pat.len()..];
                        if let Some(end) = after.find('"') {
                            let key = &after[..end];
                            let is_key = key.contains('.') && key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.');
                            if !(before.is_alphanumeric() || before == '_') && is_key && !zh.contains_key(key) {
                                missing.push(format!("{}: {}", p.file_name().unwrap().to_string_lossy(), key));
                            }
                        }
                        rest = &rest[i + pat.len()..];
                    }
                }
            }
        }
        assert!(missing.is_empty(), "i18n keys used in code but missing in zh-CN.toml: {:?}", missing);
    }

    #[test]
    fn test_i18n_fallback_and_format() {
        set_language("zh-CN");
        assert_eq!(t("tray.reload"), "重新载入配置");
        assert_eq!(t("sys.lock"), "锁屏");
        assert_eq!(t_fmt("sys.confirm_prompt", &[("what", "关机")]), "确定要关机吗?");

        set_language("en");
        assert_eq!(t("tray.reload"), "Reload Configuration");
        assert_eq!(t("sys.lock"), "Lock");
        assert_eq!(t_fmt("sys.confirm_prompt", &[("what", "Shutdown")]), "Are you sure you want to Shutdown?");

        // 缺省 key 回退测试
        assert_eq!(t("non_existing_key_xyz"), "non_existing_key_xyz");
    }
}
