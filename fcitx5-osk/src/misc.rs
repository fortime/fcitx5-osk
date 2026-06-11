use std::{
    borrow::Cow,
    hash::{Hash, Hasher},
    mem::ManuallyDrop,
    ops::{Deref, DerefMut},
};

use iced::advanced::Shell;

#[cfg(feature = "custom-action-http-api")]
pub mod secret_envelope;

pub struct NamedSubscriptionData<T> {
    name: Cow<'static, str>,
    data: T,
}

impl<T> NamedSubscriptionData<T> {
    pub fn new<S>(name: S, data: T) -> Self
    where
        S: Into<Cow<'static, str>>,
    {
        Self {
            name: name.into(),
            data,
        }
    }

    pub fn data(&self) -> &T {
        &self.data
    }
}

impl<T> Hash for NamedSubscriptionData<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

pub struct LocalShellGuard<'a, 'b, Message, LocalMessage> {
    shell: &'a mut Shell<'b, Message>,
    local_shell: ManuallyDrop<Shell<'a, LocalMessage>>,
}

impl<'a, 'b, Message, LocalMessage> LocalShellGuard<'a, 'b, Message, LocalMessage> {
    pub fn local(shell: &'a mut Shell<'b, Message>, messages: &'a mut Vec<LocalMessage>) -> Self {
        Self {
            shell,
            local_shell: ManuallyDrop::new(Shell::new(messages)),
        }
    }
}

impl<Message, LocalMessage> Drop for LocalShellGuard<'_, '_, Message, LocalMessage> {
    fn drop(&mut self) {
        if self.local_shell.is_layout_invalid() {
            self.shell.invalidate_layout();
        }

        if self.local_shell.are_widgets_invalid() {
            self.shell.invalidate_widgets();
        }

        self.shell
            .request_redraw_at(self.local_shell.redraw_request());
        if self.local_shell.is_event_captured() {
            self.shell.capture_event();
        }
        self.shell
            .input_method_mut()
            .merge(self.local_shell.input_method());
    }
}

impl<'a, Message, LocalMessage> Deref for LocalShellGuard<'a, '_, Message, LocalMessage> {
    type Target = Shell<'a, LocalMessage>;

    fn deref(&self) -> &Self::Target {
        &self.local_shell
    }
}

impl<'a, Message, LocalMessage> DerefMut for LocalShellGuard<'a, '_, Message, LocalMessage> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.local_shell
    }
}

pub trait LocalShell<'b> {
    type Message;

    fn local<'a, LocalMessage>(
        &'a mut self,
        messages: &'a mut Vec<LocalMessage>,
    ) -> LocalShellGuard<'a, 'b, Self::Message, LocalMessage>;
}

impl<'b, Message> LocalShell<'b> for Shell<'b, Message> {
    type Message = Message;

    fn local<'a, LocalMessage>(
        &'a mut self,
        messages: &'a mut Vec<LocalMessage>,
    ) -> LocalShellGuard<'a, 'b, Self::Message, LocalMessage> {
        LocalShellGuard::local(self, messages)
    }
}
