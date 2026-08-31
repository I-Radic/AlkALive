//! Security invariant tests (Wave 7, docs/security/07-validation.md).
//!
//! Three CI-enforced invariants:
//!
//! 1. **T-I4/secrets sweep** — the shipped WASM artifact stays free of
//!    credential-shaped strings (no secrets ⇒ nothing for a timing or
//!    memory side channel to leak; see Wave 1 §1.4 for the rationale).
//! 2. **T-E5/SafeRace guard** — the WGSL and GLSL shader sources stay free
//!    of data-race-capable constructs (`var<storage>`, atomics, workgroup
//!    storage), so the SafeRace DRSV class (optimizers removing memory
//!    safety guardrails from racy shaders) cannot arise.
//! 3. **T-D1 budget end-to-end** — a real oversized scene through the real
//!    shaping/tessellation path produces a bounded vertex buffer and sets
//!    the `truncated` flag.

use alkalive_backend_wgpu::tessellate::{tessellate_scene, MAX_TEXT_VERTICES};
use alkalive_scene_data::TextSceneData;

// ---------------------------------------------------------------------------
// 1. Secrets sweep over the shipped artifact
// ---------------------------------------------------------------------------

/// The exact WASM bytes served to browsers (path is stable: this test
/// lives two directories below the repository root).
const SHIPPED_WASM: &[u8] = include_bytes!("../../../deploy/pkg/alkalive_runtime_wasm_bg.wasm");

#[test]
fn shipped_wasm_contains_no_credential_patterns() {
    // T-I4 invariant (Wave 1 §1.5 made the sweep a one-off; this makes it
    // a CI gate). ASCII-scan the module for credential-shaped substrings.
    let ascii: Vec<u8> = SHIPPED_WASM
        .iter()
        .copied()
        .filter(|&b| (0x20..=0x7e).contains(&b))
        .collect();
    let haystack = String::from_utf8_lossy(&ascii).into_owned();
    for pattern in [
        "ghp_",
        "github_pat_",
        "AKIA", // AWS access key id prefix
        "BEGIN RSA PRIVATE KEY",
        "BEGIN OPENSSH PRIVATE KEY",
        "BEGIN EC PRIVATE KEY",
        "api_key",
        "secret_key",
        "password=",
        "Authorization: Bearer",
        "sk-prod-", // OpenAI-style key prefix
    ] {
        assert!(
            !haystack.contains(pattern),
            "security regression: credential pattern `{pattern}` present in the shipped WASM"
        );
    }
}

// ---------------------------------------------------------------------------
// 2. Shader data-race invariant (SafeRace / T-E5)
// ---------------------------------------------------------------------------

#[test]
fn shader_sources_contain_no_data_race_constructs() {
    // WGSL: assert the four shipped sources declare no `var<storage>`,
    // no atomics, no workgroup storage, and no read-write textures. These
    // are the constructs that make the SafeRace DRSV class (data races
    // whose guardrails optimizers may remove) expressible at all; our
    // shaders are uniform/texture/sampler only.
    let wgsl_sources = [
        alkalive_backend_wgpu::wgsl_shaders::TEXT_VERTEX_WGSL,
        alkalive_backend_wgpu::wgsl_shaders::TEXT_FRAGMENT_WGSL,
        alkalive_backend_wgpu::wgsl_shaders::RECT_VERTEX_WGSL,
        alkalive_backend_wgpu::wgsl_shaders::RECT_FRAGMENT_WGSL,
    ];
    for (i, src) in wgsl_sources.iter().enumerate() {
        for forbidden in [
            "var<storage",
            "var<,storage",
            "atomic<",
            "atomicLoad",
            "atomicStore",
            "workgroup",
            "texture_storage_2d",
            ", read_write>",
            ",read_write>",
        ] {
            assert!(
                !src.contains(forbidden),
                "security regression: WGSL shader #{i} contains `{forbidden}` — \
                 data-race-capable construct (SafeRace class)"
            );
        }
    }

    // GLSL: the fallback shaders are vertex/fragment only (never compute),
    // so no shared writable memory can exist; pin that invariant too.
    let glsl_sources = [
        alkalive_backend_wgpu::VERTEX_SHADER_SRC,
        alkalive_backend_wgpu::FRAGMENT_SHADER_SRC,
        alkalive_backend_wgpu::RECT_VERTEX_SHADER_SRC,
        alkalive_backend_wgpu::RECT_FRAGMENT_SHADER_SRC,
    ];
    for (i, src) in glsl_sources.iter().enumerate() {
        for forbidden in [
            " SSBO",
            "layout(std430",
            "layout(std140",
            "compute",
            "atomic",
            "barrier(",
        ] {
            assert!(
                !src.contains(forbidden),
                "security regression: GLSL shader #{i} contains `{forbidden}` — \
                 storage/atomic construct"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 3. Vertex budget end-to-end (T-D1)
// ---------------------------------------------------------------------------

#[test]
fn oversized_scene_hits_vertex_budget_and_flags_truncation() {
    // A scene whose input text would tessellate past MAX_TEXT_VERTICES
    // (300k chars ⇒ ~300k glyph quads ⇒ ~1.8M vertices). The shaping and
    // rasterization must complete and the output must be BOUNDED at the
    // budget, with `truncated = true`.
    let scene = TextSceneData {
        text: "Hello World!".to_string(),
        input_text: "a".repeat(300_000),
        ..Default::default()
    };
    let t = tessellate_scene(&scene, 800.0, 600.0).expect("tessellation must succeed");
    assert!(
        t.vertices.len() <= MAX_TEXT_VERTICES,
        "vertex buffer must be bounded by the security budget: got {}",
        t.vertices.len()
    );
    assert!(t.truncated, "oversized scene must set the truncated flag");
    // The title prefix must still render (graceful degradation, not a
    // blank scene): title vertices are intact.
    assert!(t.title_vertex_count > 0);
}

#[test]
fn normal_scene_stays_under_budget_untruncated() {
    let scene = TextSceneData {
        text: "Hello World!".to_string(),
        input_text: "typed text".to_string(),
        ..Default::default()
    };
    let t = tessellate_scene(&scene, 800.0, 600.0).expect("tessellation must succeed");
    assert!(!t.truncated, "normal scenes must not be truncated");
    assert!(t.vertices.len() < MAX_TEXT_VERTICES);
}
