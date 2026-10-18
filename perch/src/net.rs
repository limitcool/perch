//! 网络吞吐与延迟采样(后台线程,结果放进原子量,绘制线程只读)。

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;
use windows::Win32::NetworkManagement::IpHelper::*;

pub static DOWN_BPS: AtomicU64 = AtomicU64::new(0);
pub static UP_BPS: AtomicU64 = AtomicU64::new(0);
/// 最近一次 ping 毫秒,0 表示失败/未知
pub static PING_MS: AtomicU32 = AtomicU32::new(0);
/// 桌面被盖住时暂停采样(吞吐 + ping),完全不占 CPU
pub static PAUSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn totals() -> (u64, u64) {
    unsafe {
        let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
        if GetIfTable2(&mut table).is_err() || table.is_null() {
            return (0, 0);
        }
        let (mut rx, mut tx) = (0u64, 0u64);
        let n = (*table).NumEntries as usize;
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), n);
        for r in rows {
            // 只统计已连接的非回环接口
            if r.OperStatus.0 == 1 && r.Type != 24 {
                rx += r.InOctets;
                tx += r.OutOctets;
            }
        }
        FreeMibTable(table as *const _);
        (rx, tx)
    }
}

fn ping_once() -> u32 {
    unsafe {
        let h = match IcmpCreateFile() {
            Ok(h) => h,
            Err(_) => return 0,
        };
        let data = [0u8; 8];
        let mut reply = vec![0u8; 256];
        let addr = u32::from_le_bytes([223, 5, 5, 5]);
        let n = IcmpSendEcho(h, addr, data.as_ptr() as *const _, data.len() as u16, None, reply.as_mut_ptr() as *mut _, reply.len() as u32, 800);
        let ms = if n > 0 {
            let r = &*(reply.as_ptr() as *const ICMP_ECHO_REPLY);
            if r.Status == 0 { r.RoundTripTime.max(1) } else { 0 }
        } else {
            0
        };
        let _ = IcmpCloseHandle(h);
        ms
    }
}

pub fn start() {
    std::thread::spawn(|| {
        let (mut last_rx, mut last_tx) = totals();
        let mut tick = 0u32;
        let mut was_paused = false;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            if PAUSED.load(Ordering::Relaxed) {
                was_paused = true;
                continue;
            }
            let (rx, tx) = totals();
            if was_paused {
                // 刚恢复:基准重置,避免把暂停期间的流量算成一个尖峰
                was_paused = false;
                last_rx = rx;
                last_tx = tx;
                continue;
            }
            DOWN_BPS.store(rx.saturating_sub(last_rx), Ordering::Relaxed);
            UP_BPS.store(tx.saturating_sub(last_tx), Ordering::Relaxed);
            last_rx = rx;
            last_tx = tx;
            if tick % 3 == 0 {
                PING_MS.store(ping_once(), Ordering::Relaxed);
            }
            tick = tick.wrapping_add(1);
        }
    });
}

pub fn fmt_rate(bps: u64) -> String {
    let kb = bps as f64 / 1024.0;
    if kb >= 1024.0 {
        format!("{:.1} MB/s", kb / 1024.0)
    } else {
        format!("{:.1} kB/s", kb)
    }
}
