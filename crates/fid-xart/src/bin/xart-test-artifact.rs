//! Test-only native artifact: the stand-in test algorithms, faithful (KRN.MAN.XART AC-2).

fn main() -> std::process::ExitCode {
    fid_xart::testart::main_with(fid_xart::testart::Order::Faithful)
}
