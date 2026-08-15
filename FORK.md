# Bevy Fork Charter

Roughly 540 lines across 46 files separate this fork from upstream Bevy. Almost all of it serves
one goal: **the same bytes should exist once, not many times.**

This document records what diverges, the principles behind it, and the invariants that must hold
for the next person who merges upstream.

---

## Contents

- [The thesis](#the-thesis)
- [Design principles](#design-principles)
- [Invariants](#invariants)
- [Subsystems](#subsystems)
- [Merging upstream](#merging-upstream)
- [Gaps and omissions](#gaps-and-omissions)

---

## The thesis

Bevy's asset pipeline is written for the common case: read a file into a `Vec<u8>`, hand it to a
loader, drop it. That is a reasonable default, but it copies every asset at least twice — once out
of the reader, once into whatever structure holds it — and it copies embedded assets that were
already sitting in the binary as `'static` data and never needed copying at all.

This fork makes byte ownership explicit. Readers can hand back borrowed static data; `Image`,
`AudioSource`, and the glTF buffer set hold copy-on-write payloads; and identical assets referenced
from different files resolve to one handle instead of being decoded per-file. Everything else in
the fork — the glTF changes, the render-layer filtering, the extra `Hash` derives — is either a
consequence of that, or a targeted fix for an upstream behaviour that broke this workload.

| Branch                | Upstream base        | Fork delta               |
| --------------------- | -------------------- | ------------------------ |
| `release-0.16.1`      | `v0.16.1`            | 46 files, +481 / −147    |
| `release-0.17.3`      | `v0.17.3`            | 47 files, +473 / −190    |
| `release-0.18.1`      | `v0.18.1`            | 44 files, +481 / −201    |
| `release-0.19.1`      | `v0.19.1`            | 46 files, +537 / −227    |
| `merge-upstream-main` | `main` (0.20.0-dev)  | 46 files, +540 / −224    |

Each branch is *upstream tag + fork commits*, linear. There are no merge commits from upstream
release branches, and that is deliberate — see [Merging upstream](#merging-upstream).

---

## Design principles

These govern new changes, they do not merely describe old ones.

### P1 — Don't copy bytes that never change

Asset payloads are written once and read many times. Where a byte buffer is loaded and then only
read, it should be `Cow<'static, [u8]>`, not `Vec<u8>` — so embedded and in-memory sources can be
borrowed rather than duplicated. Mutation is still allowed; it just becomes explicit via
`to_mut()`, which is where the copy happens.

### P2 — Resolve identical assets to one handle

Two glTFs referencing the same texture URI should share a decoded image, not decode it twice. This
drives absolute-URI handling in the glTF loader and handle-only labeled assets in `bevy_asset`: a
load context can register a label that points at an asset another context owns.

### P3 — Extend additively; keep call sites boring

New capability arrives as defaulted trait methods (`read_to_cow`, `as_static_bytes`) so every
upstream implementor keeps compiling untouched. Where a type must widen, prefer widenings that make
the typical call site a one-token change — `Some(data)` becomes `Some(data.into())`, and nothing
else moves.

### P4 — Take the slower path when the fast one is unsound

Upstream loads glTF textures in parallel on an `IoTaskPool` scope. Under concurrent glTF loads that
path overflows the stack ([bevyengine/bevy#15271](https://github.com/bevyengine/bevy/issues/15271)).
The fork loads serially and accepts the throughput loss. Correctness under this workload outranks
matching upstream's performance profile.

### P5 — Re-express the change in upstream's current shape

When upstream restructures the code a fork change lives in, port the *intent* into the new
structure rather than preserving the old structure alongside it. This is the single reason the
delta has stayed near 46 files across four major versions instead of compounding. If upstream has
solved the problem more thoroughly, delete the fork's version and say so.

---

## Invariants

Rules the fork's code depends on. Breaking one compiles fine and fails at runtime or on another
target. Invariants marked **⚠** have actually been violated before.

### ⚠ INV-1 — `as_static_bytes` returns `Some` only for genuinely `'static` bytes

The signature hands out `&'static [u8]` on a safe trait, so the implementor carries the whole
guarantee. Only `DataReader` (backed by `MemoryAssetReader`'s `Value::Static`) implements it today.
Anything reading from disk, network, or a temporary buffer must leave the default `None`.

### INV-2 — A `Cow::Borrowed` from `read_to_cow` is shared, immutable data

Callers must not assume they own it. Mutating requires `to_mut()`, which clones — so hoist that
call out of hot loops rather than paying per iteration.

### INV-3 — `Image::data` mutation goes through `to_mut()`

`Image::data` is `Option<Cow<'static, [u8]>>`. This applies to every in-place pixel edit: `resize`,
`pixel_bytes_mut`, atlas builders, tilemap chunk updates. Reads should use `as_ref()` and stay
borrowed.

### ⚠ INV-4 — `LabeledAsset::asset == None` means "owned elsewhere" — skip it, never unwrap

Every consumer must treat the empty payload as a valid state: `AssetInfos::process_asset_load`
skips it, the saver and transformer accessors return `None`, and the type-mismatch diagnostic falls
back to a placeholder name. An `unwrap()` here panics only for assets that use cross-context
sharing.

### INV-5 — Labeled-asset indices stay aligned; never filter or compact the vector

Since 0.19, `label_to_asset_index` and `asset_id_to_asset_index` index into `labeled_assets`. A
payload-less entry still occupies its slot — that is why `LabeledSavedAsset::asset` is also an
`Option` rather than the entry being omitted.

### INV-6 — A handle-only labeled asset emits no load event from the registering context

`add_labeled_asset_handle` stores a strong handle, so the asset stays alive — but because
`process_asset_load` skips it, nothing waiting on that label will be notified by *this* load. The
owning context is responsible for the event.

### INV-7 — `CowArc::Static` wraps only genuinely static data

Same contract as INV-1, one layer up. `AudioLoader` maps `Cow::Borrowed → CowArc::Static` and
`Cow::Owned → CowArc::Owned`; that mapping is only sound because INV-1 holds.

### INV-8 — A fog volume renders for a view only when their `RenderLayers` intersect

Absent `RenderLayers` on either side means `RenderLayers::default()` (layer 0), so untagged volumes
and untagged cameras still see each other — the change is backward compatible for scenes that never
opt in. Both sides are compared by reference against one shared default, which keeps the
`as_ptr()` fast path in `RenderLayers::intersects` working; do not reintroduce `.cloned()` here.

Volumetric *lights* are a separate matter and are not the fork's concern: `bevy_pbr::render::light`
extracts `RenderLayers` for every light itself, and filters directional lights against the view's
layers before setting the VOLUMETRIC flag. Point and spot lights are not layer-filtered anywhere in
Bevy, since clustering has no notion of layers.

### ⚠ INV-9 — glTF textures load serially

Do not restore the `IoTaskPool` path without fixing #15271 first. This one is load-bearing and easy
to lose: upstream rewrites this block regularly, and a careless merge silently restores the
parallel path. Removing the serial loop also strands the `#[cfg(not(target_arch = "wasm32"))]`
attribute that guarded the now-unused `IoTaskPool` import — which has already broken the wasm32
build once.

### INV-10 — `Gltf::lights` records spot lights only

Directional and point lights spawn normally but are not tracked. The map exists so spot lights can
be re-adjusted after load; this is intentional, and the field documentation says so.

### INV-11 — `Mesh3d` and `MeshMaterial3d` hash by asset id

The added `Hash` derives delegate to the inner `Handle`, so two components referencing the same
asset hash and compare equal. That is what makes them usable as batching or cache keys — do not add
fields to these newtypes without revisiting it.

---

## Subsystems

### Zero-copy asset reads — `bevy_asset::io`

Two defaulted methods on the `Reader` trait. Nothing upstream needs to change; readers that can
expose static memory override `as_static_bytes` and get the fast path for free.

```rust
// crates/bevy_asset/src/io/mod.rs — added to trait Reader
fn read_to_cow<'a>(&'a mut self)
    -> Pin<Box<dyn Future<Output = io::Result<Cow<'static, [u8]>>> + 'a + Send>> {
    // borrow if the source is static, otherwise read into an owned Vec
}

fn as_static_bytes(&self) -> Option<&'static [u8]> { None }   // see INV-1
```

`LoadContext::read_asset_bytes` widened from `Vec<u8>` to `Cow<'static, [u8]>`, and every real
`AssetLoader` in the workspace now calls `read_to_cow()`: image, HDR, EXR, glTF, shader, font,
animation graph, audio, world serialization, plus test and example loaders.

`read_to_cow` returns a `StackFuture`, not a boxed future, so a read costs no allocation of its
own — matching upstream's `read_to_end`.

Borrowed bytes must survive all the way into the asset or the saving is undone at the last step.
Three additive constructors exist for that, and new asset types holding bulk bytes should follow
the pattern:

| Constructor | Borrowed path |
| ----------- | ------------- |
| `Shader::from_wgsl` / `from_wesl` / `from_spirv` | `Source` holds `Cow<'static, _>` directly |
| `Image::new_cow` | stores the `Cow` as-is |
| `Font::from_cow` | wraps static bytes in an `Arc`, no copy |

Each is additive — the owned-`Vec` constructor keeps its signature. Widening the existing
constructor to `impl Into<Cow<..>>` was tried for `Image::new` and reverted: the generic parameter
breaks inference at call sites passing `.collect()`, which contradicts P3.

### CowArc — `bevy_utils`

A fork-local copy of `atomicow::CowArc` extended with `From<Arc<T>>` and `From<Box<T>>`, used by
`AudioSource::bytes` so audio data can be shared across threads without re-allocating. `Static` is a
distinct variant from `Borrowed` precisely so static data never decays into an allocation.

> **Known wart.** Two `CowArc` types now coexist: `bevy_asset` uses `atomicow::CowArc`, `bevy_audio`
> uses `bevy_utils::CowArc`. They are structurally identical but distinct types. Consolidating means
> either upstreaming the two extra `From` impls to `atomicow`, or converting `bevy_audio` back and
> losing them.

### Handle-only labeled assets — `bevy_asset`

`LabeledAsset::asset` became `Option<ErasedLoadedAsset>`, and `LoadContext::add_labeled_asset_handle`
registers a label whose payload lives in another context. This is what lets one glTF reference a
texture another glTF owns without re-decoding it. Consequences are spelled out in INV-4, INV-5 and
INV-6.

### Copy-on-write images — `bevy_image` and dependents

`Image::data` is `Option<Cow<'static, [u8]>>`. This is the widest-reaching change by call-site
count: KTX2, Basis, tonemapping LUTs, atlas builders, GPU image preparation, tilemap chunks, custom
cursors, and two large-scene examples all touch it. Nearly every site is a mechanical `.into()` or
`.to_mut()`.

### glTF loader — `bevy_gltf`

| Change | Rationale | Status |
| ------ | --------- | ------ |
| Buffers are `Cow<'static, [u8]>` | P1; propagates into the `GltfExtensionHandler` trait signature | carried |
| Absolute `://` URIs load via the asset server | P2 — shares one source across glTFs instead of resolving relative per file | carried |
| Serial texture loading | P4 — upstream's parallel path overflows the stack (#15271) | carried |
| `Gltf::lights` spot-light map | Lets spot lights be re-adjusted after load | carried |
| `://` handling in `gltf_ext::texture_handle` | Upstream deleted the function in 0.18; behaviour now comes from `load_image` | **dropped** |

### Volumetric fog render layers — `bevy_pbr`

`VolumetricFog` and `FogVolume` now extract an optional `RenderLayers`, and uniform preparation
skips volume/view pairs whose layers don't intersect. Without this, every fog volume renders into
every camera. See INV-8 for the default-layer behaviour and for how volumetric *lights* are handled
(upstream already covers them for directional lights).

### Hash derives

`Mesh3d` and `MeshMaterial3d` gained `Hash` so they can key maps — see INV-11. The equivalent
derives on `SceneRoot` and `DynamicSceneRoot` were **dropped**: both types ceased to exist in 0.19's
BSN rewrite.

---

## Merging upstream

Bevy's release tags are **siblings off `main`, not a chain**. `v0.16.1` is not an ancestor of
`v0.17.0`; each release branches separately. Merging one release branch into another therefore
picks a merge base from before the earlier release shipped — merging `v0.17.3` into the 0.16.1
branch produced **241 conflicts**, almost all upstream-versus-upstream noise with nothing to do with
this fork.

Replaying the fork delta onto the new tag instead produces roughly ten, all genuinely fork-related.
That is the procedure:

```bash
# 1. capture the delta from the current checkpoint
git diff v0.19.1 release-0.19.1 > fork-delta.patch

# 2. branch from the new upstream point
git checkout -b release-0.20.0 v0.20.0

# 3. replay with 3-way merge; exclude paths upstream has deleted
git apply -3 --exclude=<deleted/path> fork-delta.patch

# 4. resolve, then verify — including the targets the default build misses
cargo check --workspace
cargo check -p bevy_gltf --target wasm32-unknown-unknown
cargo check -p bevy_utils --no-default-features        # no_std
cargo check -p bevy_image --features serialize         # BRP image transfer
cargo check -p bevy_asset --features embedded_watcher
cargo check -p bevy_image --features exr
cargo check -p bevy_pbr --features meshlet
cargo check --features "file_watcher asset_processor" --example asset_processing
```

> **Note.** `git apply -3` is atomic. If any path in the patch no longer exists upstream, the entire
> patch is rejected with no partial application — hence the `--exclude` flags. Check for deleted
> paths first rather than diagnosing an apparent no-op.

### What has been re-homed so far

| Fork change | Moved to | Release |
| ----------- | -------- | ------- |
| `bevy_render/render_resource/shader.rs` | `bevy_shader/src/shader.rs` | 0.17 |
| `bevy_render/mesh/components.rs` | `bevy_mesh/src/components.rs` | 0.17 |
| `bevy_winit/custom_cursor.rs` | `bevy_winit/src/cursor/custom_cursor.rs` | 0.17 |
| `bevy_render::view::RenderLayers` | `bevy_camera::visibility::RenderLayers` | 0.17 |
| `AssetServer::send_loaded_asset` | `AssetInfos::process_asset_load` | 0.18 |
| `AssetInfos::path_to_id` | `path_to_index`, then `TypeIdHashMap` | 0.18, 0.20-dev |
| Labeled assets as a `HashMap` | Index-based `Vec` + two side maps | 0.19 |

### What upstream has superseded

Three fork changes were deleted rather than ported, because upstream solved the same problem:

- `AssetProcessor`'s `read_to_cow` — 0.18 streams the asset hash and hands `ProcessContext` a
  `Reader`, which is strictly better than the fork's buffering.
- The RON scene loader's `read_to_cow` and the `SceneRoot` hashes — 0.19 removed both types.
- The GLSL arms of `ShaderLoader` — 0.20-dev supports only spv, wgsl and wesl.

---

## Gaps and omissions

### Verification coverage

Everything here is verified with `cargo check` only. No tests have been run and nothing has been
executed at runtime. The render-layer filtering, the serial texture loading, and the zero-copy paths
are *type-checked, not behaviour-checked*.

### Feature-gated code is where bugs hide

The default build does not compile `exr`, `meshlet`, `embedded_watcher`, `file_watcher`,
`asset_processor`, or the wasm32 target. Three real defects lived in exactly those gaps:

- Three `AssetLoader::load` impls carried a fifth `bytes` parameter the trait never declared —
  `E0050` the moment `exr`, `meshlet` or `asset_processor` was enabled.
- `embedded_watcher` called `read_to_cow()` on a `std::io::BufReader`, which does not implement the
  asset `Reader` trait. That feature had been broken since the change was first written against 0.16.
- Removing the unused `IoTaskPool` import stranded its `#[cfg(not(target_arch = "wasm32"))]`
  attribute onto the next import, breaking the wasm32 build while native builds stayed green.

A later audit found two more in the same blind spot: `bevy_image --features serialize` failed
because `SerializedImage` still declared `Vec<u8>`, and `bevy_utils --no-default-features` failed
because `cow_arc.rs` imported `std` in a `#![no_std]` crate.

All five are fixed. The lesson is the verification list in
[Merging upstream](#merging-upstream): the default `cargo check` is not sufficient evidence for
this fork. Every defect found in this fork to date has been in code the default build skips.

### Dead weight removed

`bevy_render/src/texture/image.rs` — 1,059 lines carried forward through several merges — had no
`mod` declaration, no references anywhere, and 0.14-era imports that would not have compiled. It was
never part of the build. Deleted.

### Inert changes dropped

Two fork changes were verified to have no effect and removed rather than carried further: a
`camera_texture_usage` map in `core_3d` that was written but never read, and a `bevy_remote` rewrite
of `into_values()` to `values()` plus a clone, which produced identical output with an extra
allocation.

---

*Current at `merge-upstream-main`, upstream `main` @ 0.20.0-dev. Verified with rustc 1.97.1. Line
counts are `git diff --shortstat` against each branch's upstream base.*
