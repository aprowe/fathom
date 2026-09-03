//! Input, normalised.
//!
//! Both targets capture input in the DOM: on web that is the page, on native it is the
//! transparent webview sitting over the render surface. So there is exactly one input
//! path, and these are the events that travel down it. Coordinates are viewport-local
//! device pixels, which is what the camera expects.

use serde::Deserialize;

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MouseEvent {
    pub x: f32,
    pub y: f32,
    /// The button for this event: 0 left, 1 middle, 2 right.
    pub button: u8,
    /// Bitmask of buttons currently held, as in the DOM.
    pub buttons: u8,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl MouseEvent {
    pub fn mods(&self) -> Modifiers {
        Modifiers { shift: self.shift, ctrl: self.ctrl, alt: self.alt }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScrollEvent {
    pub x: f32,
    pub y: f32,
    /// Positive scrolls down/away, as in the DOM.
    pub delta_y: f32,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct KeyEvent {
    pub key: String,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

/// One input event on its way from the interface to the app.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum InputEvent {
    MousePressed(MouseEvent),
    MouseMoved(MouseEvent),
    MouseDragged(MouseEvent),
    MouseReleased(MouseEvent),
    Scrolled(ScrollEvent),
    KeyPressed(KeyEvent),
    KeyReleased(KeyEvent),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_deserialise_from_the_interfaces_json() {
        let e: InputEvent = serde_json::from_str(
            r#"{"kind":"mouseDragged","x":12.5,"y":40.0,"buttons":1,"shift":true}"#,
        )
        .unwrap();
        match e {
            InputEvent::MouseDragged(m) => {
                assert_eq!((m.x, m.y, m.buttons), (12.5, 40.0, 1));
                assert!(m.mods().shift && !m.mods().ctrl);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn scroll_uses_the_dom_camel_case_field() {
        let e: InputEvent =
            serde_json::from_str(r#"{"kind":"scrolled","x":1.0,"y":2.0,"deltaY":-120.0}"#).unwrap();
        match e {
            InputEvent::Scrolled(s) => assert_eq!(s.delta_y, -120.0),
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn missing_optional_fields_fall_back_to_defaults() {
        let e: InputEvent = serde_json::from_str(r#"{"kind":"keyPressed","key":"r"}"#).unwrap();
        match e {
            InputEvent::KeyPressed(k) => {
                assert_eq!(k.key, "r");
                assert!(!k.ctrl);
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }
}
