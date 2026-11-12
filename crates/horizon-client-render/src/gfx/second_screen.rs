//! A surface on a display the game's window isn't on: the second screen of dual-screen handhelds
//! (the Android build's `game/second_screen.rs`). It draws HUD batches with the game's device and
//! the game's own HUD pipelines, so it takes the window's swapchain format.
use super::Renderer;
use crate::{Result, hud::HudBatch};

pub struct SecondScreen {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    config: wgpu::SurfaceConfiguration,
    /// For the HUD's 3D items, at the screen's size.
    depth: wgpu::TextureView,
    /// The title panorama under the next frame, looking this way (yaw and pitch, degrees).
    pub panorama: Option<[f32; 2]>,
}

impl Renderer {
    /// The instance to make the second screen's surface with: it must share this renderer's.
    /// (This crate forbids `unsafe`; making a surface on a raw native window needs it.)
    pub fn wgpu_instance(&self) -> &wgpu::Instance {
        &self.instance
    }

    /// `screen` was made with this renderer's device. A screen kept from an earlier renderer
    /// (the game started again, the graphics rebuilt) must be made again before drawing.
    pub fn owns(&self, screen: &SecondScreen) -> bool {
        screen.device == self.device
    }

    /// A second screen on `surface` (made with `wgpu_instance`), `size` in physical pixels.
    pub fn second_screen(
        &self,
        surface: wgpu::Surface<'static>,
        size: [u32; 2],
    ) -> Result<SecondScreen> {
        let caps = surface.get_capabilities(&self.adapter);
        // The HUD pipelines are built for the window's format.
        let format = self.config.format;
        if !caps.formats.contains(&format) {
            return Err(format!(
                "the second screen offers {:?}, not the game window's {format:?}",
                caps.formats
            )
            .into());
        }
        let opaque = wgpu::CompositeAlphaMode::Opaque;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: 0,
            height: 0,
            // Not Fifo: waiting for a slower lower panel (60 Hz under a 120 Hz window) would
            // hold back the game's frames.
            present_mode: [wgpu::PresentMode::Mailbox, wgpu::PresentMode::Immediate]
                .into_iter()
                .find(|mode| caps.present_modes.contains(mode))
                .unwrap_or(wgpu::PresentMode::Fifo),
            desired_maximum_frame_latency: 2,
            alpha_mode: if caps.alpha_modes.contains(&opaque) {
                opaque
            } else {
                caps.alpha_modes.first().copied().unwrap_or(opaque)
            },
            view_formats: self.config.view_formats.clone(),
            color_space: wgpu::SurfaceColorSpace::default(),
        };
        let mut screen = SecondScreen {
            depth: Self::depth(&self.device, &self.config),
            config,
            surface,
            device: self.device.clone(),
            panorama: None,
        };
        screen.resize(size);
        Ok(screen)
    }

    /// Clear the second screen to `clear`, draw `batch` (made with `HudBatch::new` at the
    /// screen's `size`) over it and present. False when the screen had no frame to give.
    pub fn draw_second_screen(
        &mut self,
        screen: &mut SecondScreen,
        batch: &HudBatch,
        clear: wgpu::Color,
    ) -> bool {
        let frame = match screen.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                screen.surface.configure(&screen.device, &screen.config);
                return false;
            }
            _ => return false,
        };
        let size = screen.size();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Second screen"),
            });
        // The window's frame was submitted already: its HUD vertices are on their way to the
        // GPU, so the HUD's buffers take this batch next, in queue order.
        self.hud
            .prepare(&self.device, &mut self.uploads, &mut encoder, batch, size);
        let target = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.config.format.remove_srgb_suffix()),
            ..Default::default()
        });
        // The title panorama first, as the window's title screen draws it. The window's frame
        // was submitted: its projection can take this screen's until the next one.
        let panorama = screen.panorama.take();
        if let Some([yaw, pitch]) = panorama {
            self.panorama
                .prepare(&self.queue, size[0], size[1], yaw, pitch);
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Second screen clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: panorama.map(|_| wgpu::RenderPassDepthStencilAttachment {
                view: &screen.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        if panorama.is_some() {
            self.panorama.draw(&mut pass);
        }
        drop(pass);
        self.hud
            .draw(&mut encoder, &target, &screen.depth, batch, size);
        self.uploads.finish(&encoder);
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
        true
    }
}

impl SecondScreen {
    /// In physical pixels.
    pub fn size(&self) -> [u32; 2] {
        [self.config.width, self.config.height]
    }

    pub fn resize(&mut self, size: [u32; 2]) {
        let max = self.device.limits().max_texture_dimension_2d;
        let [width, height] = size.map(|v| v.clamp(1, max));
        if [width, height] != self.size() {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
            self.depth = Renderer::depth(&self.device, &self.config);
        }
    }
}
