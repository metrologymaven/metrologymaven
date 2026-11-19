//! Seeded KNP1-THREAD violation: exactly one planted finding (KRN.NUM.LINT.SYN AC-1).

pub fn spawn_noop() {
    let _ = std::thread::spawn(|| ()).join();
}
