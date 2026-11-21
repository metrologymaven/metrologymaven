//! PLAT.GPU.POLICY follow-up (change log 2026-11-21): `gl` and `webgpu` are policy and
//! known-bad backends, neither mandatory, each needing its `min` entry in a row like every
//! backend but `software`; the original four keep their order. Expected outcomes are
//! hand-labelled from the spec's rules.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_gpu_policy::policy::{parse_known_bad, validate_known_bad, validate_policy, Backend, GpuOs, GpuPolicyError};
use fid_gpu_policy::{Arch, Version};

fn row(os: &str, backends: &str, min: &str) -> String {
    format!("schema_version = 1\n\n[[row]]\nos = \"{os}\"\narch = \"x86_64\"\nbackends = [{backends}]\n{min}")
}

#[test]
fn names_and_order() {
    assert_eq!(Backend::from_name("gl"), Some(Backend::Gl));
    assert_eq!(Backend::from_name("webgpu"), Some(Backend::WebGpu));
    assert_eq!(Backend::Gl.as_str(), "gl");
    assert_eq!(Backend::WebGpu.to_string(), "webgpu");
    assert_eq!(Backend::from_name("opengl"), None);
    assert_eq!(Backend::from_name("GL"), None);
    // The original four first, in their old order.
    assert_eq!(
        Backend::ALL,
        &[Backend::Vulkan, Backend::Metal, Backend::Dx12, Backend::Software, Backend::Gl, Backend::WebGpu]
    );
    // Neither is mandatory anywhere.
    for os in [GpuOs::Windows, GpuOs::Linux, GpuOs::Macos] {
        assert!(!os.mandatory().contains(&Backend::Gl) && !os.mandatory().contains(&Backend::WebGpu));
    }
}

#[test]
fn rows_with_gl_and_webgpu_validate() {
    let text = row(
        "linux",
        "\"vulkan\", \"gl\", \"webgpu\", \"software\"",
        "min.vulkan = { api = \"1.1\" }\nmin.gl = { api = \"3.3\" }\nmin.webgpu = { api = \"1\" }\n",
    );
    let v = validate_policy(text.as_bytes());
    assert!(v.errors.is_empty(), "{:?}", v.errors);
    let p = v.value.unwrap();
    let r = p.row(GpuOs::Linux, Arch::X86_64).unwrap();
    assert_eq!(r.backends, [Backend::Vulkan, Backend::Gl, Backend::WebGpu, Backend::Software]);
    assert_eq!(r.min[&Backend::Gl].api, Version::parse("3.3").unwrap());
}

#[test]
fn gl_needs_its_minimum_and_rules_still_apply() {
    let field = |e: &GpuPolicyError| match e {
        GpuPolicyError::BadField { field, msg, .. } => format!("{field} {msg}"),
        other => other.to_string(),
    };
    // No `min.gl`.
    let e = validate_policy(
        row("linux", "\"vulkan\", \"gl\", \"software\"", "min.vulkan = { api = \"1.1\" }\n").as_bytes(),
    )
    .errors;
    assert_eq!(e.iter().map(field).collect::<Vec<_>>(), ["min.gl is missing"]);
    // A `min.webgpu` for a backend the row does not list.
    let e = validate_policy(
        row("linux", "\"vulkan\", \"software\"", "min.vulkan = { api = \"1.1\" }\nmin.webgpu = { api = \"1\" }\n")
            .as_bytes(),
    )
    .errors;
    assert_eq!(e.iter().map(field).collect::<Vec<_>>(), ["min.webgpu names a backend that is not in backends"]);
    // Listed twice.
    let e = validate_policy(
        row(
            "linux",
            "\"vulkan\", \"gl\", \"gl\", \"software\"",
            "min.vulkan = { api = \"1.1\" }\nmin.gl = { api = \"3.3\" }\n",
        )
        .as_bytes(),
    )
    .errors;
    assert_eq!(e, [GpuPolicyError::DuplicateBackend { row: 1, backend: Backend::Gl }]);
    // gl does not stand in for a mandatory backend, and software must still come last.
    let e = validate_policy(row("linux", "\"software\", \"gl\"", "min.gl = { api = \"3.3\" }\n").as_bytes()).errors;
    assert!(e.iter().any(|x| matches!(x, GpuPolicyError::MissingMandatoryBackend { backend: Backend::Vulkan, .. })));
    assert!(e.iter().any(|x| matches!(x, GpuPolicyError::NoSoftwareFallback { .. })));
    // Unknown names are refused with the new list in the message.
    let e = validate_policy(
        row("linux", "\"vulkan\", \"opengl\", \"software\"", "min.vulkan = { api = \"1.1\" }\n").as_bytes(),
    )
    .errors;
    assert_eq!(
        e.iter().map(field).collect::<Vec<_>>(),
        ["backends has an element that is not one of vulkan, metal, dx12, gl, webgpu, software"]
    );
}

#[test]
fn known_bad_entries_on_gl() {
    let text = "schema_version = 1\n\n[[entry]]\nid = \"gl-test\"\nos = \"linux\"\nbackend = \"gl\"\nvendor_id = 0x1234\ndevice_ids = \"any\"\ndriver = \"any\"\naction = \"avoid_backend\"\nreason = \"Invented test entry.\"\nreference = \"https://example.invalid/bug/1\"\n";
    let v = validate_known_bad(text.as_bytes());
    assert!(v.errors.is_empty(), "{:?}", v.errors);
    let k = parse_known_bad(text.as_bytes()).unwrap();
    assert_eq!(k.entries[0].backend, Backend::Gl);
}
