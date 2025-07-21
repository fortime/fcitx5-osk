use anyhow::Result;
use evdev::{uinput::VirtualDevice, AttributeSet, KeyCode, KeyEvent};

pub struct Keyboard {
    device: VirtualDevice,
    keycodes: Vec<u16>,
    pressed_keycodes: Vec<u16>,
}

impl Keyboard {
    pub fn new(keycodes: &[u16]) -> Result<Self> {
        let mut keys = AttributeSet::<KeyCode>::new();
        for keycode in keycodes {
            // X11 keycodes are +8 shift of evdev keycodes
            keys.insert(KeyCode::new(*keycode - 8));
        }
        let device = VirtualDevice::builder()?
            .name(clap::crate_name!())
            .with_keys(&keys)?
            .build()?;
        Ok(Keyboard {
            device,
            keycodes: keycodes.to_vec(),
            pressed_keycodes: vec![],
        })
    }

    pub async fn process_key_event(&mut self, keycode: u16, is_release: bool) -> Result<bool> {
        // X11 keycodes are +8 shift of evdev keycodes
        if !self.keycodes.contains(&keycode) || keycode < 8 {
            return Ok(false);
        }
        let event = if is_release {
            if let Some(pos) = self.pressed_keycodes.iter().position(|k| *k == keycode) {
                self.pressed_keycodes.swap_remove(pos);
            }
            KeyEvent::new(KeyCode(keycode - 8), 0)
        } else {
            // Save which keycode is pressed
            self.pressed_keycodes.push(keycode);
            KeyEvent::new(KeyCode(keycode - 8), 1)
        };
        self.device.emit(&[*event])?;
        Ok(true)
    }
}

impl Drop for Keyboard {
    fn drop(&mut self) {
        // Emit release event for remaining keycodes
        for keycode in self.pressed_keycodes.drain(..) {
            let event = KeyEvent::new(KeyCode(keycode - 8), 0);
            if let Err(e) = self.device.emit(&[*event]) {
                tracing::error!("Error to release key[{keycode}]: {e:?}");
            }
        }
    }
}
