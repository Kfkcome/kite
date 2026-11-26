# tern patch to gpui-pre-apple 0.3.8

Reduces steady-state and animated Metal memory footprint on macOS without changing any public API.

## Differences

`build.rs`
- When `runtime_shaders` is not enabled, checks whether `xcrun -sdk <sdk> --find metal` and `metallib`
  are installed. If present (Xcode / CI release runners), compiles `shaders.metallib` at build time
  and emits `cfg(gpui_precompiled_shaders)`. On Command-Line-Tools-only machines, emits a warning
  and falls back to stitched runtime shaders instead of failing the build.

`src/metal_renderer.rs`
- Wraps Metal shader library creation and pipeline state compilation in `objc2::rc::autoreleasepool`
  so temporary compiler / descriptor Objective-C objects are released immediately.
- `CAMetalLayer`: sets `maximum_drawable_count` to `2` (double buffering) instead of `3`, saving one
  full-window `IOSurface` (~18 MB at 2x) during and after short UI animations.
- `path_intermediate_texture` / `path_intermediate_msaa_texture`: allocated lazily only when a frame
  contains non-empty `PrimitiveBatch::Paths` (tracked by `PathIntermediateTextures`), and marked
  `MTLPurgeableState::Volatile` in the command buffer completion handler once all in-flight path
  frames finish, removing ~18 MB from `phys_footprint` on idle and path-free terminal frames.
- `InstanceBufferPool`: starts at `512 KiB` (still doubles on demand up to `MAX_INSTANCE_BUFFER_SIZE`),
  caps pooled buffers at `2`, and marks pooled buffers `MTLPurgeableState::Volatile` while idle.

`src/metal_atlas.rs`
- Lowers `DEFAULT_ATLAS_SIZE` from `1024x1024` to `512x512` (polychrome starts at 1 MB instead of
  4 MB; monochrome at 256 KB instead of 1 MB). Oversized textures still allocate at `min_size` and
  free when their last tile is removed.
