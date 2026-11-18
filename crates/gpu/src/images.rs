//! Bitmaps on the GPU (ADR 0032): each library image is uploaded once, as a straight RGBA8
//! texture, and kept for later frames under a memory budget (least recently used first out).
//!
//! The key is the image itself (the `Arc<Pixels>` the display list holds, compared by pointer,
//! and kept alive by the cache so its address can't be reused): a replaced or re-imported
//! bitmap is a new image and never shows a stale texture, and two open documents with the same
//! bitmap id never share one. Those textures are immutable once uploaded, so an entry evicted
//! while a submitted frame still samples it stays valid for that frame (wgpu keeps it alive).
//!
//! A pixel-layer plate is different (ADR 0113). It is keyed by its stamp, not by the pointer,
//! and the cache does not keep the plate's bytes. A later generation rewrites the dirty rows
//! of the same texture. Undo, which restores an older generation, uploads the whole plate again.

use crate::binning::GpuImage;
use crate::{GpuError, describe};
use anicraft_doc::{Pixels, plate_catch_up};
use anicraft_render::NeedsFull;
use std::collections::HashMap;
use std::sync::Arc;

/// Graphics memory the cached bitmaps may hold (4 bytes a pixel). One frame may need more (its
/// own images are never evicted while it binds them); the rest go first.
pub const IMAGE_BUDGET: u64 = 256 << 20;

struct Entry {
    /// Kept so the pointer stays this image's for as long as the entry lives.
    pixels: Arc<Pixels>,
    view: wgpu::TextureView,
    bytes: u64,
    used: u64,
}

struct PlateEntry {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    bytes: u64,
    used: u64,
    generation: u64,
    width: u32,
    height: u32,
}

#[derive(Default)]
pub(crate) struct Images {
    map: HashMap<usize, Entry>,
    plates: HashMap<u64, PlateEntry>,
    bytes: u64,
    clock: u64,
}

impl Images {
    /// Images and bytes held.
    pub(crate) fn held(&self) -> (usize, u64) {
        (self.map.len() + self.plates.len(), self.bytes)
    }

    /// Texture views for `want` (in order), uploading what isn't cached yet.
    pub(crate) fn views(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, max_edge: u32, want: &[GpuImage]) -> Result<Vec<wgpu::TextureView>, GpuError> {
        self.clock += 1;
        let now = self.clock;
        let mut out = Vec::with_capacity(want.len());
        for image in want {
            if let Some(plate) = image.plate {
                out.push(self.plate_view(device, queue, max_edge, &image.pixels, plate, now)?);
            } else {
                out.push(self.bitmap_view(device, queue, max_edge, &image.pixels, now)?);
            }
        }
        self.evict(now);
        Ok(out)
    }

    fn bitmap_view(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, max_edge: u32, px: &Arc<Pixels>, now: u64) -> Result<wgpu::TextureView, GpuError> {
        let key = Arc::as_ptr(px) as usize;
        if let Some(e) = self.map.get_mut(&key).filter(|e| Arc::ptr_eq(&e.pixels, px)) {
            e.used = now;
            return Ok(e.view.clone());
        }
        let (view, bytes) = upload(device, queue, max_edge, px)?;
        self.bytes += bytes;
        self.map.insert(key, Entry { pixels: px.clone(), view: view.clone(), bytes, used: now });
        Ok(view)
    }

    fn plate_view(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, max_edge: u32, px: &Pixels, plate: anicraft_doc::PlateDraw, now: u64) -> Result<wgpu::TextureView, GpuError> {
        let (w, h) = (px.width, px.height);
        if let Some(e) = self.plates.get_mut(&plate.stamp)
            && e.width == w
            && e.height == h
        {
            // A render that started earlier must not paint its old bytes over a stroke
            // that has already moved on. Undo takes a new stamp, so it still uploads.
            if plate.generation < e.generation {
                e.used = now;
                return Ok(e.view.clone());
            }
            if e.generation == plate.generation {
                e.used = now;
                return Ok(e.view.clone());
            }
            if plate.generation > e.generation && plate.y1 > plate.y0 {
                write_rows(queue, &e.texture, px, plate.y0, plate.y1)?;
                e.generation = plate.generation;
                e.used = now;
                let view = e.view.clone();
                plate_catch_up(plate.stamp, plate.generation);
                return Ok(view);
            }
        }
        if let Some(e) = self.plates.remove(&plate.stamp) {
            self.bytes = self.bytes.saturating_sub(e.bytes);
        }
        let (view, texture, bytes) = upload_plate(device, queue, max_edge, px)?;
        plate_catch_up(plate.stamp, plate.generation);
        self.bytes += bytes;
        self.plates.insert(plate.stamp, PlateEntry { texture, view: view.clone(), bytes, used: now, generation: plate.generation, width: w, height: h });
        Ok(view)
    }

    fn evict(&mut self, now: u64) {
        while self.bytes > IMAGE_BUDGET {
            let bitmap = self.map.iter().filter(|(_, e)| e.used != now).min_by_key(|(_, e)| e.used).map(|(k, e)| (*k, e.used));
            let plate = self.plates.iter().filter(|(_, e)| e.used != now).min_by_key(|(_, e)| e.used).map(|(k, e)| (*k, e.used));
            match (bitmap, plate) {
                (Some((key, a)), Some((_, b))) if a <= b => {
                    if let Some(e) = self.map.remove(&key) {
                        self.bytes = self.bytes.saturating_sub(e.bytes);
                    }
                }
                (Some((key, _)), None) => {
                    if let Some(e) = self.map.remove(&key) {
                        self.bytes = self.bytes.saturating_sub(e.bytes);
                    }
                }
                (_, Some((stamp, _))) => {
                    if let Some(e) = self.plates.remove(&stamp) {
                        self.bytes = self.bytes.saturating_sub(e.bytes);
                    }
                }
                (None, None) => break,
            }
        }
    }
}

/// One image as a texture, in its own error scopes: a failed upload is never cached.
fn upload(device: &wgpu::Device, queue: &wgpu::Queue, max_edge: u32, px: &Pixels) -> Result<(wgpu::TextureView, u64), GpuError> {
    let (view, _texture, bytes) = upload_plate(device, queue, max_edge, px)?;
    Ok((view, bytes))
}

fn upload_plate(device: &wgpu::Device, queue: &wgpu::Queue, max_edge: u32, px: &Pixels) -> Result<(wgpu::TextureView, wgpu::Texture, u64), GpuError> {
    let (w, h) = (px.width, px.height);
    if w == 0 || h == 0 || w > max_edge || h > max_edge {
        return Err(GpuError::NeedsFull(NeedsFull::Bitmaps));
    }
    let len = w as usize * h as usize * 4;
    // A short buffer reads as transparent past its end, as `Pixels::at` does.
    let padded;
    let bytes: &[u8] = match px.rgba.get(..len) {
        Some(b) => b,
        None => {
            let mut v = px.rgba.clone();
            v.resize(len, 0);
            padded = v;
            &padded
        }
    };
    let scopes = crate::Scopes::push(device);
    let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("anicraft-bitmap"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        bytes,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(h) },
        size,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let (oom, other) = scopes.pop_split();
    if let Some(e) = other {
        return Err(GpuError::Device(describe(&e)));
    }
    if let Some(e) = oom {
        return Err(GpuError::TooLarge(format!("no room on the GPU for a {w}×{h} bitmap: {e}")));
    }
    Ok((view, texture, len as u64))
}

/// Replace rows `y0..y1` of an existing plate texture. The copy is padded so each row starts
/// on a 256-byte boundary, and it does not borrow the live plate after it returns.
fn write_rows(queue: &wgpu::Queue, texture: &wgpu::Texture, px: &Pixels, y0: u32, y1: u32) -> Result<(), GpuError> {
    let y0 = y0.min(px.height);
    let y1 = y1.min(px.height).max(y0);
    if y1 <= y0 {
        return Ok(());
    }
    let row = (px.width as usize).saturating_mul(4);
    let stride = row.div_ceil(256).saturating_mul(256).max(256);
    let rows = (y1 - y0) as usize;
    let Some(bytes) = rows.checked_mul(stride) else {
        return Err(GpuError::NeedsFull(NeedsFull::Bitmaps));
    };
    let mut padded = vec![0u8; bytes];
    for i in 0..rows {
        let src_at = ((y0 as usize) + i).saturating_mul(row);
        let dst_at = i * stride;
        if let (Some(src), Some(dst)) = (px.rgba.get(src_at..src_at + row), padded.get_mut(dst_at..dst_at + row)) {
            dst.copy_from_slice(src);
        }
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture, mip_level: 0, origin: wgpu::Origin3d { x: 0, y: y0, z: 0 }, aspect: wgpu::TextureAspect::All },
        &padded,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(stride as u32), rows_per_image: Some(rows as u32) },
        wgpu::Extent3d { width: px.width, height: rows as u32, depth_or_array_layers: 1 },
    );
    Ok(())
}
