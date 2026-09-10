# ailloli_ui_render_wgpu

Primary WGPU renderer for retained Ailloli UI scenes, text, icons, layers, and
capture output.

This beta crate targets Rust 1.88. Most applications receive it through the
default `winit` feature of the `ailloli_ui` façade.

[API documentation](https://ailloliai.github.io/ailloli_ui/ailloli_ui_render_wgpu/) ·
[Repository](https://github.com/AilloliAI/ailloli_ui)

## Host-owned composition (unreleased)

The source tree supports embedding UI in an application-owned WGPU frame. This
API is intended for the next beta; it is not in the published `0.1.0-beta.2`
crate. No Cargo version or WGPU dependency update is required to develop it.

Use `Renderer::new_with_render_context` with your device and queue, then borrow
them through `Renderer::device()` and `Renderer::queue()`. Their ownership stays
with the renderer. The host continues to own its surface, acquired image, command
encoder, submission schedule, and presentation. Ailloli's retained scene commands
stay UI-only; the host's drawing does not become part of `DrawCmd`.

The frame order is:

1. Acquire or choose the host image and create an encoder on the shared device.
2. Record the host pass and end it.
3. Call `record_layered_to_target_scaled` with `TargetLoadOp::Load`.
4. Finish the encoder, submit it on the shared queue, then present if applicable.

For example, after creating a compatible renderer and target:

```rust,no_run
use ailloli_ui_core::{Color, Rect, Scale};
use ailloli_ui_render_wgpu::{
    BorrowedRenderTarget, LayerPass, PhysicalExtent, Renderer, TargetLoadOp,
    TargetRecordingError,
};
use ailloli_ui_runtime::{scene::DrawRect, DrawCmd};

fn frame(
    ui: &mut Renderer,
    texture: &wgpu::Texture,
    view: &wgpu::TextureView,
) -> Result<(), TargetRecordingError> {
    let mut encoder = ui.device().create_command_encoder(&Default::default());
    {
        let _host_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("host background"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.02, g: 0.04, b: 0.08, a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    }
    let commands = [DrawCmd::Rect(DrawRect {
        rect: Rect::new(16.0, 16.0, 80.0, 24.0),
        color: Color::WHITE,
    })];
    ui.record_layered_to_target_scaled(
        &mut encoder,
        BorrowedRenderTarget {
            view,
            texture: Some(texture),
            size: PhysicalExtent::new(texture.width(), texture.height()),
            format: texture.format(),
        },
        &[LayerPass::new(&commands)],
        Scale::new(1.0),
        TargetLoadOp::Load,
    )?;
    ui.queue().submit([encoder.finish()]);
    Ok(()) // The host can now present its surface image.
}
```

Run the complete GPU example from this source workspace:

```sh
cargo run -p ailloli_ui_render_wgpu --example host_composition --locked
```

It draws a host-owned triangle plus retained UI text into one offscreen image,
submits one encoder, checks pixels, and writes
`target/host_composition/host_composition.png`. It requires a WGPU adapter, but not a
window server. This proves image composition, not native presentation.

### Target and load policies

- Use a single-sampled `D2` color view covering mip zero and array layer zero
  only, with `RENDER_ATTACHMENT` usage. MSAA resolve attachments, subrect views,
  format reinterpretation, and array/cube views are not supported here.
- Supply full physical dimensions, both nonzero and no larger than the device's
  `max_texture_dimension_2d`. The format must match the renderer pipelines.
- DPR must be finite and positive. At `1.0`, 100 logical units occupy 100 pixels;
  at `2.0`, they occupy 200. Target dimensions are always physical pixels.
- `Load` preserves existing pixels. Initialize the target before the UI call,
  either in an earlier submission or an earlier pass of the same encoder.
- `Clear(color)` clears the entire target, including when no UI layer draws.
  It also clears before a first-layer backdrop reads its background. Ordinary
  rendering keeps its clear in the main pass rather than adding another pass.
- Ordinary UI and foreground blur can use `texture: None`. Backdrop blur and
  `Multiply`/`Screen` blends require the matching texture with `COPY_SRC` usage.
- Resizing a target reuses pipelines and atlases; only a differently sized
  stencil attachment is recreated. It does not resize the host window or surface.

Wgpu does not expose a texture view's descriptor for inspection. The host must
guarantee the view's actual device, extent, format, and identity. A supplied
texture descriptor is checked, but cannot prove that the view belongs to it.
See the pinned [WGPU view contract](https://docs.rs/wgpu/0.20.1/wgpu/struct.TextureViewDescriptor.html).

### Submission and errors

The recorder never acquires, submits, or presents. It can enqueue glyph/icon
uploads on the shared queue. Submit the current UI encoder before recording
another UI frame with the same renderer, including managed rendering or capture.
Do not record several UI frames and submit them together: later uploads and
pooled-resource reuse can overwrite resources referenced by earlier commands.
Correctly ordered queue submissions do not require a CPU wait between frames.

`TargetRecordingError` distinguishes a missing effect source from an invalid
target descriptor or scale. A returned error leaves the encoder and UI caches
untouched. Invalid GPU handles and malformed layer plans can instead trigger
wgpu validation or a panic; those failures are not transactional. The existing
`RendererError` enum and managed render method signatures remain unchanged.

The GPU contract tests are opt-in:

```sh
cargo test -p ailloli_ui_render_wgpu --lib host_composition_tests --locked \
  -- --ignored --nocapture --test-threads=1
```

Dual-licensed under Apache-2.0 or MIT.
