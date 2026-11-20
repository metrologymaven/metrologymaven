//! Test-only native artifact that reads its call and never answers: the hanging artifact of the
//! per-call deadline test (KRN.MAN.XART, fail closed).

use std::io::Read;

fn main() {
    let mut input = Vec::new();
    let _ = std::io::stdin().lock().take(1 << 20).read_to_end(&mut input);
    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}
