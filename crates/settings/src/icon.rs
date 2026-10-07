//! Our own icons in GTK (M5.6a): each one of `design/icons/`, drawn once
//! by `edel::icons`, the code the compositor and shell-ui draw theirs
//! with, and painted by GTK in the colour of the text around it, as GTK
//! paints its symbolic icons. So a selected row, a dimmed button and the
//! dark scheme recolour it with no code here, and a changed file changes
//! every place that shows it.

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene};

mod imp {
    use std::cell::{Cell, OnceCell};

    use super::*;

    #[derive(Default)]
    pub struct Icon {
        /// The shape, white, its alpha how much of each pixel it covers.
        pub mask: OnceCell<gdk::Texture>,
        /// Its size in logical pixels.
        pub size: Cell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Icon {
        const NAME: &'static str = "EdelIcon";
        type Type = super::Icon;
        type Interfaces = (gdk::Paintable, gtk::SymbolicPaintable);
    }

    impl ObjectImpl for Icon {}

    impl PaintableImpl for Icon {
        fn intrinsic_width(&self) -> i32 {
            self.size.get()
        }

        fn intrinsic_height(&self) -> i32 {
            self.size.get()
        }

        fn snapshot(&self, snapshot: &gdk::Snapshot, width: f64, height: f64) {
            self.paint(snapshot, width, height, &gdk::RGBA::BLACK);
        }
    }

    impl SymbolicPaintableImpl for Icon {
        fn snapshot_symbolic(
            &self,
            snapshot: &gdk::Snapshot,
            width: f64,
            height: f64,
            colors: &[gdk::RGBA],
        ) {
            // The first is the text's colour where the icon sits.
            self.paint(
                snapshot,
                width,
                height,
                colors.first().unwrap_or(&gdk::RGBA::BLACK),
            );
        }
    }

    impl Icon {
        fn paint(&self, snapshot: &gdk::Snapshot, width: f64, height: f64, colour: &gdk::RGBA) {
            let (Some(mask), Some(snapshot)) =
                (self.mask.get(), snapshot.downcast_ref::<gtk::Snapshot>())
            else {
                return;
            };
            // Every pixel the colour, keeping how much of it is covered.
            let mut matrix = [0.0; 16];
            matrix[15] = colour.alpha();
            let offset = graphene::Vec4::new(colour.red(), colour.green(), colour.blue(), 0.0);
            snapshot.push_color_matrix(&graphene::Matrix::from_float(matrix), &offset);
            let bounds = graphene::Rect::new(0.0, 0.0, width as f32, height as f32);
            snapshot.append_texture(mask, &bounds);
            snapshot.pop();
        }
    }
}

glib::wrapper! {
    /// One of our icons, painted in the colour of the text around it.
    pub struct Icon(ObjectSubclass<imp::Icon>)
        @implements gdk::Paintable, gtk::SymbolicPaintable;
}

impl Icon {
    /// The icon `name` from `design/icons/`, `size` logical pixels square,
    /// drawn at twice that so it stays sharp on a screen at scale 2; an
    /// icon this release lacks paints nothing.
    pub fn new(name: &str, size: i32) -> Icon {
        let icon: Icon = glib::Object::new();
        icon.imp().size.set(size);
        let px = size.max(1) as u32 * 2;
        if let Some(pixmap) = edel::icons::mask(name, px) {
            // White where the shape is, so the colour matrix only scales.
            let data: Vec<u8> = pixmap
                .data()
                .chunks_exact(4)
                .flat_map(|p| [p[3], p[3], p[3], p[3]])
                .collect();
            let texture = gdk::MemoryTexture::new(
                px as i32,
                px as i32,
                gdk::MemoryFormat::R8g8b8a8Premultiplied,
                &glib::Bytes::from_owned(data),
                px as usize * 4,
            );
            let _ = icon.imp().mask.set(texture.upcast());
        }
        icon
    }
}

/// An image of the icon `name`, `size` logical pixels square.
pub fn image(name: &str, size: i32) -> gtk::Image {
    // Centred at its own size: GTK stretches a picture to fill a larger
    // place, as a button gives its child.
    gtk::Image::builder()
        .paintable(&Icon::new(name, size))
        .pixel_size(size)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build()
}
