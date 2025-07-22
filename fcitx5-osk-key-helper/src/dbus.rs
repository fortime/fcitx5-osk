mod server {
    use zbus::{fdo::Error, message::Header, Connection};

    use crate::keyboard::Keyboard;

    pub struct Fcitx5OskKeyHelperControllerService {
        keyboard: Keyboard,
        serial: u64,
    }

    impl Fcitx5OskKeyHelperControllerService {
        pub fn new(keyboard: Keyboard) -> Self {
            Self {
                keyboard,
                serial: 0,
            }
        }

        pub async fn start(self, conn: &Connection) -> Result<(), Error> {
            conn.object_server().at(Self::OBJECT_PATH, self).await?;
            conn.request_name(Self::SERVICE_NAME).await?;
            Ok(())
        }
    }

    #[zbus::interface(name = "fyi.fortime.Fcitx5OskKeyHelper.Controller1")]
    impl Fcitx5OskKeyHelperControllerService {
        const SERVICE_NAME: &'static str = "fyi.fortime.Fcitx5OskKeyHelper";
        const OBJECT_PATH: &'static str = "/fyi/fortime/Fcitx5OskKeyHelper/Controller";

        fn next_serial(&mut self) -> u64 {
            let old_serial = self.serial;
            self.serial = rand::random();
            tracing::info!("Serial is changed from {} to {}", old_serial, self.serial);
            self.serial
        }

        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        pub async fn reset_serial(
            &mut self,
            #[zbus(header)] header: Header<'_>,
        ) -> Result<u64, Error> {
            tracing::info!("Reset serial request from sender: {:?}", header.sender(),);
            // Reset all pressed keys
            self.keyboard.reset();
            Ok(self.next_serial())
        }

        #[tracing::instrument(level = "debug", skip(self), err, ret)]
        pub async fn process_key_event(
            &mut self,
            serial: u64,
            keycode: u16,
            is_release: bool,
        ) -> Result<u64, Error> {
            if serial != self.serial {
                tracing::warn!(
                    "Process key event, expect serial: {}, but {}",
                    self.serial,
                    serial
                );
                return Err(Error::InvalidArgs("Invalid serial".to_string()));
            }
            match self.keyboard.process_key_event(keycode, is_release).await {
                Ok(true) => Ok(self.next_serial()),
                Ok(false) => Err(Error::InvalidArgs(format!(
                    "Unsupported keycode: {}",
                    keycode
                ))),
                Err(e) => {
                    tracing::error!("Process key event failed: {:?}", e);
                    Err(Error::Failed(format!(
                        "Unable to handle process key event request, keycode: {}, is_release: {}",
                        keycode, is_release,
                    )))
                }
            }
        }
    }
}

pub use server::Fcitx5OskKeyHelperControllerService;
