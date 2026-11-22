//! Format-plugin contract, descriptor, version negotiation, and `fid-io-reg`
//! adapters (spec node `IO.REG.PLUGIN.ABI`).
//!
//! One versioned IDL source (`abi/format-plugin.idl.json`) generates both the
//! WASM component interface (`abi/generated/format-plugin.wit`) and the
//! length-prefixed native host message schema
//! (`abi/generated/message-protocol.json`), keeping both hosts locked to the
//! same contract (REQ-IO.REG.PLUGIN.ABI-001).
//!
//! Every string returned by a plugin is typed as [`fid_io_reg::UntrustedText`]
//! (CONSTITUTION C6, REQ-IO.REG.PLUGIN.ABI-004), and the contract exposes only
//! a bounded byte stream and validated options — never file system, network,
//! clock or document capabilities (REQ-IO.REG.PLUGIN.ABI-006).

pub mod abi;
