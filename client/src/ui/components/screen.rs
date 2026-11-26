use gpui::{prelude::*, *};

/// A frame at its own aspect ratio, centred in a `w` x `h` box. Sized in
/// pixels: a stretched `img` (`size_full` + `ObjectFit::Contain`) drew the
/// picture wider than the box and cut its bottom.
pub fn fitted(image: std::sync::Arc<RenderImage>, w: f32, h: f32) -> AnyElement {
    let size = image.size(0);
    let (iw, ih) = (u32::from(size.width) as f32, u32::from(size.height) as f32);
    let (fw, fh) = if iw > 0. && ih > 0. {
        let k = (w / iw).min(h / ih);
        (iw * k, ih * k)
    } else {
        (w, h)
    };
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(img(image).w(px(fw)).h(px(fh)))
        .into_any_element()
}
