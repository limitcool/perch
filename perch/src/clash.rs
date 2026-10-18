//! Clash Verge(mihomo)联动:读当前模式 / 节点 / 延迟,一键切 规则 / 全局 / 直连。
//! 走本机 external-controller(默认读 Clash Verge 自己的 config.yaml 里的地址和密钥,只访问 127.0.0.1;密钥不记录、不显示)。
//! 用 std 的 TcpStream 手写最小 HTTP/1.1,不引入网络库;所有请求都在后台线程里做,不卡界面。

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Mutex;
use std::time::Duration;

#[derive(Clone, Default, Debug)]
pub struct State {
    /// "rule" / "global" / "direct"
    pub mode: String,
    pub node: String,
    /// 当前节点所在的组(测速用:/proxies/<节点>/delay 对带 | 的名字会 404,只能用 /group/<组>/delay)
    pub parent: String,
    /// 毫秒,-1 = 未知 / 超时
    pub delay: i32,
    /// 是否连得上控制接口
    pub ok: bool,
    pub busy: bool,
}

static STATE: Mutex<State> = Mutex::new(State { mode: String::new(), node: String::new(), parent: String::new(), delay: -1, ok: false, busy: false });

pub fn state() -> State {
    STATE.lock().map(|s| s.clone()).unwrap_or_default()
}

struct Api {
    addr: String,
    secret: String,
}

fn yaml_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let l = l.trim_end();
        l.strip_prefix(key)?.strip_prefix(':').map(|v| v.trim().trim_matches(|c| c == '\'' || c == '"').to_string())
    })
}

fn load_api() -> Option<Api> {
    let dir = std::path::PathBuf::from(std::env::var("APPDATA").ok()?).join("io.github.clash-verge-rev.clash-verge-rev");
    let text = std::fs::read_to_string(dir.join("config.yaml")).ok()?;
    let ctrl = yaml_value(&text, "external-controller").filter(|s| !s.is_empty())?;
    // 只允许本机地址,避免把密钥发到别处
    let addr = ctrl.replace("0.0.0.0", "127.0.0.1");
    if !(addr.starts_with("127.0.0.1:") || addr.starts_with("localhost:")) {
        return None;
    }
    Some(Api { addr, secret: yaml_value(&text, "secret").unwrap_or_default() })
}

/// 最小 HTTP 请求,返回 (状态码, body)
fn request(api: &Api, method: &str, path: &str, body: Option<&str>, timeout: Duration) -> Option<(u16, String)> {
    let mut s = TcpStream::connect_timeout(&api.addr.parse().ok().or_else(|| api.addr.replace("localhost", "127.0.0.1").parse().ok())?, Duration::from_millis(800)).ok()?;
    let _ = s.set_read_timeout(Some(timeout));
    let _ = s.set_write_timeout(Some(Duration::from_secs(2)));
    let mut req = format!("{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n", method, path, api.addr);
    if !api.secret.is_empty() {
        req.push_str(&format!("Authorization: Bearer {}\r\n", api.secret));
    }
    match body {
        Some(b) => req.push_str(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{}", b.len(), b)),
        None => req.push_str("\r\n"),
    }
    s.write_all(req.as_bytes()).ok()?;
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf).to_string();
    let code = text.lines().next()?.split_whitespace().nth(1)?.parse::<u16>().ok()?;
    let (head, rest) = text.split_once("\r\n\r\n").map(|x| (x.0.to_lowercase(), x.1.to_string())).unwrap_or_default();
    // 大响应(如 /proxies)是 chunked 分块编码:每块前面有一行十六进制长度
    let body = if head.contains("transfer-encoding: chunked") { dechunk(&rest) } else { rest };
    Some((code, body))
}

fn dechunk(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let Some(nl) = bytes[i..].windows(2).position(|w| w == b"\r\n") else { break };
        let size = usize::from_str_radix(String::from_utf8_lossy(&bytes[i..i + nl]).split(';').next().unwrap_or("0").trim(), 16).unwrap_or(0);
        i += nl + 2;
        if size == 0 || i + size > bytes.len() {
            if size > 0 {
                out.extend_from_slice(&bytes[i..]);
            }
            break;
        }
        out.extend_from_slice(&bytes[i..i + size]);
        i += size + 2;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn json(api: &Api, path: &str, timeout: Duration) -> Option<serde_json::Value> {
    let (code, body) = request(api, "GET", path, None, timeout)?;
    if code != 200 {
        return None;
    }
    serde_json::from_str(&body).ok()
}

fn encode_name(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') { (b as char).to_string() } else { format!("%{:02X}", b) }).collect()
}

/// 当前模式 + 节点(global 看 GLOBAL 组;rule 看第一个选择组;沿 now 一直追到真正的节点)
fn fetch(api: &Api) -> Option<State> {
    let cfg = json(api, "/configs", Duration::from_secs(2))?;
    let mode = cfg["mode"].as_str().unwrap_or("").to_lowercase();
    let proxies = json(api, "/proxies", Duration::from_secs(3)).map(|p| p["proxies"].clone()).unwrap_or_default();
    let start = if mode == "global" {
        "GLOBAL".to_string()
    } else {
        // 规则模式:GLOBAL 组的 all 里的第一个策略组通常就是主选择组
        proxies["GLOBAL"]["all"].as_array().and_then(|a| a.iter().filter_map(|n| n.as_str()).find(|n| proxies[*n]["now"].is_string())).map(|s| s.to_string()).unwrap_or_else(|| "GLOBAL".to_string())
    };
    let mut node = start;
    let mut parent = String::new();
    for _ in 0..5 {
        match proxies[node.as_str()]["now"].as_str() {
            Some(next) if !next.is_empty() => {
                parent = node.clone();
                node = next.to_string();
            }
            _ => break,
        }
    }
    if mode == "direct" {
        node = "DIRECT".to_string();
        parent.clear();
    }
    Some(State { mode, node, parent, delay: -1, ok: true, busy: false })
}

/// 测当前节点延迟:对它所在的组做一次组测速(会测组内所有节点,所以只在用户点 ↻ 时才调用),取出当前节点那一项
fn measure(api: &Api, parent: &str, node: &str) -> i32 {
    if parent.is_empty() || node.is_empty() || node == "DIRECT" {
        return -1;
    }
    let path = format!("/group/{}/delay?url=http%3A%2F%2Fwww.gstatic.com%2Fgenerate_204&timeout=3000", encode_name(parent));
    json(api, &path, Duration::from_secs(12)).and_then(|v| v[node].as_i64()).map(|d| d as i32).unwrap_or(-1)
}

/// 后台刷新:只读模式和节点(不产生测速流量)
pub fn refresh_async() {
    spawn_refresh(false);
}

/// 点 ↻:读模式和节点,再测一次当前节点的延迟
pub fn measure_async() {
    spawn_refresh(true);
}

fn spawn_refresh(with_delay: bool) {
    if let Ok(mut s) = STATE.lock() {
        if s.busy {
            return;
        }
        s.busy = true;
    }
    std::thread::spawn(move || {
        let Some(api) = load_api() else {
            if let Ok(mut s) = STATE.lock() {
                *s = State { ok: false, delay: -1, ..Default::default() };
            }
            return;
        };
        match fetch(&api) {
            Some(mut st) => {
                // 保留上一次测到的延迟(模式和节点没变的话)
                let prev = state();
                if prev.node == st.node {
                    st.delay = prev.delay;
                }
                st.busy = with_delay;
                if let Ok(mut s) = STATE.lock() {
                    *s = st.clone();
                }
                if with_delay {
                    st.delay = measure(&api, &st.parent, &st.node);
                }
                st.busy = false;
                if let Ok(mut s) = STATE.lock() {
                    *s = st;
                }
            }
            None => {
                if let Ok(mut s) = STATE.lock() {
                    *s = State { ok: false, delay: -1, ..Default::default() };
                }
            }
        }
    });
}

/// 切换模式("rule" / "global" / "direct"),成功后刷新
pub fn set_mode_async(mode: &'static str) {
    if let Ok(mut s) = STATE.lock() {
        s.mode = mode.to_string();
    }
    std::thread::spawn(move || {
        if let Some(api) = load_api() {
            let _ = request(&api, "PATCH", "/configs", Some(&format!("{{\"mode\":\"{}\"}}", mode)), Duration::from_secs(3));
        }
        if let Ok(mut s) = STATE.lock() {
            s.busy = false;
        }
        refresh_async();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_keys() {
        let t = "port: 1\nexternal-controller: 127.0.0.1:9097\nsecret: 'abc 123'\nexternal-controller-pipe: x\n";
        assert_eq!(yaml_value(t, "external-controller").as_deref(), Some("127.0.0.1:9097"));
        assert_eq!(yaml_value(t, "secret").as_deref(), Some("abc 123"));
        assert_eq!(yaml_value(t, "mode"), None);
    }

    #[test]
    fn chunked_decoding() {
        assert_eq!(dechunk("5\r\nhello\r\n7\r\n 世界\r\n0\r\n\r\n"), "hello 世界");
        assert_eq!(dechunk("0\r\n\r\n"), "");
    }

    #[test]
    fn name_encoding() {
        assert_eq!(encode_name("HK 01"), "HK%2001");
        assert_eq!(encode_name("香港"), "%E9%A6%99%E6%B8%AF");
    }

    /// 只读 + 设为当前模式(幂等):确认鉴权和 PATCH 路径可用。cargo test clash_live -- --ignored --nocapture
    #[test]
    #[ignore]
    fn clash_live() {
        let api = load_api().expect("没找到 Clash Verge 的控制接口配置");
        let st = fetch(&api).expect("连不上控制接口");
        println!("mode={} node={} (密钥未显示)", st.mode, st.node);
        let body = format!("{{\"mode\":\"{}\"}}", st.mode);
        let (code, _) = request(&api, "PATCH", "/configs", Some(&body), Duration::from_secs(3)).expect("PATCH 失败");
        println!("PATCH 同一模式 → HTTP {}", code);
        println!("所在组 = {} ,延迟 = {} ms", st.parent, measure(&api, &st.parent, &st.node));
    }
}
