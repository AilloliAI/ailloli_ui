//! GPU upload helpers for glyph / atlas regions (`bytes_per_row` padding invariants).

/// Writes a tight RGBA8 rectangle into a texture, padding rows to 256 bytes.
///
/// `rgba` should contain `w * h * 4` bytes. Short data is zero-padded only when
/// row repacking is required; an already aligned short slice is passed directly
/// to wgpu and fails validation. Zero width produces a zero row pitch, which is
/// not a valid nonempty texture copy.
///
/// # Panics
///
/// Panics on arithmetic overflow, allocation failure, or a short source row
/// during the padding copy.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_render_wgpu::text::glyph_upload::write_subtexture_rgba;
/// fn upload(queue: &wgpu::Queue, texture: &wgpu::Texture) {
///     write_subtexture_rgba(queue, texture, wgpu::Origin3d::ZERO,
///         2, 2, &[255; 2 * 2 * 4]);
/// }
/// ```
pub fn write_subtexture_rgba(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    origin: wgpu::Origin3d,
    w: u32,
    h: u32,
    rgba: &[u8],
) {
    let unpadded_bpr = w * 4;
    let padded_bpr = unpadded_bpr.div_ceil(256) * 256;

    let data: Vec<u8>;
    let bytes = if padded_bpr == unpadded_bpr {
        rgba
    } else {
        data = pad_rows(rgba, unpadded_bpr as usize, padded_bpr as usize, h as usize);
        &data
    };

    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture,
            mip_level: 0,
            origin,
            aspect: wgpu::TextureAspect::All,
        },
        bytes,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(padded_bpr),
            rows_per_image: Some(h),
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
}

/// Copies tight rows into a zero-filled wider row pitch.
fn pad_rows(src: &[u8], src_bpr: usize, dst_bpr: usize, rows: usize) -> Vec<u8> {
    let mut out = vec![0u8; dst_bpr * rows];
    for row in 0..rows {
        let src_off = row * src_bpr;
        let dst_off = row * dst_bpr;
        let src_end = (src_off + src_bpr).min(src.len());
        out[dst_off..dst_off + (src_end - src_off)].copy_from_slice(&src[src_off..src_end]);
    }
    out
}
