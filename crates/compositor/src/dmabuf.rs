//! Apps draw on the GPU (roadmap M5.19): `zwp_linux_dmabuf_v1`, version 4
//! with feedback, so Mesa's EGL and Vulkan in apps render on the GPU and
//! hand their buffers over without a copy; shared memory stays for the
//! rest. The DRM backend offers the formats its renderer can import, on the
//! GPU's render node, and checks each buffer by importing it once.

use std::rc::Rc;

use smithay::backend::allocator::Format;
use smithay::backend::allocator::format::FormatSet;
use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::delegate_dmabuf;
use smithay::reexports::wayland_protocols::wp::linux_dmabuf::zv1::server::zwp_linux_dmabuf_feedback_v1::TrancheFlags;
use smithay::wayland::dmabuf::{
    DmabufFeedback, DmabufFeedbackBuilder, DmabufGlobal, DmabufHandler, DmabufState,
    ImportNotifier,
};

use crate::state::Edel;

/// How the backend checks a client's buffer: true when its renderer can
/// import it.
pub type Importer = Rc<dyn Fn(&Dmabuf) -> bool>;

/// The protocol: its state always, its global and the backend's check once
/// a backend offers it.
pub struct Dmabufs {
    pub state: DmabufState,
    /// Kept so the global lives as long as the compositor
    pub _global: Option<DmabufGlobal>,
    pub import: Option<Importer>,
    /// Whether a window has handed over a buffer yet, said once in the log
    pub seen: bool,
}

impl Default for Dmabufs {
    fn default() -> Self {
        Dmabufs {
            state: DmabufState::new(),
            _global: None,
            import: None,
            seen: false,
        }
    }
}

/// What the GPU's buffers are offered for (M5.19), kept so each screen can
/// make its own [`Feedbacks`] (M7.7).
#[derive(Clone)]
pub struct Offer {
    /// The render node's device number, where apps draw.
    pub device: u64,
    pub formats: Vec<Format>,
}

/// A screen's two feedbacks (M7.7): `render`, the default, and `scanout`,
/// which first offers the formats the screen's primary plane shows
/// itself, on the display device, so a fullscreen window that takes one
/// goes straight to the screen without being drawn again.
pub struct Feedbacks {
    pub render: DmabufFeedback,
    pub scanout: DmabufFeedback,
}

impl Offer {
    /// The feedbacks for a screen whose primary plane shows `plane`, on the
    /// display device `display`; none when no format is both drawn and shown.
    pub fn feedbacks(&self, display: u64, plane: &FormatSet) -> Option<Feedbacks> {
        let shown: Vec<Format> = self
            .formats
            .iter()
            .filter(|format| plane.contains(format))
            .copied()
            .collect();
        if shown.is_empty() {
            return None;
        }
        let builder = DmabufFeedbackBuilder::new(self.device, self.formats.clone());
        let render = builder.clone().build().ok()?;
        let scanout = builder
            .add_preference_tranche(display, Some(TrancheFlags::Scanout), shown)
            .build()
            .ok()?;
        Some(Feedbacks { render, scanout })
    }
}

impl Edel {
    /// Offers `zwp_linux_dmabuf_v1` for `formats` on the GPU `device` (a
    /// render node's device number), checking buffers with `import`; with
    /// no format to offer it says so and offers nothing.
    pub fn offer_dmabuf(&mut self, device: u64, formats: Vec<Format>, import: Importer) {
        if formats.is_empty() {
            eprintln!(
                "edel-compositor: the renderer imports no GPU buffers; apps draw in shared memory"
            );
            return;
        }
        let count = formats.len();
        let feedback = match DmabufFeedbackBuilder::new(device, formats).build() {
            Ok(feedback) => feedback,
            Err(e) => {
                eprintln!("edel-compositor: no GPU buffers for apps: {e}");
                return;
            }
        };
        let global = self
            .dmabuf
            .state
            .create_global_with_default_feedback::<Edel>(&self.display, &feedback);
        self.dmabuf._global = Some(global);
        self.dmabuf.import = Some(import);
        eprintln!("edel-compositor: apps may hand over GPU buffers in {count} formats");
    }
}

impl DmabufHandler for Edel {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf.state
    }

    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        dmabuf: Dmabuf,
        notifier: ImportNotifier,
    ) {
        let imported = self
            .dmabuf
            .import
            .as_ref()
            .is_some_and(|import| import(&dmabuf));
        if imported {
            if !self.dmabuf.seen {
                self.dmabuf.seen = true;
                eprintln!("edel-compositor: a window draws on the GPU (dmabuf)");
            }
            let _ = notifier.successful::<Edel>();
        } else {
            notifier.failed();
        }
    }
}

delegate_dmabuf!(Edel);
